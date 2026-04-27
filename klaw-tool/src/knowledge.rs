use std::sync::Arc;

use async_trait::async_trait;
use klaw_config::{AppConfig, KnowledgeToolConfig};
use klaw_knowledge::{
    CreateKnowledgeNoteInput, KnowledgeEntry, KnowledgeHit, KnowledgeProvider,
    KnowledgeSearchQuery, assemble_context_bundle, open_configured_obsidian_provider,
};
use serde_json::{Value, json};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

#[derive(Debug, Clone)]
struct KnowledgeToolRuntimeConfig {
    search_limit: usize,
    context_limit: usize,
}

impl KnowledgeToolRuntimeConfig {
    fn from_tool_config(config: &KnowledgeToolConfig) -> Self {
        Self {
            search_limit: config.search_limit,
            context_limit: config.context_limit,
        }
    }
}

pub struct KnowledgeTool {
    provider: Arc<dyn KnowledgeProvider>,
    runtime: KnowledgeToolRuntimeConfig,
}

impl KnowledgeTool {
    pub async fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let provider = open_configured_obsidian_provider(config)
            .await
            .map_err(map_knowledge_error)?;
        Ok(Self::with_provider(
            Arc::new(provider),
            &config.tools.knowledge,
        ))
    }

    pub fn with_provider(
        provider: Arc<dyn KnowledgeProvider>,
        config: &KnowledgeToolConfig,
    ) -> Self {
        Self {
            provider,
            runtime: KnowledgeToolRuntimeConfig::from_tool_config(config),
        }
    }

    #[cfg(test)]
    fn from_provider(
        provider: Arc<dyn KnowledgeProvider>,
        runtime: KnowledgeToolRuntimeConfig,
    ) -> Self {
        Self { provider, runtime }
    }

    fn require_action(args: &Value) -> Result<&str, ToolError> {
        args.get("action")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_string()))
    }

    fn require_string(args: &Value, key: &'static str) -> Result<String, ToolError> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| ToolError::InvalidArgs(format!("missing `{key}`")))
    }

    fn require_nonempty_content(args: &Value, key: &'static str) -> Result<String, ToolError> {
        args.get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| ToolError::InvalidArgs(format!("missing `{key}`")))
    }

    fn parse_limit(value: Option<u64>, fallback: usize) -> Result<usize, ToolError> {
        match value {
            Some(limit) if limit > 0 => Ok(limit as usize),
            Some(_) => Err(ToolError::InvalidArgs(
                "`limit` must be a positive integer".to_string(),
            )),
            None => Ok(fallback),
        }
    }

    fn parse_tags(args: &Value) -> Result<Option<Vec<String>>, ToolError> {
        let Some(tags) = args.get("tags") else {
            return Ok(None);
        };
        let values = tags.as_array().ok_or_else(|| {
            ToolError::InvalidArgs("`tags` must be an array of strings".to_string())
        })?;
        let mut parsed = Vec::new();
        for value in values {
            let tag = value.as_str().ok_or_else(|| {
                ToolError::InvalidArgs("`tags` must be an array of strings".to_string())
            })?;
            parsed.push(tag.trim().to_string());
        }
        Ok(Some(parsed))
    }

    fn validate_source(args: &Value) -> Result<(), ToolError> {
        let Some(source) = args.get("source") else {
            return Ok(());
        };
        let source = source
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("`source` must be a string".to_string()))?;
        let source = source.trim();
        if source.is_empty() || matches!(source, "obsidian" | "Obsidian Vault") {
            return Ok(());
        }
        Err(ToolError::InvalidArgs(format!(
            "`source` must match the configured Obsidian provider, got `{source}`"
        )))
    }

    async fn run_search(&self, args: &Value) -> Result<Vec<KnowledgeHit>, ToolError> {
        let query = Self::require_string(args, "query")?;
        let limit = Self::parse_limit(
            args.get("limit").and_then(Value::as_u64),
            self.runtime.search_limit,
        )?;
        self.provider
            .search(KnowledgeSearchQuery {
                text: query,
                tags: Self::parse_tags(args)?,
                source: args
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                limit,
                mode: args
                    .get("mode")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
            })
            .await
            .map_err(map_knowledge_error)
    }
}

#[async_trait]
impl Tool for KnowledgeTool {
    fn name(&self) -> &str {
        "knowledge"
    }

    fn description(&self) -> &str {
        "Access the user's connected knowledge base (e.g. an Obsidian vault) to retrieve or store information. Use `search` to find relevant notes by keywords or tags. Use `get` to fetch a specific note by its ID. Use `context` to assemble a compact, budget-controlled context bundle for injection into the conversation. Use `list_sources` to discover available knowledge sources. Use `create_note` to write a new note into the vault when the user asks you to save or record information persistently."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Interact with the user's knowledge base (Obsidian vault). Choose an action to search notes, retrieve a specific entry, build a compact context bundle, discover available sources, or create a new note.",
            "oneOf": [
                {
                    "description": "List all configured knowledge sources (e.g. Obsidian vault name, provider, entry count). Use this first to discover what sources are available before searching.",
                    "properties": {
                        "action": {
                            "const": "list_sources",
                            "description": "List available knowledge sources and their metadata."
                        }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "Search the knowledge base for notes matching a query. Returns ranked hits with title, excerpt, score, and tags. Use this when you need relevant reference material on a topic.",
                    "properties": {
                        "action": {
                            "const": "search",
                            "description": "Search knowledge base for relevant notes."
                        },
                        "query": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Natural language search query describing the topic or question. Use specific keywords for better results, e.g. 'Rust async error handling' or 'project deployment workflow'."
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Filter results to notes that contain at least one of these Obsidian tags (without the # prefix). Example: ['project-alpha', 'meeting-notes']."
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of search hits to return. Defaults to the runtime-configured search limit (typically 10)."
                        },
                        "source": {
                            "type": "string",
                            "description": "Restrict search to a specific knowledge source by name. Must match a source returned by `list_sources`. Example: 'Obsidian Vault'."
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["keyword", "semantic", "hybrid"],
                            "description": "Search retrieval mode. 'keyword' matches exact terms, 'semantic' uses embedding similarity for conceptual matches, 'hybrid' combines both. Defaults to the provider's default mode."
                        }
                    },
                    "required": ["action", "query"],
                    "additionalProperties": false
                },
                {
                    "description": "Retrieve a single knowledge entry by its unique ID. Returns the full note content, metadata, tags, and timestamps. Use this when you already have an entry ID (e.g. from a prior search hit) and need the complete document.",
                    "properties": {
                        "action": {
                            "const": "get",
                            "description": "Retrieve a specific knowledge entry by ID."
                        },
                        "id": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Unique identifier of the knowledge entry to retrieve. This is the `id` field returned by `search` or `list_sources`."
                        }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "description": "Search the knowledge base and assemble a compact context bundle that fits within a character budget. Use this instead of `search` when you need a ready-to-inject summary of relevant knowledge that won't overwhelm the conversation context.",
                    "properties": {
                        "action": {
                            "const": "context",
                            "description": "Assemble a budget-controlled context bundle from search results."
                        },
                        "query": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Natural language query describing the topic for which context is needed."
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of search hits to consider for the bundle. Defaults to the runtime-configured context limit (typically 5)."
                        },
                        "budget_chars": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum total character length of the assembled context bundle. Excerpts are trimmed to fit within this budget. Defaults to 2000."
                        }
                    },
                    "required": ["action", "query"],
                    "additionalProperties": false
                },
                {
                    "description": "Create a new note in the user's Obsidian vault. Use this when the user explicitly asks you to save, record, or write information into their knowledge base, or when a conversation result should be persisted as a vault note.",
                    "properties": {
                        "action": {
                            "const": "create_note",
                            "description": "Create a new note in the knowledge base."
                        },
                        "path": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Vault-relative path for the new note, including the filename with a .md extension. Subfolders are created automatically. Example: 'projects/alpha/meeting-2024-01-15.md'."
                        },
                        "content": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Full Markdown content of the note. Obsidian wikilinks, tags, and frontmatter (YAML) are supported."
                        },
                        "source": {
                            "type": "string",
                            "description": "Target knowledge source where the note should be created. Must match a source returned by `list_sources`. Defaults to the primary configured vault."
                        }
                    },
                    "required": ["action", "path", "content"],
                    "additionalProperties": false
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Knowledge
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let payload = match action {
            "list_sources" => json!({
                "action": "list_sources",
                "sources": self.provider.list_sources().await.map_err(map_knowledge_error)?,
            }),
            "search" => json!({
                "action": "search",
                "hits": self.run_search(&args).await?,
            }),
            "get" => {
                let id = Self::require_string(&args, "id")?;
                let entry: Option<KnowledgeEntry> =
                    self.provider.get(&id).await.map_err(map_knowledge_error)?;
                json!({
                    "action": "get",
                    "entry": entry,
                })
            }
            "context" => {
                let query = Self::require_string(&args, "query")?;
                let limit = Self::parse_limit(
                    args.get("limit").and_then(Value::as_u64),
                    self.runtime.context_limit,
                )?;
                let budget_chars =
                    Self::parse_limit(args.get("budget_chars").and_then(Value::as_u64), 2_000)?;
                let hits = self
                    .provider
                    .search(KnowledgeSearchQuery {
                        text: query.clone(),
                        limit,
                        ..Default::default()
                    })
                    .await
                    .map_err(map_knowledge_error)?;
                json!({
                    "action": "context",
                    "bundle": assemble_context_bundle(&query, &hits, budget_chars),
                })
            }
            "create_note" => {
                Self::validate_source(&args)?;
                let path = Self::require_string(&args, "path")?;
                let content = Self::require_nonempty_content(&args, "content")?;
                let entry = self
                    .provider
                    .create_note(CreateKnowledgeNoteInput { path, content })
                    .await
                    .map_err(map_knowledge_error)?;
                let path = entry.uri.clone();
                let id = entry.id.clone();
                let uri = entry.uri.clone();
                json!({
                    "action": "create_note",
                    "created": true,
                    "path": path,
                    "id": id,
                    "uri": uri,
                    "entry": entry,
                })
            }
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of list_sources/search/get/context/create_note"
                        .to_string(),
                ));
            }
        };

        let rendered = serde_json::to_string_pretty(&payload)
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
            media: Vec::new(),
            signals: Vec::new(),
        })
    }
}

fn map_knowledge_error(err: klaw_knowledge::KnowledgeError) -> ToolError {
    match err {
        klaw_knowledge::KnowledgeError::InvalidConfig(message)
        | klaw_knowledge::KnowledgeError::InvalidQuery(message)
        | klaw_knowledge::KnowledgeError::InvalidNotePath(message) => {
            ToolError::InvalidArgs(message)
        }
        other => ToolError::ExecutionFailed(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use klaw_knowledge::{CreateKnowledgeNoteInput, KnowledgeError, KnowledgeSourceInfo};
    use serde_json::json;

    use super::*;

    static HOME_ENV_LOCK: Mutex<Option<OsString>> = Mutex::new(None);
    static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

    #[derive(Clone)]
    struct MockProvider;

    struct HomeEnvGuard {
        original: Option<OsString>,
    }

    impl HomeEnvGuard {
        fn set(home: &std::path::Path) -> Self {
            let original = std::env::var_os("HOME");
            // SAFETY: This test holds HOME_ENV_LOCK for the full guard lifetime, so
            // this crate's HOME-dependent test setup is serialized and restored.
            unsafe {
                std::env::set_var("HOME", home);
            }
            Self { original }
        }
    }

    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            // SAFETY: HOME_ENV_LOCK is still held when this guard is dropped, so
            // restoring the process environment is serialized with matching tests.
            unsafe {
                if let Some(original) = &self.original {
                    std::env::set_var("HOME", original);
                } else {
                    std::env::remove_var("HOME");
                }
            }
        }
    }

    #[async_trait]
    impl KnowledgeProvider for MockProvider {
        fn provider_name(&self) -> &str {
            "mock"
        }

        async fn search(
            &self,
            query: KnowledgeSearchQuery,
        ) -> Result<Vec<KnowledgeHit>, KnowledgeError> {
            Ok(vec![KnowledgeHit {
                id: query.text.clone(),
                title: "Auth".to_string(),
                excerpt: "OAuth context".to_string(),
                score: 0.9,
                tags: vec!["auth".to_string()],
                uri: "vault/auth.md".to_string(),
                source: "mock".to_string(),
                metadata: json!({}),
            }])
        }

        async fn get(&self, id: &str) -> Result<Option<KnowledgeEntry>, KnowledgeError> {
            Ok(Some(KnowledgeEntry {
                id: id.to_string(),
                title: "Auth".to_string(),
                content: "OAuth full content".to_string(),
                tags: vec!["auth".to_string()],
                uri: "vault/auth.md".to_string(),
                source: "mock".to_string(),
                metadata: json!({}),
                created_at_ms: 1,
                updated_at_ms: 1,
            }))
        }

        async fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError> {
            Ok(vec![KnowledgeSourceInfo {
                provider: "mock".to_string(),
                name: "Mock Vault".to_string(),
                description: "mock".to_string(),
                entry_count: 1,
            }])
        }

        async fn create_note(
            &self,
            input: CreateKnowledgeNoteInput,
        ) -> Result<KnowledgeEntry, KnowledgeError> {
            Ok(KnowledgeEntry {
                id: input.path.clone(),
                title: "Created".to_string(),
                content: input.content,
                tags: vec!["created".to_string()],
                uri: input.path,
                source: "mock".to_string(),
                metadata: json!({}),
                created_at_ms: 2,
                updated_at_ms: 2,
            })
        }
    }

    fn tool() -> KnowledgeTool {
        KnowledgeTool::from_provider(
            Arc::new(MockProvider),
            KnowledgeToolRuntimeConfig {
                search_limit: 5,
                context_limit: 3,
            },
        )
    }

    fn ctx() -> ToolContext {
        ToolContext {
            session_key: "s1".to_string(),
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn open_default_does_not_reindex_vault() {
        let _home_lock = HOME_ENV_LOCK
            .lock()
            .expect("HOME env lock should not poison");
        let test_id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "klaw-tool-knowledge-open-default-{}-{test_id}",
            std::process::id()
        ));
        let home = root.join("home");
        let vault = root.join("vault");
        std::fs::create_dir_all(&vault).expect("vault dir should be created");
        std::fs::write(
            vault.join("note.md"),
            "# Note\n\nknowledge startup content\n",
        )
        .expect("vault note should be written");
        let _home_guard = HomeEnvGuard::set(&home);

        let mut config = AppConfig::default();
        config.knowledge.enabled = true;
        config.knowledge.obsidian.vault_path = Some(vault.display().to_string());
        config.knowledge.obsidian.auto_index = true;
        config.tools.knowledge.enabled = true;

        let tool = KnowledgeTool::open_default(&config)
            .await
            .expect("knowledge tool should open");
        let sources = tool
            .provider
            .list_sources()
            .await
            .expect("sources should load");

        assert_eq!(sources.first().map(|source| source.entry_count), Some(0));
    }

    #[tokio::test]
    async fn search_action_returns_hits() {
        let output = tool()
            .execute(json!({"action":"search","query":"auth"}), &ctx())
            .await
            .expect("search should succeed");
        assert!(output.content_for_model.contains("\"hits\""));
        assert!(output.content_for_model.contains("OAuth context"));
    }

    #[tokio::test]
    async fn context_action_returns_bundle() {
        let output = tool()
            .execute(
                json!({"action":"context","query":"auth","budget_chars":120}),
                &ctx(),
            )
            .await
            .expect("context should succeed");
        assert!(output.content_for_model.contains("\"bundle\""));
        assert!(output.content_for_model.contains("Direct match"));
    }

    #[tokio::test]
    async fn create_note_action_returns_created_entry() {
        let output = tool()
            .execute(
                json!({"action":"create_note","path":"notes/new.md","content":"# New note"}),
                &ctx(),
            )
            .await
            .expect("create_note should succeed");
        assert!(output.content_for_model.contains("\"created\": true"));
        assert!(output.content_for_model.contains("notes/new.md"));
    }

    #[tokio::test]
    async fn create_note_rejects_unknown_source() {
        let err = tool()
            .execute(
                json!({"action":"create_note","path":"notes/new.md","content":"# New note","source":"notion"}),
                &ctx(),
            )
            .await
            .expect_err("create_note should reject unsupported source");
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }
}
