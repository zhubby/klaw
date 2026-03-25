use crate::{
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronScheduleKind, CronStorage,
    CronTaskRun, CronTaskStatus, HeartbeatJob, HeartbeatStorage, HeartbeatTaskRun,
    HeartbeatTaskStatus, LlmAuditFilterOptions, LlmAuditFilterOptionsQuery, LlmAuditQuery,
    LlmAuditRecord, LlmAuditSortOrder, LlmAuditStatus, LlmUsageRecord, LlmUsageSource,
    LlmUsageSummary, NewApprovalRecord, NewCronJob, NewCronTaskRun, NewHeartbeatJob,
    NewHeartbeatTaskRun, NewLlmAuditRecord, NewLlmUsageRecord, NewWebhookEventRecord,
    SessionCompressionState, SessionIndex, SessionStorage, StorageError, StoragePaths,
    UpdateCronJobPatch, UpdateHeartbeatJobPatch, UpdateWebhookEventResult, WebhookEventQuery,
    WebhookEventRecord, WebhookEventSortOrder, WebhookEventStatus, jsonl,
    memory_db::{DbRow, DbValue, MemoryDb},
    util::{now_ms, relative_or_absolute_jsonl},
};
use async_trait::async_trait;
use sqlx::{
    Column, FromRow, Row, SqlitePool, TypeInfo,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SqlxSessionStore {
    paths: StoragePaths,
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct SqlxMemoryDb {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct SqlxArchiveDb {
    pool: SqlitePool,
}

#[derive(Debug, Clone, FromRow)]
struct SessionIndexRow {
    session_key: String,
    chat_id: String,
    channel: String,
    active_session_key: Option<String>,
    model_provider: Option<String>,
    model: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    last_message_at_ms: i64,
    turn_count: i64,
    jsonl_path: String,
}

#[derive(Debug, Clone, FromRow)]
struct CronJobRow {
    id: String,
    name: String,
    schedule_kind: String,
    schedule_expr: String,
    payload_json: String,
    enabled: i64,
    timezone: String,
    next_run_at_ms: i64,
    last_run_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
struct CronTaskRunRow {
    id: String,
    cron_id: String,
    scheduled_at_ms: i64,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    status: String,
    attempt: i64,
    error_message: Option<String>,
    published_message_id: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
struct HeartbeatJobRow {
    id: String,
    session_key: String,
    channel: String,
    chat_id: String,
    enabled: i64,
    every: String,
    prompt: String,
    silent_ack_token: String,
    timezone: String,
    next_run_at_ms: i64,
    last_run_at_ms: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
struct HeartbeatTaskRunRow {
    id: String,
    heartbeat_id: String,
    scheduled_at_ms: i64,
    started_at_ms: Option<i64>,
    finished_at_ms: Option<i64>,
    status: String,
    attempt: i64,
    error_message: Option<String>,
    published_message_id: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
struct ApprovalRow {
    id: String,
    session_key: String,
    tool_name: String,
    command_hash: String,
    command_preview: String,
    command_text: String,
    risk_level: String,
    status: String,
    requested_by: String,
    approved_by: Option<String>,
    justification: Option<String>,
    expires_at_ms: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    consumed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct LlmUsageRow {
    id: String,
    session_key: String,
    chat_id: String,
    turn_index: i64,
    request_seq: i64,
    provider: String,
    model: String,
    wire_api: String,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cached_input_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    source: String,
    provider_request_id: Option<String>,
    provider_response_id: Option<String>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
struct LlmUsageSummaryRow {
    request_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cached_input_tokens: i64,
    reasoning_tokens: i64,
}

#[derive(Debug, Clone, FromRow)]
struct LlmAuditRow {
    id: String,
    session_key: String,
    chat_id: String,
    turn_index: i64,
    request_seq: i64,
    provider: String,
    model: String,
    wire_api: String,
    status: String,
    error_code: Option<String>,
    error_message: Option<String>,
    provider_request_id: Option<String>,
    provider_response_id: Option<String>,
    request_body_json: String,
    response_body_json: Option<String>,
    requested_at_ms: i64,
    responded_at_ms: Option<i64>,
    created_at_ms: i64,
}

#[derive(Debug, Clone, FromRow)]
struct WebhookEventRow {
    id: String,
    source: String,
    event_type: String,
    session_key: String,
    chat_id: String,
    sender_id: String,
    content: String,
    payload_json: Option<String>,
    metadata_json: Option<String>,
    status: String,
    error_message: Option<String>,
    response_summary: Option<String>,
    received_at_ms: i64,
    processed_at_ms: Option<i64>,
    remote_addr: Option<String>,
    created_at_ms: i64,
}

impl From<SessionIndexRow> for SessionIndex {
    fn from(value: SessionIndexRow) -> Self {
        Self {
            session_key: value.session_key,
            chat_id: value.chat_id,
            channel: value.channel,
            active_session_key: value.active_session_key,
            model_provider: value.model_provider,
            model: value.model,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            last_message_at_ms: value.last_message_at_ms,
            turn_count: value.turn_count,
            jsonl_path: value.jsonl_path,
        }
    }
}

impl TryFrom<CronJobRow> for CronJob {
    type Error = StorageError;

    fn try_from(value: CronJobRow) -> Result<Self, Self::Error> {
        let schedule_kind = CronScheduleKind::parse(&value.schedule_kind).ok_or_else(|| {
            StorageError::backend(format!(
                "invalid cron schedule kind: {}",
                value.schedule_kind
            ))
        })?;
        Ok(Self {
            id: value.id,
            name: value.name,
            schedule_kind,
            schedule_expr: value.schedule_expr,
            payload_json: value.payload_json,
            enabled: value.enabled != 0,
            timezone: value.timezone,
            next_run_at_ms: value.next_run_at_ms,
            last_run_at_ms: value.last_run_at_ms,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        })
    }
}

impl TryFrom<CronTaskRunRow> for CronTaskRun {
    type Error = StorageError;

    fn try_from(value: CronTaskRunRow) -> Result<Self, Self::Error> {
        let status = CronTaskStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid cron task status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            cron_id: value.cron_id,
            scheduled_at_ms: value.scheduled_at_ms,
            started_at_ms: value.started_at_ms,
            finished_at_ms: value.finished_at_ms,
            status,
            attempt: value.attempt,
            error_message: value.error_message,
            published_message_id: value.published_message_id,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl From<HeartbeatJobRow> for HeartbeatJob {
    fn from(value: HeartbeatJobRow) -> Self {
        Self {
            id: value.id,
            session_key: value.session_key,
            channel: value.channel,
            chat_id: value.chat_id,
            enabled: value.enabled != 0,
            every: value.every,
            prompt: value.prompt,
            silent_ack_token: value.silent_ack_token,
            timezone: value.timezone,
            next_run_at_ms: value.next_run_at_ms,
            last_run_at_ms: value.last_run_at_ms,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl TryFrom<HeartbeatTaskRunRow> for HeartbeatTaskRun {
    type Error = StorageError;

    fn try_from(value: HeartbeatTaskRunRow) -> Result<Self, Self::Error> {
        let status = HeartbeatTaskStatus::parse(&value.status)
            .ok_or_else(|| StorageError::backend("invalid heartbeat task status"))?;
        Ok(Self {
            id: value.id,
            heartbeat_id: value.heartbeat_id,
            scheduled_at_ms: value.scheduled_at_ms,
            started_at_ms: value.started_at_ms,
            finished_at_ms: value.finished_at_ms,
            status,
            attempt: value.attempt,
            error_message: value.error_message,
            published_message_id: value.published_message_id,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl TryFrom<ApprovalRow> for ApprovalRecord {
    type Error = StorageError;

    fn try_from(value: ApprovalRow) -> Result<Self, Self::Error> {
        let status = ApprovalStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid approval status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            tool_name: value.tool_name,
            command_hash: value.command_hash,
            command_preview: value.command_preview,
            command_text: value.command_text,
            risk_level: value.risk_level,
            status,
            requested_by: value.requested_by,
            approved_by: value.approved_by,
            justification: value.justification,
            expires_at_ms: value.expires_at_ms,
            created_at_ms: value.created_at_ms,
            updated_at_ms: value.updated_at_ms,
            consumed_at_ms: value.consumed_at_ms,
        })
    }
}

impl TryFrom<LlmUsageRow> for LlmUsageRecord {
    type Error = StorageError;

    fn try_from(value: LlmUsageRow) -> Result<Self, Self::Error> {
        let source = LlmUsageSource::parse(&value.source).ok_or_else(|| {
            StorageError::backend(format!("invalid llm usage source: {}", value.source))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            chat_id: value.chat_id,
            turn_index: value.turn_index,
            request_seq: value.request_seq,
            provider: value.provider,
            model: value.model,
            wire_api: value.wire_api,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.total_tokens,
            cached_input_tokens: value.cached_input_tokens,
            reasoning_tokens: value.reasoning_tokens,
            source,
            provider_request_id: value.provider_request_id,
            provider_response_id: value.provider_response_id,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl From<LlmUsageSummaryRow> for LlmUsageSummary {
    fn from(value: LlmUsageSummaryRow) -> Self {
        Self {
            request_count: value.request_count,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.total_tokens,
            cached_input_tokens: value.cached_input_tokens,
            reasoning_tokens: value.reasoning_tokens,
        }
    }
}

impl TryFrom<LlmAuditRow> for LlmAuditRecord {
    type Error = StorageError;

    fn try_from(value: LlmAuditRow) -> Result<Self, Self::Error> {
        let status = LlmAuditStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid llm audit status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            session_key: value.session_key,
            chat_id: value.chat_id,
            turn_index: value.turn_index,
            request_seq: value.request_seq,
            provider: value.provider,
            model: value.model,
            wire_api: value.wire_api,
            status,
            error_code: value.error_code,
            error_message: value.error_message,
            provider_request_id: value.provider_request_id,
            provider_response_id: value.provider_response_id,
            request_body_json: value.request_body_json,
            response_body_json: value.response_body_json,
            requested_at_ms: value.requested_at_ms,
            responded_at_ms: value.responded_at_ms,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl TryFrom<WebhookEventRow> for WebhookEventRecord {
    type Error = StorageError;

    fn try_from(value: WebhookEventRow) -> Result<Self, Self::Error> {
        let status = WebhookEventStatus::parse(&value.status).ok_or_else(|| {
            StorageError::backend(format!("invalid webhook event status: {}", value.status))
        })?;
        Ok(Self {
            id: value.id,
            source: value.source,
            event_type: value.event_type,
            session_key: value.session_key,
            chat_id: value.chat_id,
            sender_id: value.sender_id,
            content: value.content,
            payload_json: value.payload_json,
            metadata_json: value.metadata_json,
            status,
            error_message: value.error_message,
            response_summary: value.response_summary,
            received_at_ms: value.received_at_ms,
            processed_at_ms: value.processed_at_ms,
            remote_addr: value.remote_addr,
            created_at_ms: value.created_at_ms,
        })
    }
}

impl SqlxSessionStore {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let connect_options = SqliteConnectOptions::new()
            .filename(&paths.db_path)
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options)
            .await
            .map_err(StorageError::backend)?;
        let store = Self { paths, pool };
        store
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
            .await?;
        store.init().await?;
        Ok(store)
    }

    async fn init(&self) -> Result<(), StorageError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                session_key TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                channel TEXT NOT NULL,
                active_session_key TEXT,
                model_provider TEXT,
                model TEXT,
                compression_last_len INTEGER NOT NULL DEFAULT 0,
                compression_summary_json TEXT,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                last_message_at_ms INTEGER NOT NULL,
                turn_count INTEGER NOT NULL DEFAULT 0,
                jsonl_path TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.ensure_session_column(
            "active_session_key",
            "ALTER TABLE sessions ADD COLUMN active_session_key TEXT",
        )
        .await?;
        self.ensure_session_column(
            "model_provider",
            "ALTER TABLE sessions ADD COLUMN model_provider TEXT",
        )
        .await?;
        self.ensure_session_column("model", "ALTER TABLE sessions ADD COLUMN model TEXT")
            .await?;
        self.ensure_session_column(
            "compression_last_len",
            "ALTER TABLE sessions ADD COLUMN compression_last_len INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        self.ensure_session_column(
            "compression_summary_json",
            "ALTER TABLE sessions ADD COLUMN compression_summary_json TEXT",
        )
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_sessions_updated_at_ms
             ON sessions(updated_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS llm_usage (
                id TEXT PRIMARY KEY,
                session_key TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                turn_index INTEGER NOT NULL,
                request_seq INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                wire_api TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                cached_input_tokens INTEGER,
                reasoning_tokens INTEGER,
                source TEXT NOT NULL,
                provider_request_id TEXT,
                provider_response_id TEXT,
                created_at_ms INTEGER NOT NULL,
                FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_llm_usage_session_created
             ON llm_usage(session_key, created_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_llm_usage_chat_created
             ON llm_usage(chat_id, created_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_llm_usage_session_turn
             ON llm_usage(session_key, turn_index, request_seq)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS llm_audit (
                id TEXT PRIMARY KEY,
                session_key TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                turn_index INTEGER NOT NULL,
                request_seq INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                wire_api TEXT NOT NULL,
                status TEXT NOT NULL,
                error_code TEXT,
                error_message TEXT,
                provider_request_id TEXT,
                provider_response_id TEXT,
                request_body_json TEXT NOT NULL,
                response_body_json TEXT,
                requested_at_ms INTEGER NOT NULL,
                responded_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_llm_audit_session_requested
             ON llm_audit(session_key, requested_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_llm_audit_provider_requested
             ON llm_audit(provider, requested_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_llm_audit_requested
             ON llm_audit(requested_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_llm_audit_session_turn
             ON llm_audit(session_key, turn_index, request_seq)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS webhook_events (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                event_type TEXT NOT NULL,
                session_key TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                content TEXT NOT NULL,
                payload_json TEXT,
                metadata_json TEXT,
                status TEXT NOT NULL,
                error_message TEXT,
                response_summary TEXT,
                received_at_ms INTEGER NOT NULL,
                processed_at_ms INTEGER,
                remote_addr TEXT,
                created_at_ms INTEGER NOT NULL,
                FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_events_received
             ON webhook_events(received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_events_source_received
             ON webhook_events(source, received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_events_status_received
             ON webhook_events(status, received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_events_session_received
             ON webhook_events(session_key, received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cron (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                schedule_kind TEXT NOT NULL,
                schedule_expr TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                timezone TEXT NOT NULL DEFAULT 'UTC',
                next_run_at_ms INTEGER NOT NULL,
                last_run_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS cron_task (
                id TEXT PRIMARY KEY,
                cron_id TEXT NOT NULL,
                scheduled_at_ms INTEGER NOT NULL,
                started_at_ms INTEGER,
                finished_at_ms INTEGER,
                status TEXT NOT NULL,
                attempt INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                published_message_id TEXT,
                created_at_ms INTEGER NOT NULL,
                FOREIGN KEY (cron_id) REFERENCES cron(id) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS heartbeat (
                id TEXT PRIMARY KEY,
                session_key TEXT NOT NULL UNIQUE,
                channel TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                every TEXT NOT NULL,
                prompt TEXT NOT NULL,
                silent_ack_token TEXT NOT NULL,
                timezone TEXT NOT NULL DEFAULT 'UTC',
                next_run_at_ms INTEGER NOT NULL,
                last_run_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS heartbeat_task (
                id TEXT PRIMARY KEY,
                heartbeat_id TEXT NOT NULL,
                scheduled_at_ms INTEGER NOT NULL,
                started_at_ms INTEGER,
                finished_at_ms INTEGER,
                status TEXT NOT NULL,
                attempt INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                published_message_id TEXT,
                created_at_ms INTEGER NOT NULL,
                FOREIGN KEY (heartbeat_id) REFERENCES heartbeat(id) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cron_enabled_next_run
             ON cron(enabled, next_run_at_ms)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cron_task_cron_created
             ON cron_task(cron_id, created_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_cron_task_status_scheduled
             ON cron_task(status, scheduled_at_ms)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_heartbeat_enabled_next_run
             ON heartbeat(enabled, next_run_at_ms)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_heartbeat_task_heartbeat_created
             ON heartbeat_task(heartbeat_id, created_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_heartbeat_task_status_scheduled
             ON heartbeat_task(status, scheduled_at_ms)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS approvals (
                id TEXT PRIMARY KEY,
                session_key TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                command_hash TEXT NOT NULL,
                command_preview TEXT NOT NULL,
                command_text TEXT NOT NULL,
                risk_level TEXT NOT NULL,
                status TEXT NOT NULL,
                requested_by TEXT NOT NULL,
                approved_by TEXT,
                justification TEXT,
                expires_at_ms INTEGER NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                consumed_at_ms INTEGER,
                FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.ensure_approval_column(
            "command_text",
            "ALTER TABLE approvals ADD COLUMN command_text TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_approvals_session_status
             ON approvals(session_key, status, created_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_approvals_expiry
             ON approvals(status, expires_at_ms)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn ensure_session_column(&self, column: &str, sql: &str) -> Result<(), StorageError> {
        let result = sqlx::query(sql).execute(&self.pool).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name") {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure sessions.{column} column: {message}"
                )))
            }
        }
    }

    async fn ensure_approval_column(&self, column: &str, sql: &str) -> Result<(), StorageError> {
        let result = sqlx::query(sql).execute(&self.pool).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name") {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure approvals.{column} column: {message}"
                )))
            }
        }
    }
}

impl SqlxMemoryDb {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let connect_options = SqliteConnectOptions::new()
            .filename(&paths.memory_db_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options)
            .await
            .map_err(StorageError::backend)?;
        let db = Self { pool };
        db.execute_batch("PRAGMA journal_mode = WAL;")
            .await
            .map_err(StorageError::backend)?;
        Ok(db)
    }
}

impl SqlxArchiveDb {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let connect_options = SqliteConnectOptions::new()
            .filename(&paths.archive_db_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options)
            .await
            .map_err(StorageError::backend)?;
        let db = Self { pool };
        db.execute_batch("PRAGMA journal_mode = WAL;")
            .await
            .map_err(StorageError::backend)?;
        Ok(db)
    }
}

#[async_trait]
impl MemoryDb for SqlxSessionStore {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        sqlx::raw_sql(sql)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(StorageError::backend)
    }

    async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, StorageError> {
        let mut query = sqlx::query(sql);
        for value in params {
            query = bind_db_value(query, value.clone());
        }
        let result = query
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(result.rows_affected())
    }

    async fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, StorageError> {
        let mut query = sqlx::query(sql);
        for value in params {
            query = bind_db_value(query, value.clone());
        }
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let mut values = Vec::with_capacity(row.columns().len());
            for idx in 0..row.columns().len() {
                values.push(sqlx_row_value(&row, idx)?);
            }
            out.push(DbRow { values });
        }
        Ok(out)
    }
}

#[async_trait]
impl MemoryDb for SqlxMemoryDb {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        sqlx::raw_sql(sql)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(StorageError::backend)
    }

    async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, StorageError> {
        let mut query = sqlx::query(sql);
        for value in params {
            query = bind_db_value(query, value.clone());
        }
        let result = query
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(result.rows_affected())
    }

    async fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, StorageError> {
        let mut query = sqlx::query(sql);
        for value in params {
            query = bind_db_value(query, value.clone());
        }
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let mut values = Vec::with_capacity(row.columns().len());
            for idx in 0..row.columns().len() {
                values.push(sqlx_row_value(&row, idx)?);
            }
            out.push(DbRow { values });
        }
        Ok(out)
    }
}

#[async_trait]
impl MemoryDb for SqlxArchiveDb {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        sqlx::raw_sql(sql)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(StorageError::backend)
    }

    async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, StorageError> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = bind_sqlx_value(query, param);
        }
        query
            .execute(&self.pool)
            .await
            .map(|result| result.rows_affected())
            .map_err(StorageError::backend)
    }

    async fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, StorageError> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = bind_sqlx_value(query, param);
        }

        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        rows.iter()
            .map(|row| {
                let mut values = Vec::new();
                for idx in 0..row.len() {
                    values.push(sqlx_row_value(row, idx)?);
                }
                Ok(DbRow { values })
            })
            .collect()
    }
}

#[async_trait]
impl SessionStorage for SqlxSessionStore {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        sqlx::query(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
            ) VALUES (?1, ?2, ?3, NULL, NULL, NULL, ?4, ?5, ?6, 0, ?7)
            ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                last_message_at_ms=excluded.last_message_at_ms,
                jsonl_path=excluded.jsonl_path",
        )
        .bind(session_key)
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(jsonl_path_str)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_session(session_key).await
    }

    async fn complete_turn(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        let updated = sqlx::query(
            "UPDATE sessions
             SET
                chat_id = ?1,
                channel = ?2,
                updated_at_ms = ?3,
                last_message_at_ms = ?4,
                turn_count = turn_count + 1,
                jsonl_path = ?5
             WHERE session_key = ?6",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(now)
        .bind(jsonl_path_str.clone())
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        if updated.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO sessions (
                    session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
                ) VALUES (?1, ?2, ?3, NULL, NULL, NULL, ?4, ?5, ?6, 1, ?7)",
            )
            .bind(session_key)
            .bind(chat_id)
            .bind(channel)
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(jsonl_path_str)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        }

        self.get_session(session_key).await
    }

    async fn append_chat_record(
        &self,
        session_key: &str,
        record: &ChatRecord,
    ) -> Result<(), StorageError> {
        jsonl::append_chat_record(&self.paths, session_key, record).await
    }

    async fn read_chat_records(&self, session_key: &str) -> Result<Vec<ChatRecord>, StorageError> {
        jsonl::read_chat_records(&self.paths, session_key).await
    }

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, StorageError> {
        let row = sqlx::query_as::<_, SessionIndexRow>(
            "SELECT session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn get_or_create_session_state(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        _default_provider: &str,
        _default_model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        sqlx::query(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
            ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, 0, ?8)
            ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                active_session_key=COALESCE(sessions.active_session_key, excluded.active_session_key),
                jsonl_path=excluded.jsonl_path",
        )
        .bind(session_key)
        .bind(chat_id)
        .bind(channel)
        .bind(session_key)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(jsonl_path_str)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_session(session_key).await
    }

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 active_session_key = ?4
             WHERE session_key = ?5",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(active_session_key)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting active_session_key"
            )));
        }
        self.get_session(session_key).await
    }

    async fn set_model_provider(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model_provider: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 model_provider = ?4,
                 model = ?5
             WHERE session_key = ?6",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(model_provider)
        .bind(model)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting model_provider"
            )));
        }
        self.get_session(session_key).await
    }

    async fn set_model(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 model = ?4
             WHERE session_key = ?5",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(model)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting model"
            )));
        }
        self.get_session(session_key).await
    }

    async fn clear_model_routing_override(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE sessions
             SET chat_id = ?1,
                 channel = ?2,
                 updated_at_ms = ?3,
                 model_provider = NULL,
                 model = NULL
             WHERE session_key = ?4",
        )
        .bind(chat_id)
        .bind(channel)
        .bind(now)
        .bind(session_key)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when clearing model routing override"
            )));
        }
        self.get_session(session_key).await
    }

    async fn get_session_compression_state(
        &self,
        session_key: &str,
    ) -> Result<Option<SessionCompressionState>, StorageError> {
        let row = sqlx::query(
            "SELECT compression_last_len, compression_summary_json
             FROM sessions
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        Ok(row.map(|value| SessionCompressionState {
            last_compressed_len: value.get::<i64, _>("compression_last_len"),
            summary_json: value.get::<Option<String>, _>("compression_summary_json"),
        }))
    }

    async fn set_session_compression_state(
        &self,
        session_key: &str,
        state: &SessionCompressionState,
    ) -> Result<(), StorageError> {
        let updated = sqlx::query(
            "UPDATE sessions
             SET compression_last_len = ?2,
                 compression_summary_json = ?3,
                 updated_at_ms = ?4
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .bind(state.last_compressed_len)
        .bind(state.summary_json.as_deref())
        .bind(now_ms())
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting compression state"
            )));
        }
        Ok(())
    }

    async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
        updated_from_ms: Option<i64>,
        updated_to_ms: Option<i64>,
    ) -> Result<Vec<SessionIndex>, StorageError> {
        let mut query = String::from(
            "SELECT session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions WHERE 1=1",
        );
        if updated_from_ms.is_some() {
            query.push_str(" AND updated_at_ms >= ?");
        }
        if updated_to_ms.is_some() {
            query.push_str(" AND updated_at_ms <= ?");
        }
        query.push_str(" ORDER BY updated_at_ms DESC LIMIT ? OFFSET ?");

        let mut q = sqlx::query_as::<_, SessionIndexRow>(&query);
        if let Some(from) = updated_from_ms {
            q = q.bind(from);
        }
        if let Some(to) = updated_to_ms {
            q = q.bind(to);
        }
        q = q.bind(limit.max(1)).bind(offset.max(0));

        let rows = q
            .fetch_all(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO llm_usage (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                source, provider_request_id, provider_response_id, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(input.turn_index)
        .bind(input.request_seq)
        .bind(&input.provider)
        .bind(&input.model)
        .bind(&input.wire_api)
        .bind(input.input_tokens)
        .bind(input.output_tokens)
        .bind(input.total_tokens)
        .bind(input.cached_input_tokens)
        .bind(input.reasoning_tokens)
        .bind(input.source.as_str())
        .bind(&input.provider_request_id)
        .bind(&input.provider_response_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let row = sqlx::query_as::<_, LlmUsageRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        LlmUsageRecord::try_from(row)
    }

    async fn list_llm_usage(
        &self,
        session_key: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LlmUsageRecord>, StorageError> {
        let rows = sqlx::query_as::<_, LlmUsageRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE session_key = ?1
             ORDER BY turn_index DESC, request_seq DESC, created_at_ms DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind(session_key)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(LlmUsageRecord::try_from).collect()
    }

    async fn sum_llm_usage_by_session(
        &self,
        session_key: &str,
    ) -> Result<LlmUsageSummary, StorageError> {
        let row = sqlx::query_as::<_, LlmUsageSummaryRow>(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn sum_llm_usage_by_turn(
        &self,
        session_key: &str,
        turn_index: i64,
    ) -> Result<LlmUsageSummary, StorageError> {
        let row = sqlx::query_as::<_, LlmUsageSummaryRow>(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = ?1 AND turn_index = ?2",
        )
        .bind(session_key)
        .bind(turn_index)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO llm_audit (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                status, error_code, error_message, provider_request_id, provider_response_id,
                request_body_json, response_body_json, requested_at_ms, responded_at_ms, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(input.turn_index)
        .bind(input.request_seq)
        .bind(&input.provider)
        .bind(&input.model)
        .bind(&input.wire_api)
        .bind(input.status.as_str())
        .bind(&input.error_code)
        .bind(&input.error_message)
        .bind(&input.provider_request_id)
        .bind(&input.provider_response_id)
        .bind(&input.request_body_json)
        .bind(&input.response_body_json)
        .bind(input.requested_at_ms)
        .bind(input.responded_at_ms)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let row = sqlx::query_as::<_, LlmAuditRow>(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        LlmAuditRecord::try_from(row)
    }

    async fn list_llm_audit(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, StorageError> {
        let sort_order = match query.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
            LlmAuditSortOrder::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
        };
        let rows = sqlx::query_as::<_, LlmAuditRow>(&format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE (?1 IS NULL OR session_key = ?1)
               AND (?2 IS NULL OR provider = ?2)
               AND (?3 IS NULL OR requested_at_ms >= ?3)
               AND (?4 IS NULL OR requested_at_ms <= ?4)
             ORDER BY {sort_order}
             LIMIT ?5 OFFSET ?6"
        ))
        .bind(query.session_key.as_deref())
        .bind(query.provider.as_deref())
        .bind(query.requested_from_ms)
        .bind(query.requested_to_ms)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(LlmAuditRecord::try_from).collect()
    }

    async fn list_llm_audit_filter_options(
        &self,
        query: &LlmAuditFilterOptionsQuery,
    ) -> Result<LlmAuditFilterOptions, StorageError> {
        let session_keys = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT session_key
             FROM llm_audit
             WHERE (?1 IS NULL OR requested_at_ms >= ?1)
               AND (?2 IS NULL OR requested_at_ms <= ?2)
             ORDER BY session_key ASC",
        )
        .bind(query.requested_from_ms)
        .bind(query.requested_to_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let providers = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT provider
             FROM llm_audit
             WHERE (?1 IS NULL OR requested_at_ms >= ?1)
               AND (?2 IS NULL OR requested_at_ms <= ?2)
             ORDER BY provider ASC",
        )
        .bind(query.requested_from_ms)
        .bind(query.requested_to_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(LlmAuditFilterOptions {
            session_keys,
            providers,
        })
    }

    async fn append_webhook_event(
        &self,
        input: &NewWebhookEventRecord,
    ) -> Result<WebhookEventRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO webhook_events (
                id, source, event_type, session_key, chat_id, sender_id, content,
                payload_json, metadata_json, status, error_message, response_summary,
                received_at_ms, processed_at_ms, remote_addr, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        )
        .bind(&input.id)
        .bind(&input.source)
        .bind(&input.event_type)
        .bind(&input.session_key)
        .bind(&input.chat_id)
        .bind(&input.sender_id)
        .bind(&input.content)
        .bind(&input.payload_json)
        .bind(&input.metadata_json)
        .bind(input.status.as_str())
        .bind(&input.error_message)
        .bind(&input.response_summary)
        .bind(input.received_at_ms)
        .bind(input.processed_at_ms)
        .bind(&input.remote_addr)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, WebhookEventRow>(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        WebhookEventRecord::try_from(row)
    }

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, StorageError> {
        sqlx::query(
            "UPDATE webhook_events
             SET status = ?2, error_message = ?3, response_summary = ?4, processed_at_ms = ?5
             WHERE id = ?1",
        )
        .bind(event_id)
        .bind(update.status.as_str())
        .bind(&update.error_message)
        .bind(&update.response_summary)
        .bind(update.processed_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, WebhookEventRow>(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             WHERE id = ?1",
        )
        .bind(event_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        WebhookEventRecord::try_from(row)
    }

    async fn list_webhook_events(
        &self,
        query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, StorageError> {
        let sort_order = match query.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "received_at_ms ASC, created_at_ms ASC",
            WebhookEventSortOrder::ReceivedAtDesc => "received_at_ms DESC, created_at_ms DESC",
        };
        let rows = sqlx::query_as::<_, WebhookEventRow>(&format!(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             WHERE (?1 IS NULL OR source = ?1)
               AND (?2 IS NULL OR event_type = ?2)
               AND (?3 IS NULL OR session_key = ?3)
               AND (?4 IS NULL OR status = ?4)
               AND (?5 IS NULL OR received_at_ms >= ?5)
               AND (?6 IS NULL OR received_at_ms <= ?6)
             ORDER BY {sort_order}
             LIMIT ?7 OFFSET ?8"
        ))
        .bind(query.source.as_deref())
        .bind(query.event_type.as_deref())
        .bind(query.session_key.as_deref())
        .bind(query.status.map(|status| status.as_str()))
        .bind(query.received_from_ms)
        .bind(query.received_to_ms)
        .bind(query.limit.max(1))
        .bind(query.offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(WebhookEventRecord::try_from).collect()
    }

    async fn create_approval(
        &self,
        input: &NewApprovalRecord,
    ) -> Result<ApprovalRecord, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO approvals (
                id, session_key, tool_name, command_hash, command_preview, command_text, risk_level, status,
                requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12, ?13, NULL)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.tool_name)
        .bind(&input.command_hash)
        .bind(&input.command_preview)
        .bind(&input.command_text)
        .bind(&input.risk_level)
        .bind(ApprovalStatus::Pending.as_str())
        .bind(&input.requested_by)
        .bind(&input.justification)
        .bind(input.expires_at_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_approval(&input.id).await
    }

    async fn get_approval(&self, approval_id: &str) -> Result<ApprovalRecord, StorageError> {
        let row = sqlx::query_as::<_, ApprovalRow>(
            "SELECT id, session_key, tool_name, command_hash, command_preview, command_text, risk_level, status,
                    requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
             FROM approvals
             WHERE id = ?1",
        )
        .bind(approval_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        ApprovalRecord::try_from(row)
    }

    async fn update_approval_status(
        &self,
        approval_id: &str,
        status: ApprovalStatus,
        approved_by: Option<&str>,
    ) -> Result<ApprovalRecord, StorageError> {
        let now = now_ms();
        let updated = sqlx::query(
            "UPDATE approvals
             SET status = ?1,
                 approved_by = ?2,
                 updated_at_ms = ?3
             WHERE id = ?4",
        )
        .bind(status.as_str())
        .bind(approved_by)
        .bind(now)
        .bind(approval_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        if updated.rows_affected() == 0 {
            return Err(StorageError::backend(format!(
                "approval '{approval_id}' not found when setting status"
            )));
        }
        self.get_approval(approval_id).await
    }

    async fn consume_approved_shell_command(
        &self,
        approval_id: &str,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let updated = sqlx::query(
            "UPDATE approvals
             SET status = ?1,
                 consumed_at_ms = ?2,
                 updated_at_ms = ?2
             WHERE id = ?3
               AND session_key = ?4
               AND tool_name = 'shell'
               AND command_hash = ?5
               AND status = ?6
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= ?2",
        )
        .bind(ApprovalStatus::Consumed.as_str())
        .bind(now_ms)
        .bind(approval_id)
        .bind(session_key)
        .bind(command_hash)
        .bind(ApprovalStatus::Approved.as_str())
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(updated.rows_affected() > 0)
    }

    async fn consume_latest_approved_shell_command(
        &self,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let approval_id = sqlx::query_scalar::<_, String>(
            "SELECT id
             FROM approvals
             WHERE session_key = ?1
               AND tool_name = 'shell'
               AND command_hash = ?2
               AND status = ?3
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= ?4
             ORDER BY created_at_ms DESC
             LIMIT 1",
        )
        .bind(session_key)
        .bind(command_hash)
        .bind(ApprovalStatus::Approved.as_str())
        .bind(now_ms)
        .fetch_optional(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        let Some(approval_id) = approval_id else {
            return Ok(false);
        };
        self.consume_approved_shell_command(&approval_id, session_key, command_hash, now_ms)
            .await
    }

    fn session_jsonl_path(&self, session_key: &str) -> PathBuf {
        jsonl::session_jsonl_path(&self.paths, session_key)
    }
}

#[async_trait]
impl CronStorage for SqlxSessionStore {
    async fn create_cron(&self, input: &NewCronJob) -> Result<CronJob, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO cron (
                id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9, ?10)",
        )
        .bind(&input.id)
        .bind(&input.name)
        .bind(input.schedule_kind.as_str())
        .bind(&input.schedule_expr)
        .bind(&input.payload_json)
        .bind(if input.enabled { 1_i64 } else { 0_i64 })
        .bind(&input.timezone)
        .bind(input.next_run_at_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_cron(&input.id).await
    }

    async fn update_cron(
        &self,
        cron_id: &str,
        patch: &UpdateCronJobPatch,
    ) -> Result<CronJob, StorageError> {
        let current = self.get_cron(cron_id).await?;
        let now = now_ms();
        sqlx::query(
            "UPDATE cron
             SET name = ?1,
                 schedule_kind = ?2,
                 schedule_expr = ?3,
                 payload_json = ?4,
                 timezone = ?5,
                 next_run_at_ms = ?6,
                 updated_at_ms = ?7
             WHERE id = ?8",
        )
        .bind(patch.name.as_ref().unwrap_or(&current.name))
        .bind(
            patch
                .schedule_kind
                .unwrap_or(current.schedule_kind)
                .as_str(),
        )
        .bind(
            patch
                .schedule_expr
                .as_ref()
                .unwrap_or(&current.schedule_expr),
        )
        .bind(patch.payload_json.as_ref().unwrap_or(&current.payload_json))
        .bind(patch.timezone.as_ref().unwrap_or(&current.timezone))
        .bind(patch.next_run_at_ms.unwrap_or(current.next_run_at_ms))
        .bind(now)
        .bind(cron_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_cron(cron_id).await
    }

    async fn set_enabled(&self, cron_id: &str, enabled: bool) -> Result<(), StorageError> {
        sqlx::query("UPDATE cron SET enabled = ?1, updated_at_ms = ?2 WHERE id = ?3")
            .bind(if enabled { 1_i64 } else { 0_i64 })
            .bind(now_ms())
            .bind(cron_id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn delete_cron(&self, cron_id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM cron WHERE id = ?1")
            .bind(cron_id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn get_cron(&self, cron_id: &str) -> Result<CronJob, StorageError> {
        let row = sqlx::query_as::<_, CronJobRow>(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE id = ?1",
        )
        .bind(cron_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        row.try_into()
    }

    async fn list_crons(&self, limit: i64, offset: i64) -> Result<Vec<CronJob>, StorageError> {
        let rows = sqlx::query_as::<_, CronJobRow>(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             ORDER BY updated_at_ms DESC
             LIMIT ?1 OFFSET ?2",
        )
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_due_crons(&self, now_ms: i64, limit: i64) -> Result<Vec<CronJob>, StorageError> {
        let rows = sqlx::query_as::<_, CronJobRow>(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE enabled = 1 AND next_run_at_ms <= ?1
             ORDER BY next_run_at_ms ASC
             LIMIT ?2",
        )
        .bind(now_ms)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn claim_next_run(
        &self,
        cron_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            "UPDATE cron
             SET next_run_at_ms = ?1,
                 last_run_at_ms = ?2,
                 updated_at_ms = ?3
             WHERE id = ?4 AND enabled = 1 AND next_run_at_ms = ?5",
        )
        .bind(new_next_run_at_ms)
        .bind(expected_next_run_at_ms)
        .bind(now_ms)
        .bind(cron_id)
        .bind(expected_next_run_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn append_task_run(&self, input: &NewCronTaskRun) -> Result<CronTaskRun, StorageError> {
        sqlx::query(
            "INSERT INTO cron_task (
                id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms,
                status, attempt, error_message, published_message_id, created_at_ms
            ) VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL, NULL, ?6)",
        )
        .bind(&input.id)
        .bind(&input.cron_id)
        .bind(input.scheduled_at_ms)
        .bind(input.status.as_str())
        .bind(input.attempt)
        .bind(input.created_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, CronTaskRunRow>(
            "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM cron_task
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        row.try_into()
    }

    async fn mark_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE cron_task
             SET status = ?1, started_at_ms = ?2
             WHERE id = ?3",
        )
        .bind(CronTaskStatus::Running.as_str())
        .bind(started_at_ms)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn mark_task_result(
        &self,
        run_id: &str,
        status: CronTaskStatus,
        finished_at_ms: i64,
        error_message: Option<&str>,
        published_message_id: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE cron_task
             SET status = ?1,
                 finished_at_ms = ?2,
                 error_message = ?3,
                 published_message_id = ?4
             WHERE id = ?5",
        )
        .bind(status.as_str())
        .bind(finished_at_ms)
        .bind(error_message)
        .bind(published_message_id)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn list_task_runs(
        &self,
        cron_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CronTaskRun>, StorageError> {
        let rows = sqlx::query_as::<_, CronTaskRunRow>(
            "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM cron_task
             WHERE cron_id = ?1
             ORDER BY created_at_ms DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind(cron_id)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}

#[async_trait]
impl HeartbeatStorage for SqlxSessionStore {
    async fn create_heartbeat(
        &self,
        input: &NewHeartbeatJob,
    ) -> Result<HeartbeatJob, StorageError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO heartbeat (
                id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12)",
        )
        .bind(&input.id)
        .bind(&input.session_key)
        .bind(&input.channel)
        .bind(&input.chat_id)
        .bind(if input.enabled { 1_i64 } else { 0_i64 })
        .bind(&input.every)
        .bind(&input.prompt)
        .bind(&input.silent_ack_token)
        .bind(&input.timezone)
        .bind(input.next_run_at_ms)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_heartbeat(&input.id).await
    }

    async fn update_heartbeat(
        &self,
        heartbeat_id: &str,
        patch: &UpdateHeartbeatJobPatch,
    ) -> Result<HeartbeatJob, StorageError> {
        let current = self.get_heartbeat(heartbeat_id).await?;
        sqlx::query(
            "UPDATE heartbeat
             SET session_key = ?1,
                 channel = ?2,
                 chat_id = ?3,
                 every = ?4,
                 prompt = ?5,
                 silent_ack_token = ?6,
                 timezone = ?7,
                 next_run_at_ms = ?8,
                 updated_at_ms = ?9
             WHERE id = ?10",
        )
        .bind(patch.session_key.as_ref().unwrap_or(&current.session_key))
        .bind(patch.channel.as_ref().unwrap_or(&current.channel))
        .bind(patch.chat_id.as_ref().unwrap_or(&current.chat_id))
        .bind(patch.every.as_ref().unwrap_or(&current.every))
        .bind(patch.prompt.as_ref().unwrap_or(&current.prompt))
        .bind(
            patch
                .silent_ack_token
                .as_ref()
                .unwrap_or(&current.silent_ack_token),
        )
        .bind(patch.timezone.as_ref().unwrap_or(&current.timezone))
        .bind(patch.next_run_at_ms.unwrap_or(current.next_run_at_ms))
        .bind(now_ms())
        .bind(heartbeat_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.get_heartbeat(heartbeat_id).await
    }

    async fn set_heartbeat_enabled(
        &self,
        heartbeat_id: &str,
        enabled: bool,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE heartbeat
             SET enabled = ?1, updated_at_ms = ?2
             WHERE id = ?3",
        )
        .bind(if enabled { 1_i64 } else { 0_i64 })
        .bind(now_ms())
        .bind(heartbeat_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn delete_heartbeat(&self, heartbeat_id: &str) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM heartbeat WHERE id = ?1")
            .bind(heartbeat_id)
            .execute(&self.pool)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn get_heartbeat(&self, heartbeat_id: &str) -> Result<HeartbeatJob, StorageError> {
        let row = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE id = ?1",
        )
        .bind(heartbeat_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn get_heartbeat_by_session_key(
        &self,
        session_key: &str,
    ) -> Result<HeartbeatJob, StorageError> {
        let row = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn list_heartbeats(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let rows = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             ORDER BY updated_at_ms DESC
             LIMIT ?1 OFFSET ?2",
        )
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_due_heartbeats(
        &self,
        now_ms: i64,
        limit: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let rows = sqlx::query_as::<_, HeartbeatJobRow>(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE enabled = 1 AND next_run_at_ms <= ?1
             ORDER BY next_run_at_ms ASC
             LIMIT ?2",
        )
        .bind(now_ms)
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn claim_next_heartbeat_run(
        &self,
        heartbeat_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            "UPDATE heartbeat
             SET next_run_at_ms = ?1,
                 last_run_at_ms = ?2,
                 updated_at_ms = ?3
             WHERE id = ?4 AND enabled = 1 AND next_run_at_ms = ?5",
        )
        .bind(new_next_run_at_ms)
        .bind(expected_next_run_at_ms)
        .bind(now_ms)
        .bind(heartbeat_id)
        .bind(expected_next_run_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(result.rows_affected() == 1)
    }

    async fn append_heartbeat_task_run(
        &self,
        input: &NewHeartbeatTaskRun,
    ) -> Result<HeartbeatTaskRun, StorageError> {
        sqlx::query(
            "INSERT INTO heartbeat_task (
                id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                attempt, error_message, published_message_id, created_at_ms
            ) VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL, NULL, ?6)",
        )
        .bind(&input.id)
        .bind(&input.heartbeat_id)
        .bind(input.scheduled_at_ms)
        .bind(input.status.as_str())
        .bind(input.attempt)
        .bind(input.created_at_ms)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;

        let row = sqlx::query_as::<_, HeartbeatTaskRunRow>(
            "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM heartbeat_task
             WHERE id = ?1",
        )
        .bind(&input.id)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        row.try_into()
    }

    async fn mark_heartbeat_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE heartbeat_task
             SET status = ?1, started_at_ms = ?2
             WHERE id = ?3",
        )
        .bind(HeartbeatTaskStatus::Running.as_str())
        .bind(started_at_ms)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn mark_heartbeat_task_result(
        &self,
        run_id: &str,
        status: HeartbeatTaskStatus,
        finished_at_ms: i64,
        error_message: Option<&str>,
        published_message_id: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE heartbeat_task
             SET status = ?1,
                 finished_at_ms = ?2,
                 error_message = ?3,
                 published_message_id = ?4
             WHERE id = ?5",
        )
        .bind(status.as_str())
        .bind(finished_at_ms)
        .bind(error_message)
        .bind(published_message_id)
        .bind(run_id)
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn list_heartbeat_task_runs(
        &self,
        heartbeat_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatTaskRun>, StorageError> {
        let rows = sqlx::query_as::<_, HeartbeatTaskRunRow>(
            "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM heartbeat_task
             WHERE heartbeat_id = ?1
             ORDER BY created_at_ms DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind(heartbeat_id)
        .bind(limit.max(1))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}

fn bind_db_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    value: DbValue,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    match value {
        DbValue::Null => query.bind(Option::<String>::None),
        DbValue::Integer(v) => query.bind(v),
        DbValue::Real(v) => query.bind(v),
        DbValue::Text(v) => query.bind(v),
        DbValue::Blob(v) => query.bind(v),
    }
}

fn sqlx_row_value(row: &sqlx::sqlite::SqliteRow, index: usize) -> Result<DbValue, StorageError> {
    let type_name = row
        .columns()
        .get(index)
        .map(|col| col.type_info().name().to_ascii_uppercase())
        .unwrap_or_default();

    if type_name.contains("BLOB") {
        if let Ok(v) = row.try_get::<Vec<u8>, _>(index) {
            return Ok(DbValue::Blob(v));
        }
    }
    if type_name.contains("INT") {
        if let Ok(v) = row.try_get::<i64, _>(index) {
            return Ok(DbValue::Integer(v));
        }
    }
    if type_name.contains("REAL") || type_name.contains("FLOA") || type_name.contains("DOUB") {
        if let Ok(v) = row.try_get::<f64, _>(index) {
            return Ok(DbValue::Real(v));
        }
    }

    if let Ok(v) = row.try_get::<String, _>(index) {
        return Ok(DbValue::Text(v));
    }
    if let Ok(v) = row.try_get::<i64, _>(index) {
        return Ok(DbValue::Integer(v));
    }
    if let Ok(v) = row.try_get::<f64, _>(index) {
        return Ok(DbValue::Real(v));
    }
    if let Ok(v) = row.try_get::<Vec<u8>, _>(index) {
        return Ok(DbValue::Blob(v));
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(index) {
        return Ok(v.map(DbValue::Text).unwrap_or(DbValue::Null));
    }

    Err(StorageError::backend(format!(
        "unsupported sqlx value at column index {index}"
    )))
}
