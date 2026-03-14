use async_trait::async_trait;
use chrono::Utc;
use klaw_storage::{
    open_default_store, CronJob, CronScheduleKind, CronStorage, CronTaskRun, NewCronJob,
    StorageError, UpdateCronJobPatch,
};
use serde_json::{json, Value};
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
            Some(v) => v
                .as_bool()
                .map(Some)
                .ok_or_else(|| ToolError::InvalidArgs(format!("`{key}` must be a boolean"))),
        }
    }

    fn parse_schedule_kind(raw: &str) -> Result<CronScheduleKind, ToolError> {
        CronScheduleKind::parse(raw).ok_or_else(|| {
            ToolError::InvalidArgs("`schedule_kind` must be one of cron/every".to_string())
        })
    }

    fn parse_payload_json(args: &Value) -> Result<String, ToolError> {
        if let Some(raw) = args.get("payload_json") {
            let payload = raw
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ToolError::InvalidArgs("`payload_json` must be a string".to_string())
                })?;
            serde_json::from_str::<Value>(payload).map_err(|err| {
                ToolError::InvalidArgs(format!("`payload_json` is not valid json: {err}"))
            })?;
            return Ok(payload.to_string());
        }

        let payload = args.get("payload").cloned().ok_or_else(|| {
            ToolError::InvalidArgs("missing `payload` or `payload_json`".to_string())
        })?;
        if !payload.is_object() {
            return Err(ToolError::InvalidArgs(
                "`payload` must be a JSON object".to_string(),
            ));
        }
        serde_json::to_string(&payload).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize `payload` to json failed: {err}"))
        })
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

    async fn do_create(&self, args: &Value) -> Result<Value, ToolError> {
        let id = Self::optional_str(args, "id")?.unwrap_or_else(|| Uuid::new_v4().to_string());
        let name = Self::require_str(args, "name")?;
        let schedule_kind = Self::parse_schedule_kind(&Self::require_str(args, "schedule_kind")?)?;
        let schedule_expr = Self::require_str(args, "schedule_expr")?;
        let payload_json = Self::parse_payload_json(args)?;
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

    async fn do_update(&self, args: &Value) -> Result<Value, ToolError> {
        let cron_id = Self::require_str(args, "id")?;
        let current = self
            .storage
            .get_cron(&cron_id)
            .await
            .map_err(map_storage_err)?;

        let schedule_kind = match Self::optional_str(args, "schedule_kind")? {
            Some(v) => Some(Self::parse_schedule_kind(&v)?),
            None => None,
        };
        let schedule_expr = Self::optional_str(args, "schedule_expr")?;

        let payload_json = if args.get("payload").is_some() || args.get("payload_json").is_some() {
            Some(Self::parse_payload_json(args)?)
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
        "Manage scheduled cron jobs. Supports create/update/delete/get/list_due/list_runs/set_enabled for persisted cron tasks and run records."
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
                        "schedule_kind": { "type": "string", "enum": ["cron", "every"] },
                        "schedule_expr": { "type": "string", "description": "Cron expression or every interval string (e.g. 30s, 5m)." },
                        "payload": { "type": "object", "description": "InboundMessage payload object to publish when triggered." },
                        "payload_json": { "type": "string", "description": "Payload JSON string alternative to `payload`." },
                        "enabled": { "type": "boolean", "description": "Whether the cron starts enabled. Defaults to true." },
                        "timezone": { "type": "string", "description": "Timezone label. Defaults to UTC." },
                        "next_run_at_ms": { "type": "integer", "description": "Optional explicit next run timestamp in ms." }
                    },
                    "required": ["action", "name", "schedule_kind", "schedule_expr"],
                    "anyOf": [
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
                        "schedule_kind": { "type": "string", "enum": ["cron", "every"] },
                        "schedule_expr": { "type": "string", "description": "Updated schedule expression." },
                        "payload": { "type": "object", "description": "Updated payload object." },
                        "payload_json": { "type": "string", "description": "Updated payload JSON string." },
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
            "create" => self.do_create(&args).await?,
            "update" => self.do_update(&args).await?,
            "delete" => self.do_delete(&args).await?,
            "get" => self.do_get(&args).await?,
            "set_enabled" => self.do_set_enabled(&args).await?,
            "list_due" => self.do_list_due(&args).await?,
            "list_runs" => self.do_list_runs(&args).await?,
            _ => return Err(ToolError::InvalidArgs(
                "`action` must be one of create/update/delete/get/list_due/list_runs/set_enabled"
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
}
