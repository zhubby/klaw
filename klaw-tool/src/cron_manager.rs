use async_trait::async_trait;
use chrono::Utc;
use klaw_storage::{
    open_default_store, CronJob, CronScheduleKind, CronStorage, CronTaskRun, NewCronJob,
    StorageError, UpdateCronJobPatch,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const DEFAULT_LIST_LIMIT: i64 = 20;
const MAX_LIST_LIMIT: i64 = 200;

pub struct CronManagerTool {
    storage: Arc<dyn CronStorage>,
}

impl CronManagerTool {
    pub async fn open_default() -> Result<Self, ToolError> {
        let store = open_default_store()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("open storage failed: {err}")))?;
        Ok(Self {
            storage: Arc::new(store),
        })
    }

    #[cfg(test)]
    fn from_storage(storage: Arc<dyn CronStorage>) -> Self {
        Self { storage }
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

    fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>, ToolError> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(v)) => Ok(Some(*v)),
            Some(Value::String(v)) => parse_bool_like(v).map(Some).map_err(|_| {
                ToolError::InvalidArgs(format!(
                    "`{key}` must be a boolean; also accepts string values like \"true\"/\"false\""
                ))
            }),
            Some(_) => Err(ToolError::InvalidArgs(format!(
                "`{key}` must be a boolean; also accepts string values like \"true\"/\"false\""
            ))),
        }
    }

    fn parse_schedule_kind(raw: &str) -> Result<CronScheduleKind, ToolError> {
        CronScheduleKind::parse(raw).ok_or_else(|| {
            ToolError::InvalidArgs("`schedule_kind` must be one of cron/every".to_string())
        })
    }

    fn resolve_create_schedule(args: &Value) -> Result<(CronScheduleKind, String), ToolError> {
        let raw_expr = Self::require_str(args, "schedule_expr")?;
        let explicit_kind = Self::optional_str(args, "schedule_kind")?
            .map(|value| Self::parse_schedule_kind(&value))
            .transpose()?;

        normalize_schedule_input(explicit_kind, &raw_expr)
    }

    fn resolve_update_schedule(
        args: &Value,
        current: &CronJob,
    ) -> Result<(Option<CronScheduleKind>, Option<String>), ToolError> {
        let schedule_kind = Self::optional_str(args, "schedule_kind")?
            .map(|value| Self::parse_schedule_kind(&value))
            .transpose()?;

        let schedule_expr = match Self::optional_str(args, "schedule_expr")? {
            Some(raw_expr) => {
                let effective_kind = schedule_kind.unwrap_or(current.schedule_kind);
                let (_, normalized_expr) =
                    normalize_schedule_input(Some(effective_kind), &raw_expr)?;
                Some(normalized_expr)
            }
            None => None,
        };

        Ok((schedule_kind, schedule_expr))
    }

    fn parse_payload_json(
        args: &Value,
        ctx: &ToolContext,
        default_session_key: Option<&str>,
    ) -> Result<String, ToolError> {
        if let Some(raw) = args.get("payload_json") {
            let payload = raw
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ToolError::InvalidArgs("`payload_json` must be a string".to_string())
                })?;
            return parse_payload_object(payload, "`payload_json`");
        }

        if let Some(payload) = args.get("payload").cloned() {
            if let Some(raw) = payload.as_str() {
                return parse_payload_object(raw.trim(), "`payload`");
            }
            if !payload.is_object() {
                return Err(ToolError::InvalidArgs(
                    "`payload` must be a JSON object, or a JSON string that decodes to an object"
                        .to_string(),
                ));
            }
            validate_inbound_payload_value(&payload).map_err(|err| {
                ToolError::InvalidArgs(format!(
                    "`payload` must decode to a valid InboundMessage-like object: {err}"
                ))
            })?;
            return serde_json::to_string(&payload).map_err(|err| {
                ToolError::ExecutionFailed(format!("serialize `payload` to json failed: {err}"))
            });
        }

        build_payload_from_shortcut(args, ctx, default_session_key)
    }

    fn compute_next_run_ms(
        kind: CronScheduleKind,
        expr: &str,
        from_ms: i64,
    ) -> Result<i64, ToolError> {
        match kind {
            CronScheduleKind::Every => {
                let interval = humantime::parse_duration(expr)
                    .map_err(|err| ToolError::InvalidArgs(format!("invalid schedule: {err}")))?;
                if interval.is_zero() {
                    return Err(ToolError::InvalidArgs(
                        "invalid schedule: every duration must be greater than zero".to_string(),
                    ));
                }
                Ok(from_ms.saturating_add(interval.as_millis() as i64))
            }
            CronScheduleKind::Cron => {
                let schedule = cron::Schedule::from_str(expr)
                    .map_err(|err| ToolError::InvalidArgs(format!("invalid schedule: {err}")))?;
                let after =
                    chrono::DateTime::<Utc>::from_timestamp_millis(from_ms.saturating_add(1))
                        .unwrap_or_else(Utc::now);
                let next = schedule.after(&after).next().ok_or_else(|| {
                    ToolError::InvalidArgs(
                        "invalid schedule: cron expression has no next run".to_string(),
                    )
                })?;
                Ok(next.timestamp_millis())
            }
        }
    }

    async fn do_create(&self, args: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let id = Self::optional_str(args, "id")?.unwrap_or_else(|| Uuid::new_v4().to_string());
        let name = Self::require_str(args, "name")?;
        let (schedule_kind, schedule_expr) = Self::resolve_create_schedule(args)?;
        let default_session_key = format!("cron:{id}");
        let payload_json = Self::parse_payload_json(args, ctx, Some(&default_session_key))?;
        let enabled = Self::optional_bool(args, "enabled")?.unwrap_or(true);
        let timezone = Self::optional_str(args, "timezone")?.unwrap_or_else(|| "UTC".to_string());
        let next_run_at_ms = match Self::optional_i64(args, "next_run_at_ms")? {
            Some(v) => v,
            None => Self::compute_next_run_ms(schedule_kind, &schedule_expr, now_ms())?,
        };

        let job = self
            .storage
            .create_cron(&NewCronJob {
                id,
                name,
                schedule_kind,
                schedule_expr,
                payload_json,
                enabled,
                timezone,
                next_run_at_ms,
            })
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "create",
            "cron": cron_job_to_json(job)
        }))
    }

    async fn do_update(&self, args: &Value, ctx: &ToolContext) -> Result<Value, ToolError> {
        let cron_id = Self::require_str(args, "id")?;
        let current = self
            .storage
            .get_cron(&cron_id)
            .await
            .map_err(map_storage_err)?;

        let (schedule_kind, schedule_expr) = Self::resolve_update_schedule(args, &current)?;

        let payload_json = if args.get("payload").is_some()
            || args.get("payload_json").is_some()
            || args.get("message").is_some()
        {
            Some(Self::parse_payload_json(
                args,
                ctx,
                Some(&format!("cron:{cron_id}")),
            )?)
        } else {
            None
        };

        let mut patch = UpdateCronJobPatch {
            name: Self::optional_str(args, "name")?,
            schedule_kind,
            schedule_expr,
            payload_json,
            timezone: Self::optional_str(args, "timezone")?,
            next_run_at_ms: Self::optional_i64(args, "next_run_at_ms")?,
        };

        if patch.next_run_at_ms.is_none()
            && (patch.schedule_kind.is_some() || patch.schedule_expr.is_some())
        {
            let effective_kind = patch.schedule_kind.unwrap_or(current.schedule_kind);
            let effective_expr = patch
                .schedule_expr
                .as_deref()
                .unwrap_or(&current.schedule_expr);
            patch.next_run_at_ms = Some(Self::compute_next_run_ms(
                effective_kind,
                effective_expr,
                now_ms(),
            )?);
        }

        let job = self
            .storage
            .update_cron(&cron_id, &patch)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "update",
            "cron": cron_job_to_json(job)
        }))
    }

    async fn do_delete(&self, args: &Value) -> Result<Value, ToolError> {
        let cron_id = Self::require_str(args, "id")?;
        self.storage
            .delete_cron(&cron_id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "delete",
            "id": cron_id,
            "deleted": true
        }))
    }

    async fn do_get(&self, args: &Value) -> Result<Value, ToolError> {
        let cron_id = Self::require_str(args, "id")?;
        let job = self
            .storage
            .get_cron(&cron_id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "get",
            "cron": cron_job_to_json(job)
        }))
    }

    async fn do_set_enabled(&self, args: &Value) -> Result<Value, ToolError> {
        let cron_id = Self::require_str(args, "id")?;
        let enabled = Self::optional_bool(args, "enabled")?
            .ok_or_else(|| ToolError::InvalidArgs("missing `enabled`".to_string()))?;
        self.storage
            .set_enabled(&cron_id, enabled)
            .await
            .map_err(map_storage_err)?;
        let job = self
            .storage
            .get_cron(&cron_id)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "set_enabled",
            "cron": cron_job_to_json(job)
        }))
    }

    async fn do_list(&self, args: &Value) -> Result<Value, ToolError> {
        let limit = Self::optional_i64(args, "limit")?.unwrap_or(DEFAULT_LIST_LIMIT);
        let offset = Self::optional_i64(args, "offset")?.unwrap_or(0).max(0);
        let bounded_limit = limit.clamp(1, MAX_LIST_LIMIT);
        let items = self
            .storage
            .list_crons(bounded_limit, offset)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "list",
            "limit": bounded_limit,
            "offset": offset,
            "items": items.into_iter().map(cron_job_to_json).collect::<Vec<_>>()
        }))
    }

    async fn do_list_due(&self, args: &Value) -> Result<Value, ToolError> {
        let now = Self::optional_i64(args, "now_ms")?.unwrap_or_else(now_ms);
        let limit = Self::optional_i64(args, "limit")?.unwrap_or(DEFAULT_LIST_LIMIT);
        let bounded_limit = limit.clamp(1, MAX_LIST_LIMIT);
        let items = self
            .storage
            .list_due_crons(now, bounded_limit)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "list_due",
            "now_ms": now,
            "limit": bounded_limit,
            "items": items.into_iter().map(cron_job_to_json).collect::<Vec<_>>()
        }))
    }

    async fn do_list_runs(&self, args: &Value) -> Result<Value, ToolError> {
        let cron_id = Self::require_str(args, "id")?;
        let limit = Self::optional_i64(args, "limit")?.unwrap_or(DEFAULT_LIST_LIMIT);
        let offset = Self::optional_i64(args, "offset")?.unwrap_or(0).max(0);
        let bounded_limit = limit.clamp(1, MAX_LIST_LIMIT);
        let runs = self
            .storage
            .list_task_runs(&cron_id, bounded_limit, offset)
            .await
            .map_err(map_storage_err)?;
        Ok(json!({
            "action": "list_runs",
            "id": cron_id,
            "limit": bounded_limit,
            "offset": offset,
            "items": runs.into_iter().map(cron_task_run_to_json).collect::<Vec<_>>()
        }))
    }
}

impl Default for CronManagerTool {
    fn default() -> Self {
        panic!("CronManagerTool::default is not supported; use open_default()")
    }
}

#[async_trait]
impl Tool for CronManagerTool {
    fn name(&self) -> &str {
        "cron_manager"
    }

    fn description(&self) -> &str {
        "Manage scheduled cron jobs. Supports create/update/delete/get/list/list_due/list_runs/set_enabled for persisted cron tasks and run records."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Manage cron job definitions and run records in storage.",
            "oneOf": [
                {
                    "description": "Create a cron job definition.",
                    "properties": {
                    "action": { "const": "create" },
                        "id": { "type": "string", "description": "Optional cron job id. Auto-generated when omitted." },
                        "name": { "type": "string", "description": "Cron display name." },
                        "schedule_kind": { "type": "string", "enum": ["cron", "every"], "description": "Optional when it can be inferred from `schedule_expr`. Use `every` for intervals like `24h`; use `cron` for calendar schedules." },
                        "schedule_expr": { "type": "string", "description": "Schedule expression. Accepted examples: `24h`, `every 24h`, `0 8 * * *`, `0 0 8 * * *`, or daily shorthand `8:00`." },
                        "message": { "type": "string", "description": "Preferred shortcut. If set and `payload`/`payload_json` are omitted, the tool builds an inbound payload automatically using the current conversation context. Use this for 'run this prompt on a schedule in the current chat'." },
                        "payload": { "description": "InboundMessage payload to publish when triggered. Prefer a JSON object. A JSON string that decodes to an object is also accepted.", "oneOf": [{ "type": "object" }, { "type": "string" }] },
                        "payload_json": { "type": "string", "description": "Payload JSON string alternative to `payload`. Must decode to a JSON object." },
                        "channel": { "type": "string", "description": "Optional override used only with `message` shortcut. Defaults to the current session channel, or `cron` if it cannot be inferred." },
                        "sender_id": { "type": "string", "description": "Optional override used only with `message` shortcut. Defaults to `system`." },
                        "chat_id": { "type": "string", "description": "Optional override used only with `message` shortcut. Defaults to the current session chat id when it can be inferred." },
                        "session_key": { "type": "string", "description": "Optional override used only with `message` shortcut. Defaults to an isolated cron session such as `cron:<job_id>`." },
                        "metadata": { "type": "object", "description": "Optional metadata object used only with `message` shortcut. Defaults to `{}`." },
                        "enabled": { "description": "Whether the cron starts enabled. Defaults to true. Prefer a boolean; string values like `\"true\"` and `\"false\"` are also accepted.", "oneOf": [{ "type": "boolean" }, { "type": "string" }] },
                        "timezone": { "type": "string", "description": "Timezone label. Defaults to UTC." },
                        "next_run_at_ms": { "type": "integer", "description": "Optional explicit next run timestamp in ms." }
                    },
                    "required": ["action", "name", "schedule_expr"],
                    "anyOf": [
                        { "required": ["message"] },
                        { "required": ["payload"] },
                        { "required": ["payload_json"] }
                    ],
                    "additionalProperties": false
                },
                {
                    "description": "Update an existing cron job definition.",
                    "properties": {
                        "action": { "const": "update" },
                        "id": { "type": "string", "description": "Cron job id." },
                        "name": { "type": "string", "description": "Updated cron display name." },
                        "schedule_kind": { "type": "string", "enum": ["cron", "every"], "description": "Optional schedule kind override." },
                        "schedule_expr": { "type": "string", "description": "Updated schedule expression. Accepted examples: `24h`, `every 24h`, `0 8 * * *`, `0 0 8 * * *`, or `8:00` for daily 08:00." },
                        "message": { "type": "string", "description": "Shortcut to rebuild the payload for the current conversation context." },
                        "payload": { "description": "Updated payload object or JSON string that decodes to an object.", "oneOf": [{ "type": "object" }, { "type": "string" }] },
                        "payload_json": { "type": "string", "description": "Updated payload JSON string." },
                        "channel": { "type": "string", "description": "Optional override used only with `message` shortcut." },
                        "sender_id": { "type": "string", "description": "Optional override used only with `message` shortcut." },
                        "chat_id": { "type": "string", "description": "Optional override used only with `message` shortcut." },
                        "session_key": { "type": "string", "description": "Optional override used only with `message` shortcut. Defaults to an isolated cron session such as `cron:<job_id>`." },
                        "metadata": { "type": "object", "description": "Optional metadata object used only with `message` shortcut." },
                        "timezone": { "type": "string", "description": "Updated timezone label." },
                        "next_run_at_ms": { "type": "integer", "description": "Optional explicit next run timestamp in ms." }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "description": "Delete a cron job definition.",
                    "properties": {
                        "action": { "const": "delete" },
                        "id": { "type": "string", "description": "Cron job id." }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "description": "Get one cron job definition by id.",
                    "properties": {
                        "action": { "const": "get" },
                        "id": { "type": "string", "description": "Cron job id." }
                    },
                    "required": ["action", "id"],
                    "additionalProperties": false
                },
                {
                    "description": "Set enabled status on one cron job.",
                    "properties": {
                        "action": { "const": "set_enabled" },
                        "id": { "type": "string", "description": "Cron job id." },
                        "enabled": { "type": "boolean", "description": "Target enabled state." }
                    },
                    "required": ["action", "id", "enabled"],
                    "additionalProperties": false
                },
                {
                    "description": "List cron job definitions.",
                    "properties": {
                        "action": { "const": "list" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIST_LIMIT, "default": DEFAULT_LIST_LIMIT },
                        "offset": { "type": "integer", "minimum": 0, "default": 0 }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "List due cron jobs at a reference timestamp.",
                    "properties": {
                        "action": { "const": "list_due" },
                        "now_ms": { "type": "integer", "description": "Reference timestamp in ms. Defaults to current time." },
                        "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIST_LIMIT, "default": DEFAULT_LIST_LIMIT }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "List run records for one cron job.",
                    "properties": {
                        "action": { "const": "list_runs" },
                        "id": { "type": "string", "description": "Cron job id." },
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

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let payload = match action {
            "create" => self.do_create(&args, _ctx).await?,
            "update" => self.do_update(&args, _ctx).await?,
            "delete" => self.do_delete(&args).await?,
            "get" => self.do_get(&args).await?,
            "set_enabled" => self.do_set_enabled(&args).await?,
            "list" => self.do_list(&args).await?,
            "list_due" => self.do_list_due(&args).await?,
            "list_runs" => self.do_list_runs(&args).await?,
            _ => return Err(ToolError::InvalidArgs(
                "`action` must be one of create/update/delete/get/list/list_due/list_runs/set_enabled"
                    .to_string(),
            )),
        };
        let rendered = serde_json::to_string_pretty(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize cron_manager output failed: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }
}

fn map_storage_err(err: StorageError) -> ToolError {
    ToolError::ExecutionFailed(err.to_string())
}

fn cron_job_to_json(job: CronJob) -> Value {
    json!({
        "id": job.id,
        "name": job.name,
        "schedule_kind": job.schedule_kind.as_str(),
        "schedule_expr": job.schedule_expr,
        "payload_json": job.payload_json,
        "enabled": job.enabled,
        "timezone": job.timezone,
        "next_run_at_ms": job.next_run_at_ms,
        "last_run_at_ms": job.last_run_at_ms,
        "created_at_ms": job.created_at_ms,
        "updated_at_ms": job.updated_at_ms
    })
}

fn cron_task_run_to_json(run: CronTaskRun) -> Value {
    json!({
        "id": run.id,
        "cron_id": run.cron_id,
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn parse_bool_like(raw: &str) -> Result<bool, ()> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "y" | "on" => Ok(true),
        "false" | "0" | "no" | "n" | "off" => Ok(false),
        _ => Err(()),
    }
}

fn parse_payload_object(raw: &str, field_name: &str) -> Result<String, ToolError> {
    let payload = serde_json::from_str::<Value>(raw)
        .map_err(|err| ToolError::InvalidArgs(format!("{field_name} is not valid json: {err}")))?;
    if !payload.is_object() {
        return Err(ToolError::InvalidArgs(format!(
            "{field_name} must decode to a JSON object"
        )));
    }
    validate_inbound_payload_value(&payload).map_err(|err| {
        ToolError::InvalidArgs(format!(
            "{field_name} must decode to a valid InboundMessage-like object: {err}"
        ))
    })?;
    serde_json::to_string(&payload).map_err(|err| {
        ToolError::ExecutionFailed(format!("serialize {field_name} to json failed: {err}"))
    })
}

fn build_payload_from_shortcut(
    args: &Value,
    ctx: &ToolContext,
    default_session_key: Option<&str>,
) -> Result<String, ToolError> {
    let message = args
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolError::InvalidArgs("missing `payload`, `payload_json`, or `message`".to_string())
        })?;

    let inferred = infer_channel_and_chat_id(&ctx.session_key);
    let channel = args
        .get("channel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| inferred.as_ref().map(|(channel, _)| channel.clone()))
        .unwrap_or_else(|| "cron".to_string());
    let chat_id = args
        .get("chat_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| inferred.as_ref().map(|(_, chat_id)| chat_id.clone()))
        .unwrap_or_else(|| ctx.session_key.clone());
    let session_key = args
        .get("session_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| default_session_key.map(ToString::to_string))
        .unwrap_or_else(|| "cron".to_string());
    let sender_id = args
        .get("sender_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("system");
    let mut metadata = inherited_channel_metadata(&ctx.metadata);
    if let Some(base_session_key) =
        infer_base_session_key_for_cron_shortcut(&ctx.session_key, &channel)
    {
        metadata.insert(
            "cron.base_session_key".to_string(),
            serde_json::Value::String(base_session_key),
        );
    }
    match args.get("metadata") {
        None | Some(Value::Null) => {}
        Some(Value::Object(map)) => metadata.extend(map.clone()),
        Some(_) => {
            return Err(ToolError::InvalidArgs(
                "`metadata` must be a JSON object when used with `message`".to_string(),
            ))
        }
    }

    let payload = json!({
        "channel": channel,
        "sender_id": sender_id,
        "chat_id": chat_id,
        "session_key": session_key,
        "content": message,
        "metadata": metadata
    });
    validate_inbound_payload_value(&payload).map_err(|err| {
        ToolError::InvalidArgs(format!(
            "`message` shortcut could not build a valid inbound payload: {err}"
        ))
    })?;
    serde_json::to_string(&payload).map_err(|err| {
        ToolError::ExecutionFailed(format!("serialize shortcut payload to json failed: {err}"))
    })
}

fn infer_channel_and_chat_id(session_key: &str) -> Option<(String, String)> {
    let (channel, rest) = session_key.split_once(':')?;
    let channel = channel.trim();
    let chat_id = rest
        .rsplit_once(':')
        .map(|(_, value)| value)
        .unwrap_or(rest)
        .trim();
    if channel.is_empty() || chat_id.is_empty() {
        return None;
    }
    Some((channel.to_string(), chat_id.to_string()))
}

fn infer_base_session_key_for_cron_shortcut(session_key: &str, channel: &str) -> Option<String> {
    match channel {
        "dingtalk" => {
            let mut parts = session_key.split(':');
            let prefix = parts.next()?.trim();
            if prefix != "dingtalk" {
                return None;
            }

            let account_id = parts.next()?.trim();
            let chat_id = parts.next()?.trim();
            if account_id.is_empty() || chat_id.is_empty() {
                return None;
            }

            Some(format!("dingtalk:{account_id}:{chat_id}"))
        }
        _ => None,
    }
}

fn inherited_channel_metadata(
    metadata: &BTreeMap<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in metadata {
        if matches!(
            key.as_str(),
            "channel.dingtalk.session_webhook" | "channel.dingtalk.bot_title"
        ) {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

fn normalize_schedule_input(
    explicit_kind: Option<CronScheduleKind>,
    raw_expr: &str,
) -> Result<(CronScheduleKind, String), ToolError> {
    let trimmed = raw_expr.trim();
    if trimmed.is_empty() {
        return Err(ToolError::InvalidArgs(
            "`schedule_expr` cannot be empty".to_string(),
        ));
    }

    let inferred_kind = explicit_kind.unwrap_or_else(|| infer_schedule_kind(trimmed));
    let normalized_expr = match inferred_kind {
        CronScheduleKind::Every => normalize_every_expr(trimmed)?,
        CronScheduleKind::Cron => normalize_cron_expr(trimmed)?,
    };

    Ok((inferred_kind, normalized_expr))
}

fn infer_schedule_kind(expr: &str) -> CronScheduleKind {
    if expr.trim_start().to_ascii_lowercase().starts_with("every ") {
        return CronScheduleKind::Every;
    }
    if humantime::parse_duration(expr.trim()).is_ok() {
        return CronScheduleKind::Every;
    }
    CronScheduleKind::Cron
}

fn normalize_every_expr(expr: &str) -> Result<String, ToolError> {
    let trimmed = expr.trim();
    let normalized = trimmed
        .strip_prefix("every ")
        .or_else(|| trimmed.strip_prefix("Every "))
        .unwrap_or(trimmed)
        .trim();

    let parsed = humantime::parse_duration(normalized).map_err(|_| {
        ToolError::InvalidArgs(
            "invalid schedule: `every` expects a duration like `30s`, `5m`, `2h`, or `24h`"
                .to_string(),
        )
    })?;
    if parsed.is_zero() {
        return Err(ToolError::InvalidArgs(
            "invalid schedule: every duration must be greater than zero".to_string(),
        ));
    }
    Ok(normalized.to_string())
}

fn normalize_cron_expr(expr: &str) -> Result<String, ToolError> {
    if let Some(shorthand) = parse_daily_time_shorthand(expr)? {
        return Ok(shorthand);
    }

    let fields = expr.split_whitespace().collect::<Vec<_>>();
    match fields.len() {
        5 => Ok(format!("0 {}", fields.join(" "))),
        6 | 7 => Ok(fields.join(" ")),
        _ => Err(ToolError::InvalidArgs(
            "invalid schedule: cron expects 5, 6, or 7 fields; examples: `0 8 * * *`, `0 0 8 * * *`, or daily shorthand `8:00`".to_string(),
        )),
    }
}

fn parse_daily_time_shorthand(expr: &str) -> Result<Option<String>, ToolError> {
    let parts = expr.trim().split(':').collect::<Vec<_>>();
    if !(parts.len() == 2 || parts.len() == 3) {
        return Ok(None);
    }

    let hour = parts[0].parse::<u32>().map_err(|_| {
        ToolError::InvalidArgs(
            "invalid schedule: daily time shorthand must look like `8:00` or `08:00:30`"
                .to_string(),
        )
    })?;
    let minute = parts[1].parse::<u32>().map_err(|_| {
        ToolError::InvalidArgs(
            "invalid schedule: daily time shorthand must look like `8:00` or `08:00:30`"
                .to_string(),
        )
    })?;
    let second = if parts.len() == 3 {
        parts[2].parse::<u32>().map_err(|_| {
            ToolError::InvalidArgs(
                "invalid schedule: daily time shorthand must look like `8:00` or `08:00:30`"
                    .to_string(),
            )
        })?
    } else {
        0
    };

    if hour > 23 || minute > 59 || second > 59 {
        return Err(ToolError::InvalidArgs(
            "invalid schedule: daily time shorthand must be within 00:00:00 to 23:59:59"
                .to_string(),
        ));
    }

    Ok(Some(format!("{second} {minute} {hour} * * *")))
}

fn validate_inbound_payload_value(payload: &Value) -> Result<(), String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "payload must be a JSON object".to_string())?;

    require_string_field(object, "channel")?;
    require_string_field(object, "sender_id")?;
    require_string_field(object, "chat_id")?;
    require_string_field(object, "session_key")?;
    require_string_field(object, "content")?;

    match object.get("metadata") {
        Some(Value::Object(_)) => {}
        Some(_) => return Err("`metadata` must be a JSON object".to_string()),
        None => return Err("missing required field `metadata`".to_string()),
    }

    if let Some(media_references) = object.get("media_references") {
        if !media_references.is_array() {
            return Err("`media_references` must be an array when provided".to_string());
        }
    }

    Ok(())
}

fn require_string_field(object: &serde_json::Map<String, Value>, key: &str) -> Result<(), String> {
    match object.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(()),
        Some(Value::String(_)) => Err(format!("`{key}` cannot be empty")),
        Some(_) => Err(format!("`{key}` must be a string")),
        None => Err(format!("missing required field `{key}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_storage::{CronTaskStatus, NewCronTaskRun};
    use serde_json::json;
    use std::collections::BTreeMap;
    use tokio::sync::Mutex;

    #[derive(Default)]
    struct MockCronStorage {
        jobs: Mutex<BTreeMap<String, CronJob>>,
        runs: Mutex<Vec<CronTaskRun>>,
    }

    #[async_trait]
    impl CronStorage for MockCronStorage {
        async fn create_cron(&self, input: &NewCronJob) -> Result<CronJob, StorageError> {
            let now = now_ms();
            let job = CronJob {
                id: input.id.clone(),
                name: input.name.clone(),
                schedule_kind: input.schedule_kind,
                schedule_expr: input.schedule_expr.clone(),
                payload_json: input.payload_json.clone(),
                enabled: input.enabled,
                timezone: input.timezone.clone(),
                next_run_at_ms: input.next_run_at_ms,
                last_run_at_ms: None,
                created_at_ms: now,
                updated_at_ms: now,
            };
            self.jobs.lock().await.insert(job.id.clone(), job.clone());
            Ok(job)
        }

        async fn update_cron(
            &self,
            cron_id: &str,
            patch: &UpdateCronJobPatch,
        ) -> Result<CronJob, StorageError> {
            let mut jobs = self.jobs.lock().await;
            let current = jobs
                .get_mut(cron_id)
                .ok_or_else(|| StorageError::backend("cron not found"))?;
            if let Some(v) = patch.name.as_ref() {
                current.name = v.clone();
            }
            if let Some(v) = patch.schedule_kind {
                current.schedule_kind = v;
            }
            if let Some(v) = patch.schedule_expr.as_ref() {
                current.schedule_expr = v.clone();
            }
            if let Some(v) = patch.payload_json.as_ref() {
                current.payload_json = v.clone();
            }
            if let Some(v) = patch.timezone.as_ref() {
                current.timezone = v.clone();
            }
            if let Some(v) = patch.next_run_at_ms {
                current.next_run_at_ms = v;
            }
            current.updated_at_ms = now_ms();
            Ok(current.clone())
        }

        async fn set_enabled(&self, cron_id: &str, enabled: bool) -> Result<(), StorageError> {
            let mut jobs = self.jobs.lock().await;
            let current = jobs
                .get_mut(cron_id)
                .ok_or_else(|| StorageError::backend("cron not found"))?;
            current.enabled = enabled;
            Ok(())
        }

        async fn delete_cron(&self, cron_id: &str) -> Result<(), StorageError> {
            self.jobs.lock().await.remove(cron_id);
            Ok(())
        }

        async fn get_cron(&self, cron_id: &str) -> Result<CronJob, StorageError> {
            self.jobs
                .lock()
                .await
                .get(cron_id)
                .cloned()
                .ok_or_else(|| StorageError::backend("cron not found"))
        }

        async fn list_crons(&self, limit: i64, offset: i64) -> Result<Vec<CronJob>, StorageError> {
            let mut out = self.jobs.lock().await.values().cloned().collect::<Vec<_>>();
            out.sort_by_key(|j| std::cmp::Reverse(j.updated_at_ms));
            let skip = offset.max(0) as usize;
            let take = limit.max(1) as usize;
            Ok(out.into_iter().skip(skip).take(take).collect())
        }

        async fn list_due_crons(
            &self,
            now_ms: i64,
            limit: i64,
        ) -> Result<Vec<CronJob>, StorageError> {
            let mut out = self
                .jobs
                .lock()
                .await
                .values()
                .filter(|j| j.enabled && j.next_run_at_ms <= now_ms)
                .cloned()
                .collect::<Vec<_>>();
            out.sort_by_key(|j| j.next_run_at_ms);
            out.truncate(limit.max(1) as usize);
            Ok(out)
        }

        async fn claim_next_run(
            &self,
            _cron_id: &str,
            _expected_next_run_at_ms: i64,
            _new_next_run_at_ms: i64,
            _now_ms: i64,
        ) -> Result<bool, StorageError> {
            Ok(true)
        }

        async fn append_task_run(
            &self,
            input: &NewCronTaskRun,
        ) -> Result<CronTaskRun, StorageError> {
            let run = CronTaskRun {
                id: input.id.clone(),
                cron_id: input.cron_id.clone(),
                scheduled_at_ms: input.scheduled_at_ms,
                started_at_ms: None,
                finished_at_ms: None,
                status: input.status,
                attempt: input.attempt,
                error_message: None,
                published_message_id: None,
                created_at_ms: input.created_at_ms,
            };
            self.runs.lock().await.push(run.clone());
            Ok(run)
        }

        async fn mark_task_running(
            &self,
            _run_id: &str,
            _started_at_ms: i64,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        async fn mark_task_result(
            &self,
            _run_id: &str,
            _status: CronTaskStatus,
            _finished_at_ms: i64,
            _error_message: Option<&str>,
            _published_message_id: Option<&str>,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        async fn list_task_runs(
            &self,
            cron_id: &str,
            limit: i64,
            offset: i64,
        ) -> Result<Vec<CronTaskRun>, StorageError> {
            let mut out = self
                .runs
                .lock()
                .await
                .iter()
                .filter(|r| r.cron_id == cron_id)
                .cloned()
                .collect::<Vec<_>>();
            out.sort_by_key(|r| r.created_at_ms);
            let skip = offset.max(0) as usize;
            let take = limit.max(1) as usize;
            Ok(out.into_iter().skip(skip).take(take).collect())
        }
    }

    fn ctx() -> ToolContext {
        ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn create_and_get_cron_job() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage);

        let create = tool
            .execute(
                json!({
                    "action": "create",
                    "id": "job-1",
                    "name": "heartbeat",
                    "schedule_kind": "every",
                    "schedule_expr": "30s",
                    "payload": {
                        "channel": "cron",
                        "sender_id": "system",
                        "chat_id": "chat-1",
                        "session_key": "cron:chat-1",
                        "content": "ping",
                        "metadata": {}
                    }
                }),
                &ctx(),
            )
            .await
            .expect("create should succeed");
        assert!(create.content_for_model.contains("\"action\": \"create\""));

        let get = tool
            .execute(json!({"action":"get","id":"job-1"}), &ctx())
            .await
            .expect("get should succeed");
        assert!(get.content_for_model.contains("\"id\": \"job-1\""));
    }

    #[tokio::test]
    async fn set_enabled_and_delete_cron_job() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage.clone());
        storage
            .create_cron(&NewCronJob {
                id: "job-2".to_string(),
                name: "disable-me".to_string(),
                schedule_kind: CronScheduleKind::Every,
                schedule_expr: "1m".to_string(),
                payload_json: "{\"channel\":\"cron\",\"sender_id\":\"s\",\"chat_id\":\"c\",\"session_key\":\"cron:c\",\"content\":\"x\",\"metadata\":{}}".to_string(),
                enabled: true,
                timezone: "UTC".to_string(),
                next_run_at_ms: now_ms() + 60_000,
            })
            .await
            .expect("seed");

        let out = tool
            .execute(
                json!({"action":"set_enabled","id":"job-2","enabled":false}),
                &ctx(),
            )
            .await
            .expect("set_enabled");
        assert!(out.content_for_model.contains("\"enabled\": false"));

        let out = tool
            .execute(json!({"action":"delete","id":"job-2"}), &ctx())
            .await
            .expect("delete");
        assert!(out.content_for_model.contains("\"deleted\": true"));
    }

    #[tokio::test]
    async fn create_accepts_string_payload_boolean_string_and_inferred_every() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage.clone());

        tool.execute(
            json!({
                "action": "create",
                "id": "job-3",
                "name": "weather",
                "schedule_expr": "every 24h",
                "payload": "{\"channel\":\"cron\",\"sender_id\":\"system\",\"chat_id\":\"chat-1\",\"session_key\":\"cron:chat-1\",\"content\":\"weather\",\"metadata\":{}}",
                "enabled": "true"
            }),
            &ctx(),
        )
        .await
        .expect("create should succeed");

        let job = storage.get_cron("job-3").await.expect("job should exist");
        assert_eq!(job.schedule_kind, CronScheduleKind::Every);
        assert_eq!(job.schedule_expr, "24h");
        assert!(job.enabled);
    }

    #[tokio::test]
    async fn create_accepts_five_field_cron() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage.clone());

        tool.execute(
            json!({
                "action": "create",
                "id": "job-4",
                "name": "daily weather",
                "schedule_kind": "cron",
                "schedule_expr": "0 8 * * *",
                "payload": {
                    "channel": "cron",
                    "sender_id": "system",
                    "chat_id": "chat-1",
                    "session_key": "cron:chat-1",
                    "content": "weather",
                    "metadata": {}
                }
            }),
            &ctx(),
        )
        .await
        .expect("create should succeed");

        let job = storage.get_cron("job-4").await.expect("job should exist");
        assert_eq!(job.schedule_kind, CronScheduleKind::Cron);
        assert_eq!(job.schedule_expr, "0 0 8 * * *");
    }

    #[tokio::test]
    async fn create_accepts_daily_time_shorthand() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage.clone());

        tool.execute(
            json!({
                "action": "create",
                "id": "job-5",
                "name": "daily shorthand",
                "schedule_expr": "8:00",
                "payload": {
                    "channel": "cron",
                    "sender_id": "system",
                    "chat_id": "chat-1",
                    "session_key": "cron:chat-1",
                    "content": "weather",
                    "metadata": {}
                }
            }),
            &ctx(),
        )
        .await
        .expect("create should succeed");

        let job = storage.get_cron("job-5").await.expect("job should exist");
        assert_eq!(job.schedule_kind, CronScheduleKind::Cron);
        assert_eq!(job.schedule_expr, "0 0 8 * * *");
    }

    #[tokio::test]
    async fn create_rejects_payload_missing_inbound_message_fields() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage);

        let err = tool
            .execute(
                json!({
                    "action": "create",
                    "id": "job-invalid",
                    "name": "invalid payload",
                    "schedule_expr": "5m",
                    "payload": {
                        "sender_id": "system",
                        "chat_id": "chat-1",
                        "session_key": "cron:chat-1",
                        "content": "weather",
                        "metadata": {}
                    }
                }),
                &ctx(),
            )
            .await
            .expect_err("create should fail");

        assert!(err
            .to_string()
            .contains("must decode to a valid InboundMessage-like object"));
    }

    #[tokio::test]
    async fn create_accepts_message_shortcut_and_infers_context_payload() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage.clone());

        tool.execute(
            json!({
                "action": "create",
                "id": "job-shortcut",
                "name": "weather shortcut",
                "schedule_expr": "24h",
                "message": "请查询无锡今天的天气情况"
            }),
            &ToolContext {
                session_key: "dingtalk:chat-99".to_string(),
                metadata: BTreeMap::new(),
            },
        )
        .await
        .expect("create should succeed");

        let job = storage
            .get_cron("job-shortcut")
            .await
            .expect("job should exist");
        let payload: Value = serde_json::from_str(&job.payload_json).expect("payload json");
        assert_eq!(
            payload.get("channel").and_then(Value::as_str),
            Some("dingtalk")
        );
        assert_eq!(
            payload.get("chat_id").and_then(Value::as_str),
            Some("chat-99")
        );
        assert_eq!(
            payload.get("session_key").and_then(Value::as_str),
            Some("cron:job-shortcut")
        );
        assert_eq!(
            payload.get("sender_id").and_then(Value::as_str),
            Some("system")
        );
    }

    #[tokio::test]
    async fn create_message_shortcut_inherits_dingtalk_webhook_metadata() {
        let storage = Arc::new(MockCronStorage::default());
        let tool = CronManagerTool::from_storage(storage.clone());

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "channel.dingtalk.session_webhook".to_string(),
            json!("https://example/session"),
        );
        metadata.insert("channel.dingtalk.bot_title".to_string(), json!("Klaw Bot"));
        metadata.insert("channel.delivery_mode".to_string(), json!("direct_reply"));

        tool.execute(
            json!({
                "action": "create",
                "id": "job-dingtalk-shortcut",
                "name": "weather shortcut",
                "schedule_expr": "24h",
                "message": "请查询无锡今天的天气情况"
            }),
            &ToolContext {
                session_key: "dingtalk:account-1:chat-99".to_string(),
                metadata,
            },
        )
        .await
        .expect("create should succeed");

        let job = storage
            .get_cron("job-dingtalk-shortcut")
            .await
            .expect("job should exist");
        let payload: Value = serde_json::from_str(&job.payload_json).expect("payload json");
        let meta = payload
            .get("metadata")
            .and_then(Value::as_object)
            .expect("metadata object");
        assert_eq!(
            meta.get("channel.dingtalk.session_webhook")
                .and_then(Value::as_str),
            Some("https://example/session")
        );
        assert_eq!(
            meta.get("channel.dingtalk.bot_title")
                .and_then(Value::as_str),
            Some("Klaw Bot")
        );
        assert_eq!(
            meta.get("cron.base_session_key").and_then(Value::as_str),
            Some("dingtalk:account-1:chat-99")
        );
        assert!(meta.get("channel.delivery_mode").is_none());
        assert_eq!(
            payload.get("chat_id").and_then(Value::as_str),
            Some("chat-99")
        );
        assert_eq!(
            payload.get("session_key").and_then(Value::as_str),
            Some("cron:job-dingtalk-shortcut")
        );
    }
}
