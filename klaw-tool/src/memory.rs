use async_trait::async_trait;
use klaw_config::{AppConfig, MemoryToolConfig};
use klaw_memory::{
    MemoryError, MemoryRecord, MemoryService, SqliteMemoryService, UpsertMemoryInput,
    govern_long_term_write,
};
use klaw_storage::{DefaultSessionStore, SessionStorage};
use serde_json::{Value, json};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const MAX_SEARCH_LIMIT: usize = 50;
const DEFAULT_SESSION_WITHIN_DAYS: i64 = 3;
const MAX_SESSION_SCAN_RECORDS: usize = 1000;
const LONG_TERM_SCOPE: &str = "long_term";

#[derive(Debug, Clone)]
struct MemoryToolRuntimeConfig {
    search_limit: usize,
}

impl MemoryToolRuntimeConfig {
    fn from_config(config: &AppConfig) -> Self {
        Self::from_tool_config(&config.tools.memory)
    }

    fn from_tool_config(config: &MemoryToolConfig) -> Self {
        Self {
            search_limit: config.search_limit.min(MAX_SEARCH_LIMIT),
        }
    }
}

impl Default for MemoryToolRuntimeConfig {
    fn default() -> Self {
        Self::from_tool_config(&MemoryToolConfig::default())
    }
}

pub struct MemoryTool {
    service: Arc<dyn MemoryService>,
    session_store: Arc<DefaultSessionStore>,
    runtime: MemoryToolRuntimeConfig,
}

impl MemoryTool {
    pub async fn open_default(
        config: &AppConfig,
        session_store: DefaultSessionStore,
    ) -> Result<Self, ToolError> {
        let service = SqliteMemoryService::open_default(config)
            .await
            .map_err(map_memory_err)?;
        Ok(Self {
            service: Arc::new(service),
            session_store: Arc::new(session_store),
            runtime: MemoryToolRuntimeConfig::from_config(config),
        })
    }

    pub fn with_store(
        service: Arc<dyn MemoryService>,
        session_store: DefaultSessionStore,
        config: &AppConfig,
    ) -> Self {
        Self {
            service,
            session_store: Arc::new(session_store),
            runtime: MemoryToolRuntimeConfig::from_config(config),
        }
    }

    #[cfg(test)]
    fn from_parts(
        service: Arc<dyn MemoryService>,
        session_store: DefaultSessionStore,
        runtime: MemoryToolRuntimeConfig,
    ) -> Self {
        Self {
            service,
            session_store: Arc::new(session_store),
            runtime,
        }
    }

    #[cfg(test)]
    fn from_service(service: Arc<dyn MemoryService>, session_store: DefaultSessionStore) -> Self {
        Self::from_parts(service, session_store, MemoryToolRuntimeConfig::default())
    }

    fn require_action(args: &Value) -> Result<&str, ToolError> {
        args.get("action")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_string()))
    }

    fn parse_metadata(args: &Value) -> Result<Value, ToolError> {
        match args.get("metadata") {
            Some(value @ Value::Object(_)) => Ok(value.clone()),
            Some(_) => Err(ToolError::InvalidArgs(
                "`metadata` must be a JSON object".to_string(),
            )),
            None => Ok(json!({})),
        }
    }

    fn require_nonempty_string(args: &Value, key: &'static str) -> Result<String, ToolError> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| ToolError::InvalidArgs(format!("missing `{key}`")))
    }

    fn parse_bool(args: &Value, key: &str, default_value: bool) -> Result<bool, ToolError> {
        match args.get(key) {
            Some(v) => v
                .as_bool()
                .ok_or_else(|| ToolError::InvalidArgs(format!("`{key}` must be a boolean"))),
            None => Ok(default_value),
        }
    }

    fn reject_add_scope(args: &Value) -> Result<(), ToolError> {
        if args.get("scope").is_some() {
            return Err(ToolError::InvalidArgs(
                "`scope` is not supported for `add`; persistent memories are always stored as `long_term`"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn parse_search_scope(args: &Value) -> Result<&str, ToolError> {
        match args.get("scope").and_then(Value::as_str) {
            Some("session") | None => Ok("session"),
            Some(other) => Err(ToolError::InvalidArgs(format!(
                "`scope` must be `session`, got `{other}`"
            ))),
        }
    }

    fn parse_limit(&self, args: &Value) -> Result<usize, ToolError> {
        match args.get("limit").and_then(Value::as_u64) {
            Some(limit) if limit > 0 => Ok((limit as usize).min(self.runtime.search_limit)),
            Some(_) => Err(ToolError::InvalidArgs(
                "`limit` must be a positive integer".to_string(),
            )),
            None => Ok(self.runtime.search_limit),
        }
    }

    fn parse_within_days(args: &Value) -> Result<i64, ToolError> {
        match args.get("within_days").and_then(Value::as_i64) {
            Some(days) if days > 0 => Ok(days),
            Some(_) => Err(ToolError::InvalidArgs(
                "`within_days` must be a positive integer".to_string(),
            )),
            None => Ok(DEFAULT_SESSION_WITHIN_DAYS),
        }
    }

    async fn resolve_session_scope(
        &self,
        ctx: &ToolContext,
    ) -> Result<ResolvedSessionScope, ToolError> {
        let from_metadata = ctx
            .metadata
            .get("channel.base_session_key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let base_session_key = match from_metadata {
            Some(key) => key,
            None => self
                .session_store
                .get_session_by_active_session_key(&ctx.session_key)
                .await
                .map(|base| base.session_key)
                .unwrap_or_else(|_| ctx.session_key.clone()),
        };

        let active_session_key = self
            .session_store
            .get_session(&base_session_key)
            .await
            .ok()
            .and_then(|session| session.active_session_key)
            .filter(|value| !value.trim().is_empty());
        let mut session_keys = vec![base_session_key.clone()];
        if let Some(active_session_key) = active_session_key {
            if active_session_key != base_session_key {
                session_keys.push(active_session_key);
            }
        }
        if ctx.session_key != base_session_key && !session_keys.contains(&ctx.session_key) {
            session_keys.push(ctx.session_key.clone());
        }
        Ok(ResolvedSessionScope {
            base_session_key,
            session_keys,
        })
    }

    async fn search_session_history(
        &self,
        scope: &ResolvedSessionScope,
        query: &str,
        within_days: i64,
        limit: usize,
    ) -> Result<Vec<SessionSearchHit>, ToolError> {
        let cutoff_ms = (OffsetDateTime::now_utc() - Duration::days(within_days))
            .unix_timestamp_nanos()
            .saturating_div(1_000_000) as i64;
        let mut hits = Vec::new();
        for session_key in &scope.session_keys {
            let records = self
                .session_store
                .read_chat_records(session_key)
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!("read session history failed: {err}"))
                })?;
            for record in records.into_iter().rev().take(MAX_SESSION_SCAN_RECORDS) {
                if record.ts_ms < cutoff_ms {
                    continue;
                }
                if !matches!(record.role.as_str(), "user" | "assistant") {
                    continue;
                }
                let Some(score) = session_match_score(&record.content, query) else {
                    continue;
                };
                hits.push(SessionSearchHit {
                    session_key: session_key.clone(),
                    ts_ms: record.ts_ms,
                    role: record.role,
                    content: record.content,
                    score,
                });
            }
        }
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.ts_ms.cmp(&a.ts_ms))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    async fn add_long_term_memory(
        &self,
        content: String,
        metadata: Value,
        pinned: bool,
    ) -> Result<Value, ToolError> {
        let existing_records = self
            .service
            .list_scope_records(LONG_TERM_SCOPE)
            .await
            .map_err(map_memory_err)?;
        let plan = govern_long_term_write(
            &existing_records,
            UpsertMemoryInput {
                id: None,
                scope: LONG_TERM_SCOPE.to_string(),
                content,
                metadata,
                pinned,
            },
        )
        .map_err(map_memory_err)?;
        let record = self
            .service
            .upsert(plan.primary)
            .await
            .map_err(map_memory_err)?;
        for update in plan.superseded_updates {
            self.service.upsert(update).await.map_err(map_memory_err)?;
        }
        Ok(json!({
            "action": "add",
            "record": record_to_json(record),
            "governance": {
                "kind": plan.kind.as_str(),
                "reused_existing_id": plan.reused_existing_id,
                "supersedes": plan.supersedes_ids,
            }
        }))
    }
}

#[derive(Debug, Clone)]
struct ResolvedSessionScope {
    base_session_key: String,
    session_keys: Vec<String>,
}

#[derive(Debug, Clone)]
struct SessionSearchHit {
    session_key: String,
    ts_ms: i64,
    role: String,
    content: String,
    score: f64,
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Persistent memory store for the agent. Use `add` to save durable long-term facts. Use `search` with `scope=\"session\"` to retrieve recent session history."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Add durable long-term memories or search recent session history.",
            "oneOf": [
                {
                    "description": "Store a durable long-term fact, preference, rule, or constraint that should be injected into future system prompts.",
                    "properties": {
                        "action": { "const": "add" },
                        "content": {
                            "type": "string",
                            "description": "The fact or context to remember."
                        },
                        "metadata": {
                            "type": "object",
                            "description": "Optional structured metadata for this memory. Supported governance fields: `kind` (`identity|preference|project_rule|workflow|fact|constraint`), optional `topic` for conflict replacement, and optional `supersedes` (string or string array). `status` is system-managed and only `active` is accepted on new writes.",
                            "additionalProperties": true
                        },
                        "pinned": {
                            "type": "boolean",
                            "description": "If true, this memory appears first in search results.",
                            "default": false
                        }
                    },
                    "required": ["action", "content"],
                    "additionalProperties": false
                },
                {
                    "description": "Search recent session history for relevant prior conversation turns. Session search only reads session-scoped chat history.",
                    "properties": {
                        "action": { "const": "search" },
                        "query": {
                            "type": "string",
                            "description": "Keywords or a short phrase to search for in session history."
                        },
                        "scope": {
                            "type": "string",
                            "enum": ["session"],
                            "description": "Session history search scope. Only `session` is supported."
                        },
                        "within_days": {
                            "type": "integer",
                            "description": "Only search session history updated within this many days. Defaults to 3.",
                            "minimum": 1
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of hits to return. Clamped by runtime config.",
                            "minimum": 1
                        }
                    },
                    "required": ["action", "query"],
                    "additionalProperties": false
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Memory
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let payload = match action {
            "add" => {
                Self::reject_add_scope(&args)?;
                let content = Self::require_nonempty_string(&args, "content")?;
                let metadata = Self::parse_metadata(&args)?;
                let pinned = Self::parse_bool(&args, "pinned", false)?;
                self.add_long_term_memory(content, metadata, pinned).await?
            }
            "search" => {
                let scope = Self::parse_search_scope(&args)?;
                let query = Self::require_nonempty_string(&args, "query")?;
                let within_days = Self::parse_within_days(&args)?;
                let limit = self.parse_limit(&args)?;
                let resolved_scope = self.resolve_session_scope(ctx).await?;
                let hits = self
                    .search_session_history(&resolved_scope, &query, within_days, limit)
                    .await?;

                json!({
                    "action": "search",
                    "scope": scope,
                    "base_session_key": resolved_scope.base_session_key,
                    "session_keys": resolved_scope.session_keys,
                    "within_days": within_days,
                    "limit": limit,
                    "hits": hits.into_iter().map(session_hit_to_json).collect::<Vec<_>>()
                })
            }
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of add/search".to_string(),
                ));
            }
        };

        let rendered = serde_json::to_string_pretty(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize memory output failed: {err}"))
        })?;

        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
            media: Vec::new(),
            signals: Vec::new(),
        })
    }
}

fn map_memory_err(err: MemoryError) -> ToolError {
    match err {
        MemoryError::InvalidConfig(message)
        | MemoryError::InvalidQuery(message)
        | MemoryError::CapabilityUnavailable(message) => ToolError::InvalidArgs(message),
        other => ToolError::ExecutionFailed(other.to_string()),
    }
}

fn record_to_json(record: MemoryRecord) -> Value {
    json!({
        "id": record.id,
        "scope": record.scope,
        "content": record.content,
        "metadata": record.metadata,
        "pinned": record.pinned,
        "created_at_ms": record.created_at_ms,
        "updated_at_ms": record.updated_at_ms
    })
}

fn session_hit_to_json(hit: SessionSearchHit) -> Value {
    json!({
        "session_key": hit.session_key,
        "ts_ms": hit.ts_ms,
        "role": hit.role,
        "content": hit.content,
        "score": hit.score
    })
}

fn session_match_score(content: &str, query: &str) -> Option<f64> {
    let normalized_content = content.to_ascii_lowercase();
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return None;
    }

    let phrase_match = normalized_content.contains(&normalized_query);
    let tokens = normalized_query
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let token_hits = tokens
        .iter()
        .filter(|token| normalized_content.contains(**token))
        .count();
    if !phrase_match && token_hits == 0 {
        return None;
    }

    let token_score = if tokens.is_empty() {
        0.0
    } else {
        token_hits as f64 / tokens.len() as f64
    };
    Some(if phrase_match {
        2.0 + token_score
    } else {
        token_score
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use klaw_storage::{ChatRecord, StoragePaths};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct MockMemoryService {
        upsert_inputs: Mutex<Vec<UpsertMemoryInput>>,
        scope_records: Mutex<Vec<MemoryRecord>>,
    }

    #[async_trait]
    impl MemoryService for MockMemoryService {
        async fn upsert(&self, input: UpsertMemoryInput) -> Result<MemoryRecord, MemoryError> {
            self.upsert_inputs.lock().await.push(input.clone());
            Ok(MemoryRecord {
                id: input.id.unwrap_or_else(|| "generated-id".to_string()),
                scope: input.scope,
                content: input.content,
                metadata: input.metadata,
                pinned: input.pinned,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
        }

        async fn list_scope_records(&self, scope: &str) -> Result<Vec<MemoryRecord>, MemoryError> {
            Ok(self
                .scope_records
                .lock()
                .await
                .iter()
                .filter(|record| record.scope == scope)
                .cloned()
                .collect())
        }

        async fn search(
            &self,
            _query: klaw_memory::MemorySearchQuery,
        ) -> Result<Vec<klaw_memory::MemoryHit>, MemoryError> {
            Ok(Vec::new())
        }

        async fn get(&self, _id: &str) -> Result<Option<MemoryRecord>, MemoryError> {
            Ok(None)
        }

        async fn delete(&self, _id: &str) -> Result<bool, MemoryError> {
            Ok(false)
        }

        async fn pin(&self, _id: &str, _pinned: bool) -> Result<Option<MemoryRecord>, MemoryError> {
            Ok(None)
        }
    }

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("klaw-memory-tool-test-{suffix}-{}", Uuid::new_v4()));
        DefaultSessionStore::open(StoragePaths::from_root(base))
            .await
            .expect("session store should open")
    }

    fn tool_with_mock(mock: Arc<MockMemoryService>, store: DefaultSessionStore) -> MemoryTool {
        MemoryTool::from_service(mock, store)
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_key: "session-123".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn add_stores_long_term_memory_and_generates_id() {
        let mock = Arc::new(MockMemoryService::default());
        let tool = tool_with_mock(mock.clone(), create_store().await);

        let output = tool
            .execute(
                json!({
                    "action": "add",
                    "content": "remember this"
                }),
                &test_ctx(),
            )
            .await
            .expect("add should succeed");

        let captured = mock.upsert_inputs.lock().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].scope, "long_term");
        assert!(captured[0].id.is_some());
        assert_eq!(captured[0].metadata["kind"], "fact");
        assert_eq!(captured[0].metadata["status"], "active");
        assert!(output.content_for_model.contains("remember this"));
    }

    #[tokio::test]
    async fn add_applies_governance_kind_topic_and_conflict_replacement() {
        let mock = Arc::new(MockMemoryService::default());
        {
            let mut scope_records = mock.scope_records.lock().await;
            scope_records.push(MemoryRecord {
                id: "old-pref".to_string(),
                scope: "long_term".to_string(),
                content: "Default language is English.".to_string(),
                metadata: json!({
                    "kind": "preference",
                    "topic": "reply_language",
                    "status": "active"
                }),
                pinned: false,
                created_at_ms: 1,
                updated_at_ms: 1,
            });
        }
        let tool = tool_with_mock(mock.clone(), create_store().await);

        let output = tool
            .execute(
                json!({
                    "action": "add",
                    "content": "Default language is Chinese.",
                    "metadata": {
                        "kind": "preference",
                        "topic": "reply_language"
                    }
                }),
                &test_ctx(),
            )
            .await
            .expect("governed add should succeed");

        let captured = mock.upsert_inputs.lock().await;
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].metadata["kind"], "preference");
        assert_eq!(captured[0].metadata["status"], "active");
        assert_eq!(captured[0].metadata["supersedes"], json!(["old-pref"]));
        assert_eq!(captured[1].id.as_deref(), Some("old-pref"));
        assert_eq!(captured[1].metadata["status"], "superseded");
        assert!(output.content_for_model.contains("\"supersedes\": ["));
    }

    #[tokio::test]
    async fn add_rejects_non_active_status_override() {
        let mock = Arc::new(MockMemoryService::default());
        let tool = tool_with_mock(mock, create_store().await);
        let err = tool
            .execute(
                json!({
                    "action": "add",
                    "content": "Outdated preference",
                    "metadata": {
                        "status": "superseded"
                    }
                }),
                &test_ctx(),
            )
            .await
            .expect_err("system-managed status should be rejected");
        assert!(format!("{err}").contains("system-managed"));
    }

    #[tokio::test]
    async fn search_reads_session_history_only_and_honors_window_and_limit() {
        let mock = Arc::new(MockMemoryService::default());
        let store = create_store().await;
        store
            .append_chat_record(
                "session-123",
                &ChatRecord {
                    ts_ms: (OffsetDateTime::now_utc() - Duration::days(5))
                        .unix_timestamp_nanos()
                        .saturating_div(1_000_000) as i64,
                    role: "assistant".to_string(),
                    content: "old answer about deploy".to_string(),
                    metadata_json: None,
                    message_id: None,
                },
            )
            .await
            .expect("old chat record should persist");
        store
            .append_chat_record(
                "session-123",
                &ChatRecord {
                    ts_ms: (OffsetDateTime::now_utc() - Duration::hours(12))
                        .unix_timestamp_nanos()
                        .saturating_div(1_000_000) as i64,
                    role: "assistant".to_string(),
                    content: "recent answer about deploy rollback".to_string(),
                    metadata_json: None,
                    message_id: None,
                },
            )
            .await
            .expect("recent chat record should persist");
        store
            .append_chat_record(
                "session-123",
                &ChatRecord {
                    ts_ms: (OffsetDateTime::now_utc() - Duration::hours(6))
                        .unix_timestamp_nanos()
                        .saturating_div(1_000_000) as i64,
                    role: "user".to_string(),
                    content: "deploy rollback question".to_string(),
                    metadata_json: None,
                    message_id: None,
                },
            )
            .await
            .expect("user chat record should persist");

        let tool = MemoryTool::from_parts(
            mock.clone(),
            store,
            MemoryToolRuntimeConfig { search_limit: 1 },
        );
        let output = tool
            .execute(
                json!({
                    "action": "search",
                    "scope": "session",
                    "query": "deploy rollback",
                    "within_days": 1
                }),
                &test_ctx(),
            )
            .await
            .expect("session search should succeed");

        assert!(output.content_for_model.contains("deploy rollback"));
        assert!(!output.content_for_model.contains("old answer about deploy"));
        assert!(
            !output
                .content_for_model
                .contains("\"scope\": \"long_term\"")
        );
    }

    #[tokio::test]
    async fn rejects_unsupported_action() {
        let mock = Arc::new(MockMemoryService::default());
        let tool = tool_with_mock(mock, create_store().await);
        let err = tool
            .execute(json!({"action": "delete", "id": "m1"}), &test_ctx())
            .await
            .expect_err("delete should be rejected");
        assert!(format!("{err}").contains("add/search"));
    }

    #[tokio::test]
    async fn add_rejects_legacy_scope_parameter() {
        let mock = Arc::new(MockMemoryService::default());
        let tool = tool_with_mock(mock, create_store().await);
        let err = tool
            .execute(
                json!({
                    "action": "add",
                    "content": "important fact",
                    "scope": "long_term"
                }),
                &test_ctx(),
            )
            .await
            .expect_err("legacy add scope should be rejected");
        assert!(format!("{err}").contains("not supported"));
    }

    #[tokio::test]
    async fn search_rejects_non_session_scope() {
        let mock = Arc::new(MockMemoryService::default());
        let tool = tool_with_mock(mock, create_store().await);
        let err = tool
            .execute(
                json!({
                    "action": "search",
                    "query": "test",
                    "scope": "long_term"
                }),
                &test_ctx(),
            )
            .await
            .expect_err("long_term search should be rejected");
        assert!(format!("{err}").contains("scope"));
    }
}
