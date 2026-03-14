use async_trait::async_trait;
use klaw_config::{AppConfig, MemoryToolConfig};
use klaw_memory::{
    MemoryError, MemoryHit, MemoryRecord, MemorySearchQuery, MemoryService, SqliteMemoryService,
    UpsertMemoryInput,
};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const MAX_SEARCH_LIMIT: usize = 50;

#[derive(Debug, Clone)]
struct MemoryToolRuntimeConfig {
    search_limit: usize,
    fts_limit: usize,
    vector_limit: usize,
    use_vector: bool,
}

impl MemoryToolRuntimeConfig {
    fn from_config(config: &AppConfig) -> Self {
        Self::from_tool_config(&config.tools.memory)
    }

    fn from_tool_config(config: &MemoryToolConfig) -> Self {
        Self {
            search_limit: config.search_limit.min(MAX_SEARCH_LIMIT),
            fts_limit: config.fts_limit.min(MAX_SEARCH_LIMIT),
            vector_limit: config.vector_limit.min(MAX_SEARCH_LIMIT),
            use_vector: config.use_vector,
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
    runtime: MemoryToolRuntimeConfig,
}

impl MemoryTool {
    pub async fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let service = SqliteMemoryService::open_default(config)
            .await
            .map_err(map_memory_err)?;
        Ok(Self {
            service: Arc::new(service),
            runtime: MemoryToolRuntimeConfig::from_config(config),
        })
    }

    pub fn from_service(service: Arc<dyn MemoryService>) -> Self {
        Self {
            service,
            runtime: MemoryToolRuntimeConfig::default(),
        }
    }

    #[cfg(test)]
    fn from_service_with_runtime(
        service: Arc<dyn MemoryService>,
        runtime: MemoryToolRuntimeConfig,
    ) -> Self {
        Self { service, runtime }
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
            Some(Value::Object(_)) => Ok(args["metadata"].clone()),
            Some(_) => Err(ToolError::InvalidArgs(
                "`metadata` must be a JSON object".to_string(),
            )),
            None => Ok(json!({})),
        }
    }

    fn parse_bool(args: &Value, key: &str, default_value: bool) -> Result<bool, ToolError> {
        match args.get(key) {
            Some(v) => v
                .as_bool()
                .ok_or_else(|| ToolError::InvalidArgs(format!("`{key}` must be a boolean"))),
            None => Ok(default_value),
        }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Add and search long-term memory for this agent. Use `add` to persist important facts and `search` to recall relevant prior context in the current session scope."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Memory operation request. Scope and retrieval strategy are auto-managed by runtime config to reduce argument complexity.",
            "oneOf": [
                {
                    "description": "Add one memory record to the current session scope.",
                    "properties": {
                        "action": { "const": "add" },
                        "content": {
                            "type": "string",
                            "description": "Memory text content."
                        },
                        "metadata": {
                            "type": "object",
                            "description": "Optional structured metadata for filtering/traceability.",
                            "additionalProperties": true
                        },
                        "pinned": {
                            "type": "boolean",
                            "description": "Pinned flag for add.",
                            "default": false
                        }
                    },
                    "required": ["action", "content"],
                    "additionalProperties": false
                },
                {
                    "description": "Search memory records in the current session scope.",
                    "properties": {
                        "action": { "const": "search" },
                        "query": {
                            "type": "string",
                            "description": "Search query text."
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
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| ToolError::InvalidArgs("missing `content`".to_string()))?
                    .to_string();
                let metadata = Self::parse_metadata(&args)?;
                let pinned = Self::parse_bool(&args, "pinned", false)?;

                let record = self
                    .service
                    .upsert(UpsertMemoryInput {
                        id: None,
                        scope: ctx.session_key.clone(),
                        content,
                        metadata,
                        pinned,
                    })
                    .await
                    .map_err(map_memory_err)?;
                json!({
                    "action": "add",
                    "record": record_to_json(record)
                })
            }
            "search" => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| ToolError::InvalidArgs("missing `query`".to_string()))?
                    .to_string();

                let hits = self
                    .service
                    .search(MemorySearchQuery {
                        scope: Some(ctx.session_key.clone()),
                        text: query,
                        limit: self.runtime.search_limit,
                        fts_limit: self.runtime.fts_limit,
                        vector_limit: self.runtime.vector_limit,
                        use_vector: self.runtime.use_vector,
                    })
                    .await
                    .map_err(map_memory_err)?;

                json!({
                    "action": "search",
                    "hits": hits.into_iter().map(hit_to_json).collect::<Vec<_>>()
                })
            }
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of add/search".to_string(),
                ))
            }
        };

        let rendered = serde_json::to_string_pretty(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize memory output failed: {err}"))
        })?;

        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
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

fn hit_to_json(hit: MemoryHit) -> Value {
    json!({
        "fused_score": hit.fused_score,
        "bm25_rank": hit.bm25_rank,
        "vector_rank": hit.vector_rank,
        "record": record_to_json(hit.record)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::{BTreeMap, VecDeque};
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockMemoryService {
        upsert_inputs: Mutex<Vec<UpsertMemoryInput>>,
        search_inputs: Mutex<Vec<MemorySearchQuery>>,
        search_hits: Mutex<VecDeque<Vec<MemoryHit>>>,
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

        async fn search(&self, query: MemorySearchQuery) -> Result<Vec<MemoryHit>, MemoryError> {
            self.search_inputs.lock().await.push(query);
            Ok(self
                .search_hits
                .lock()
                .await
                .pop_front()
                .unwrap_or_default())
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

    fn tool_with_mock(mock: Arc<MockMemoryService>) -> MemoryTool {
        MemoryTool::from_service(mock)
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_key: "session-123".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn add_defaults_scope_to_session_key_and_generates_id() {
        let mock = Arc::new(MockMemoryService::default());
        let tool = tool_with_mock(mock.clone());

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
        assert_eq!(captured[0].scope, "session-123");
        assert!(captured[0].id.is_none());
        assert!(output.content_for_model.contains("remember this"));
    }

    #[tokio::test]
    async fn search_uses_session_scope_and_runtime_defaults() {
        let mock = Arc::new(MockMemoryService::default());
        {
            let mut hits = mock.search_hits.lock().await;
            hits.push_back(vec![MemoryHit {
                record: MemoryRecord {
                    id: "m1".to_string(),
                    scope: "session-123".to_string(),
                    content: "session fact".to_string(),
                    metadata: json!({"kind": "fact"}),
                    pinned: true,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                },
                fused_score: 0.9,
                bm25_rank: Some(1),
                vector_rank: Some(1),
            }]);
        }

        let runtime = MemoryToolRuntimeConfig {
            search_limit: 5,
            fts_limit: 7,
            vector_limit: 9,
            use_vector: false,
        };
        let tool = MemoryTool::from_service_with_runtime(mock.clone(), runtime);
        let output = tool
            .execute(
                json!({
                    "action": "search",
                    "query": "fact"
                }),
                &test_ctx(),
            )
            .await
            .expect("search should succeed");

        let captured = mock.search_inputs.lock().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].scope.as_deref(), Some("session-123"));
        assert_eq!(captured[0].limit, 5);
        assert_eq!(captured[0].fts_limit, 7);
        assert_eq!(captured[0].vector_limit, 9);
        assert!(!captured[0].use_vector);
        assert!(output.content_for_model.contains("session fact"));
    }

    #[tokio::test]
    async fn rejects_unsupported_action() {
        let mock = Arc::new(MockMemoryService::default());
        let tool = tool_with_mock(mock);
        let err = tool
            .execute(json!({"action": "delete", "id": "m1"}), &test_ctx())
            .await
            .expect_err("delete should be rejected");
        assert!(format!("{err}").contains("add/search"));
    }
}
