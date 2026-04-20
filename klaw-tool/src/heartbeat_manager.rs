use async_trait::async_trait;
use klaw_storage::{
    DefaultSessionStore, HeartbeatJob, HeartbeatStorage, SessionStorage, UpdateHeartbeatJobPatch,
    open_default_store,
};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

pub struct HeartbeatManagerTool {
    storage: Arc<DefaultSessionStore>,
}

impl HeartbeatManagerTool {
    pub async fn open_default() -> Result<Self, ToolError> {
        let store = open_default_store()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("open storage failed: {err}")))?;
        Ok(Self::with_store(store))
    }

    pub fn with_store(store: DefaultSessionStore) -> Self {
        Self {
            storage: Arc::new(store),
        }
    }

    fn require_action(args: &Value) -> Result<&str, ToolError> {
        args.get("action")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_string()))
    }

    fn require_string(args: &Value, key: &str) -> Result<String, ToolError> {
        match args.get(key) {
            Some(Value::String(value)) => Ok(value.trim().to_string()),
            Some(_) => Err(ToolError::InvalidArgs(format!("`{key}` must be a string"))),
            None => Err(ToolError::InvalidArgs(format!("missing `{key}`"))),
        }
    }

    async fn resolve_current_heartbeat_session_key(
        &self,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let resolved = self
            .storage
            .get_session_by_active_session_key(&ctx.session_key)
            .await
            .map(|session| session.session_key)
            .unwrap_or_else(|_| ctx.session_key.clone());
        Ok(resolved)
    }

    async fn load_current_job(&self, ctx: &ToolContext) -> Result<HeartbeatJob, ToolError> {
        let session_key = self.resolve_current_heartbeat_session_key(ctx).await?;
        self.storage
            .get_heartbeat_by_session_key(&session_key)
            .await
            .map_err(map_storage_err)
    }

    async fn do_get(&self, ctx: &ToolContext) -> Result<Value, ToolError> {
        let job = self.load_current_job(ctx).await?;
        Ok(json!({
            "action": "get",
            "heartbeat": heartbeat_job_to_json(job)
        }))
    }

    async fn do_update(&self, args: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let current = self.load_current_job(ctx).await?;
        let prompt = Self::require_string(args, "prompt")?;

        self.storage
            .update_heartbeat(
                &current.id,
                &UpdateHeartbeatJobPatch {
                    prompt: Some(prompt),
                    ..UpdateHeartbeatJobPatch::default()
                },
            )
            .await
            .map_err(map_storage_err)?;

        let job = self
            .storage
            .get_heartbeat(&current.id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "update",
            "heartbeat": heartbeat_job_to_json(job)
        }))
    }
}

#[async_trait]
impl Tool for HeartbeatManagerTool {
    fn name(&self) -> &str {
        "heartbeat_manager"
    }

    fn description(&self) -> &str {
        "Get or update the heartbeat bound to the current conversation. Use `get` to inspect the current session heartbeat, or `update` to change only the custom prompt that is prepended before the fixed heartbeat instruction."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Read or update the heartbeat bound to the current conversation session.",
            "oneOf": [
                {
                    "properties": {
                        "action": { "const": "get" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "update" },
                        "prompt": { "type": "string", "description": "Custom prompt prepended before the fixed heartbeat instruction. Use an empty string to clear it." }
                    },
                    "required": ["action", "prompt"],
                    "additionalProperties": false
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Destructive
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let payload = match action {
            "get" => self.do_get(ctx).await?,
            "update" => self.do_update(&args, ctx).await?,
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of get/update".to_string(),
                ));
            }
        };
        let rendered = serde_json::to_string_pretty(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize heartbeat_manager output failed: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
            media: Vec::new(),
            signals: Vec::new(),
        })
    }
}

fn map_storage_err(err: impl ToString) -> ToolError {
    ToolError::ExecutionFailed(err.to_string())
}

#[cfg(test)]
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or_default()
}

fn heartbeat_job_to_json(job: HeartbeatJob) -> Value {
    json!({
        "id": job.id,
        "session_key": job.session_key,
        "channel": job.channel,
        "chat_id": job.chat_id,
        "enabled": job.enabled,
        "every": job.every,
        "prompt": job.prompt,
        "silent_ack_token": job.silent_ack_token,
        "recent_messages_limit": job.recent_messages_limit,
        "timezone": job.timezone,
        "next_run_at_ms": job.next_run_at_ms,
        "last_run_at_ms": job.last_run_at_ms,
        "created_at_ms": job.created_at_ms,
        "updated_at_ms": job.updated_at_ms
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_storage::{NewHeartbeatJob, StoragePaths};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn test_tool() -> HeartbeatManagerTool {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("klaw-heartbeat-tool-test-{}-{suffix}", now_ms()));
        let store = DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("store should open");
        HeartbeatManagerTool::with_store(store)
    }

    fn ctx() -> ToolContext {
        ToolContext {
            session_key: "telegram:test".to_string(),
            metadata: BTreeMap::from([
                ("channel".to_string(), json!("telegram")),
                ("chat_id".to_string(), json!("chat-1")),
            ]),
        }
    }

    async fn seed_heartbeat(
        tool: &HeartbeatManagerTool,
        session_key: &str,
        active_session_key: Option<&str>,
    ) {
        tool.storage
            .get_or_create_session_state(session_key, "chat-1", "telegram", "openai", "gpt-4o-mini")
            .await
            .expect("base session");
        if let Some(active_session_key) = active_session_key {
            tool.storage
                .get_or_create_session_state(
                    active_session_key,
                    "chat-1",
                    "telegram",
                    "openai",
                    "gpt-4o-mini",
                )
                .await
                .expect("active session");
            tool.storage
                .set_active_session(session_key, "chat-1", "telegram", active_session_key)
                .await
                .expect("set active session");
        }
        tool.storage
            .create_heartbeat(&NewHeartbeatJob {
                id: format!("hb-{session_key}"),
                session_key: session_key.to_string(),
                channel: "telegram".to_string(),
                chat_id: "chat-1".to_string(),
                enabled: true,
                every: "30m".to_string(),
                prompt: "check unresolved items".to_string(),
                silent_ack_token: "HEARTBEAT_OK".to_string(),
                recent_messages_limit: 12,
                timezone: "UTC".to_string(),
                next_run_at_ms: now_ms(),
            })
            .await
            .expect("seed heartbeat");
    }

    #[tokio::test]
    async fn get_returns_current_session_heartbeat() {
        let tool = test_tool().await;
        seed_heartbeat(&tool, "telegram:test", None).await;

        let output = tool
            .execute(json!({ "action": "get" }), &ctx())
            .await
            .expect("get should succeed");

        assert!(
            output
                .content_for_model
                .contains("\"session_key\": \"telegram:test\"")
        );
        assert!(
            output
                .content_for_model
                .contains("\"prompt\": \"check unresolved items\"")
        );
    }

    #[tokio::test]
    async fn update_resolves_base_session_from_active_child() {
        let tool = test_tool().await;
        seed_heartbeat(&tool, "telegram:test", Some("telegram:test:child")).await;

        let output = tool
            .execute(
                json!({
                    "action": "update",
                    "prompt": "review unread mentions"
                }),
                &ToolContext {
                    session_key: "telegram:test:child".to_string(),
                    metadata: BTreeMap::from([
                        ("channel".to_string(), json!("telegram")),
                        ("chat_id".to_string(), json!("chat-1")),
                    ]),
                },
            )
            .await
            .expect("update should succeed");

        assert!(
            output
                .content_for_model
                .contains("\"prompt\": \"review unread mentions\"")
        );

        let stored = tool
            .storage
            .get_heartbeat_by_session_key("telegram:test")
            .await
            .expect("base heartbeat");
        assert_eq!(stored.prompt, "review unread mentions");
    }

    #[tokio::test]
    async fn update_allows_clearing_custom_prompt() {
        let tool = test_tool().await;
        seed_heartbeat(&tool, "telegram:test", None).await;

        let output = tool
            .execute(json!({ "action": "update", "prompt": "" }), &ctx())
            .await
            .expect("clearing prompt should succeed");
        assert!(output.content_for_model.contains("\"prompt\": \"\""));
    }
}
