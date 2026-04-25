use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use klaw_config::{AppConfig, KnowledgeToolConfig};
use klaw_knowledge::{
    KnowledgeEntry, KnowledgeHit, KnowledgeProvider, KnowledgeSearchQuery, assemble_context_bundle,
    build_local_embedding_model, build_local_orchestrator, build_local_reranker,
    obsidian::provider::ObsidianKnowledgeProvider,
};
use klaw_storage::open_default_knowledge_db;
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
        let provider_name = config.knowledge.provider.trim();
        if provider_name != "obsidian" {
            return Err(ToolError::InvalidArgs(format!(
                "unsupported knowledge provider '{provider_name}'"
            )));
        }
        let vault_path = config
            .knowledge
            .obsidian
            .vault_path
            .as_ref()
            .map(|path| PathBuf::from(path.trim()))
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| {
                ToolError::InvalidArgs(
                    "knowledge.obsidian.vault_path must be configured for the knowledge tool"
                        .to_string(),
                )
            })?;
        let db = Arc::new(
            open_default_knowledge_db()
                .await
                .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?,
        );
        let provider = ObsidianKnowledgeProvider::open(
            db,
            vault_path,
            config.knowledge.obsidian.exclude_folders.clone(),
            config.knowledge.obsidian.max_excerpt_length,
            false,
            "Obsidian Vault",
        )
        .await
        .map_err(map_knowledge_error)?;
        let provider = if let Some(embedder) =
            build_local_embedding_model(config).map_err(map_knowledge_error)?
        {
            provider.with_embedding_model(Arc::new(embedder))
        } else {
            provider
        };
        let provider =
            if let Some(reranker) = build_local_reranker(config).map_err(map_knowledge_error)? {
                provider.with_reranker(Arc::new(reranker))
            } else {
                provider
            };
        let provider = if let Some(orchestrator) =
            build_local_orchestrator(config).map_err(map_knowledge_error)?
        {
            provider.with_orchestrator(Arc::new(orchestrator))
        } else {
            provider
        };
        if config.knowledge.obsidian.index_on_startup {
            provider.reindex().await.map_err(map_knowledge_error)?;
        }
        Ok(Self {
            provider: Arc::new(provider),
            runtime: KnowledgeToolRuntimeConfig::from_tool_config(&config.tools.knowledge),
        })
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
        "Search connected knowledge bases such as an Obsidian vault for relevant notes, documents, and reference material. Use this when the answer may exist in the user's personal or project knowledge sources."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Search read-only knowledge sources, fetch a specific entry, or assemble a context bundle.",
            "oneOf": [
                {
                    "properties": {
                        "action": { "const": "list_sources" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "search" },
                        "query": { "type": "string", "minLength": 1 },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "limit": { "type": "integer", "minimum": 1 },
                        "source": { "type": "string" },
                        "mode": { "type": "string" }
                    },
                    "required": ["action", "query"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "get" },
                        "id": { "type": "string", "minLength": 1 }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "context" },
                        "query": { "type": "string", "minLength": 1 },
                        "limit": { "type": "integer", "minimum": 1 },
                        "budget_chars": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["action", "query"],
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
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of list_sources/search/get/context".to_string(),
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
        | klaw_knowledge::KnowledgeError::InvalidQuery(message) => ToolError::InvalidArgs(message),
        other => ToolError::ExecutionFailed(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use klaw_knowledge::{KnowledgeError, KnowledgeSourceInfo};
    use serde_json::json;

    use super::*;

    #[derive(Clone)]
    struct MockProvider;

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
}
