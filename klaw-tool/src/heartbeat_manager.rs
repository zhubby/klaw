use async_trait::async_trait;
use klaw_storage::{
    DefaultSessionStore, HeartbeatJob, HeartbeatStorage, HeartbeatTaskRun, NewHeartbeatJob,
    UpdateHeartbeatJobPatch, open_default_store,
};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const DEFAULT_LIST_LIMIT: i64 = 20;
const MAX_LIST_LIMIT: i64 = 200;

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
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_string()))
    }

    fn require_str(args: &Value, key: &str) -> Result<String, ToolError> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .ok_or_else(|| ToolError::InvalidArgs(format!("missing `{key}`")))
    }

    fn optional_str(args: &Value, key: &str) -> Result<Option<String>, ToolError> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(v)) => {
                let trimmed = v.trim();
                if trimmed.is_empty() {
                    return Err(ToolError::InvalidArgs(format!("`{key}` cannot be empty")));
                }
                Ok(Some(trimmed.to_string()))
            }
            _ => Err(ToolError::InvalidArgs(format!("`{key}` must be a string"))),
        }
    }

    fn optional_i64(args: &Value, key: &str) -> Result<Option<i64>, ToolError> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_i64()
                .map(Some)
                .ok_or_else(|| ToolError::InvalidArgs(format!("`{key}` must be an integer"))),
        }
    }

    fn parse_bool_like(raw: &str) -> Result<bool, ToolError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" | "on" => Ok(true),
            "false" | "0" | "no" | "n" | "off" => Ok(false),
            _ => Err(ToolError::InvalidArgs("expected boolean value".to_string())),
        }
    }

    fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>, ToolError> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(v)) => Ok(Some(*v)),
            Some(Value::String(v)) => Self::parse_bool_like(v).map(Some),
            Some(_) => Err(ToolError::InvalidArgs(format!("`{key}` must be a boolean"))),
        }
    }

    fn build_input(
        args: &Value,
        ctx: &ToolContext,
    ) -> Result<
        (
            Option<String>,
            String,
            String,
            String,
            bool,
            String,
            String,
            String,
            String,
        ),
        ToolError,
    > {
        let session_key =
            Self::optional_str(args, "session_key")?.unwrap_or_else(|| ctx.session_key.clone());
        let channel = Self::optional_str(args, "channel")?
            .or_else(|| {
                ctx.metadata
                    .get("channel")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .ok_or_else(|| ToolError::InvalidArgs("missing `channel`".to_string()))?;
        let chat_id = Self::optional_str(args, "chat_id")?
            .or_else(|| {
                ctx.metadata
                    .get("chat_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .ok_or_else(|| ToolError::InvalidArgs("missing `chat_id`".to_string()))?;

        let every = Self::require_str(args, "every")?;
        validate_every(&every)?;

        Ok((
            Self::optional_str(args, "id")?,
            session_key,
            channel,
            chat_id,
            Self::optional_bool(args, "enabled")?.unwrap_or(true),
            every,
            Self::require_str(args, "prompt")?,
            Self::optional_str(args, "silent_ack_token")?
                .unwrap_or_else(|| "HEARTBEAT_OK".to_string()),
            Self::optional_str(args, "timezone")?.unwrap_or_else(|| "UTC".to_string()),
        ))
    }

    async fn do_create(&self, args: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let (id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token, timezone) =
            Self::build_input(args, ctx)?;
        let heartbeat_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let job = self
            .storage
            .create_heartbeat(&NewHeartbeatJob {
                id: heartbeat_id,
                session_key,
                channel,
                chat_id,
                enabled,
                every: every.clone(),
                prompt,
                silent_ack_token,
                timezone,
                next_run_at_ms: compute_next_run_at_ms(&every)?,
            })
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "create",
            "heartbeat": heartbeat_job_to_json(job)
        }))
    }

    async fn do_update(&self, args: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let heartbeat_id = Self::require_str(args, "id")?;
        let (_, session_key, channel, chat_id, _enabled, every, prompt, silent_ack_token, timezone) =
            Self::build_input(args, ctx)?;
        self.storage
            .update_heartbeat(
                &heartbeat_id,
                &UpdateHeartbeatJobPatch {
                    session_key: Some(session_key),
                    channel: Some(channel),
                    chat_id: Some(chat_id),
                    every: Some(every.clone()),
                    prompt: Some(prompt),
                    silent_ack_token: Some(silent_ack_token),
                    timezone: Some(timezone),
                    next_run_at_ms: Some(compute_next_run_at_ms(&every)?),
                },
            )
            .await
            .map_err(map_storage_err)?;
        if let Some(enabled) = Self::optional_bool(args, "enabled")? {
            self.storage
                .set_heartbeat_enabled(&heartbeat_id, enabled)
                .await
                .map_err(map_storage_err)?;
        }
        let job = self
            .storage
            .get_heartbeat(&heartbeat_id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "update",
            "heartbeat": heartbeat_job_to_json(job)
        }))
    }

    async fn do_delete(&self, args: &Value) -> Result<Value, ToolError> {
        let heartbeat_id = Self::require_str(args, "id")?;
        self.storage
            .delete_heartbeat(&heartbeat_id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "delete",
            "id": heartbeat_id,
            "deleted": true
        }))
    }

    async fn do_get(&self, args: &Value) -> Result<Value, ToolError> {
        let heartbeat_id = Self::require_str(args, "id")?;
        let job = self
            .storage
            .get_heartbeat(&heartbeat_id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "get",
            "heartbeat": heartbeat_job_to_json(job)
        }))
    }

    async fn do_set_enabled(&self, args: &Value, enabled: bool) -> Result<Value, ToolError> {
        let heartbeat_id = Self::require_str(args, "id")?;
        self.storage
            .set_heartbeat_enabled(&heartbeat_id, enabled)
            .await
            .map_err(map_storage_err)?;
        let job = self
            .storage
            .get_heartbeat(&heartbeat_id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "set_enabled",
            "heartbeat": heartbeat_job_to_json(job)
        }))
    }

    async fn do_list(&self, args: &Value) -> Result<Value, ToolError> {
        let limit = Self::optional_i64(args, "limit")?.unwrap_or(DEFAULT_LIST_LIMIT);
        let offset = Self::optional_i64(args, "offset")?.unwrap_or(0).max(0);
        let bounded_limit = limit.clamp(1, MAX_LIST_LIMIT);
        let items = self
            .storage
            .list_heartbeats(bounded_limit, offset)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "list",
            "limit": bounded_limit,
            "offset": offset,
            "items": items.into_iter().map(heartbeat_job_to_json).collect::<Vec<_>>()
        }))
    }

    async fn do_list_runs(&self, args: &Value) -> Result<Value, ToolError> {
        let heartbeat_id = Self::require_str(args, "id")?;
        let limit = Self::optional_i64(args, "limit")?.unwrap_or(DEFAULT_LIST_LIMIT);
        let offset = Self::optional_i64(args, "offset")?.unwrap_or(0).max(0);
        let bounded_limit = limit.clamp(1, MAX_LIST_LIMIT);
        let runs = self
            .storage
            .list_heartbeat_task_runs(&heartbeat_id, bounded_limit, offset)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "list_runs",
            "id": heartbeat_id,
            "limit": bounded_limit,
            "offset": offset,
            "items": runs.into_iter().map(heartbeat_task_run_to_json).collect::<Vec<_>>()
        }))
    }
}

#[async_trait]
impl Tool for HeartbeatManagerTool {
    fn name(&self) -> &str {
        "heartbeat_manager"
    }

    fn description(&self) -> &str {
        "Manage session-bound heartbeat jobs. Use this to create, update, list, enable, disable, or delete heartbeats that wake up an existing conversation context."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Manage persisted heartbeat jobs bound to existing sessions.",
            "oneOf": [
                {
                    "properties": {
                        "action": { "const": "create" },
                        "id": { "type": "string", "description": "Optional heartbeat id. Auto-generated when omitted." },
                        "session_key": { "type": "string", "description": "Target session key. Defaults to current tool session." },
                        "channel": { "type": "string", "description": "Target channel. Required if not inferable from tool context." },
                        "chat_id": { "type": "string", "description": "Target chat id. Required if not inferable from tool context." },
                        "enabled": { "type": "boolean", "description": "Whether the heartbeat starts enabled. Defaults to true." },
                        "every": { "type": "string", "description": "Repeat interval such as `10m`, `1h`, or `24h`." },
                        "prompt": { "type": "string", "description": "Prompt injected into the bound session on each heartbeat." },
                        "silent_ack_token": { "type": "string", "description": "Exact token used to suppress no-op heartbeat output. Defaults to HEARTBEAT_OK." },
                        "timezone": { "type": "string", "description": "Timezone label. Defaults to UTC." }
                    },
                    "required": ["action", "every", "prompt"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "update" },
                        "id": { "type": "string" },
                        "session_key": { "type": "string" },
                        "channel": { "type": "string" },
                        "chat_id": { "type": "string" },
                        "enabled": { "type": "boolean" },
                        "every": { "type": "string" },
                        "prompt": { "type": "string" },
                        "silent_ack_token": { "type": "string" },
                        "timezone": { "type": "string" }
                    },
                    "required": ["action", "id", "every", "prompt"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "get" },
                        "id": { "type": "string" }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "delete" },
                        "id": { "type": "string" }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "enum": ["set_enabled", "enable", "disable"] },
                        "id": { "type": "string" },
                        "enabled": { "type": "boolean" }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "list" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIST_LIMIT, "default": DEFAULT_LIST_LIMIT },
                        "offset": { "type": "integer", "minimum": 0, "default": 0 }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "properties": {
                        "action": { "const": "list_runs" },
                        "id": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIST_LIMIT, "default": DEFAULT_LIST_LIMIT },
                        "offset": { "type": "integer", "minimum": 0, "default": 0 }
                    },
                    "required": ["action", "id"],
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
            "create" => self.do_create(&args, ctx).await?,
            "update" => self.do_update(&args, ctx).await?,
            "delete" => self.do_delete(&args).await?,
            "get" => self.do_get(&args).await?,
            "set_enabled" => {
                let enabled = Self::optional_bool(&args, "enabled")?
                    .ok_or_else(|| ToolError::InvalidArgs("missing `enabled`".to_string()))?;
                self.do_set_enabled(&args, enabled).await?
            }
            "enable" => self.do_set_enabled(&args, true).await?,
            "disable" => self.do_set_enabled(&args, false).await?,
            "list" => self.do_list(&args).await?,
            "list_runs" => self.do_list_runs(&args).await?,
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of create/update/delete/get/list/list_runs/set_enabled/enable/disable"
                        .to_string(),
                ))
            }
        };
        let rendered = serde_json::to_string_pretty(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize heartbeat_manager output failed: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }
}

fn map_storage_err(err: impl ToString) -> ToolError {
    ToolError::ExecutionFailed(err.to_string())
}

fn compute_next_run_at_ms(value: &str) -> Result<i64, ToolError> {
    validate_every(value)?;
    let every = humantime::parse_duration(value)
        .map_err(|err| ToolError::InvalidArgs(format!("invalid `every`: {err}")))?;
    Ok(now_ms().saturating_add(every.as_millis() as i64))
}

fn validate_every(value: &str) -> Result<(), ToolError> {
    let every = humantime::parse_duration(value)
        .map_err(|err| ToolError::InvalidArgs(format!("invalid `every`: {err}")))?;
    if every.is_zero() {
        return Err(ToolError::InvalidArgs(
            "`every` must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

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
        "timezone": job.timezone,
        "next_run_at_ms": job.next_run_at_ms,
        "last_run_at_ms": job.last_run_at_ms,
        "created_at_ms": job.created_at_ms,
        "updated_at_ms": job.updated_at_ms
    })
}

fn heartbeat_task_run_to_json(run: HeartbeatTaskRun) -> Value {
    json!({
        "id": run.id,
        "heartbeat_id": run.heartbeat_id,
        "scheduled_at_ms": run.scheduled_at_ms,
        "started_at_ms": run.started_at_ms,
        "finished_at_ms": run.finished_at_ms,
        "status": run.status.as_str(),
        "attempt": run.attempt,
        "error_message": run.error_message,
        "published_message_id": run.published_message_id,
        "created_at_ms": run.created_at_ms
    })
}
