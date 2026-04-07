use crate::{
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronScheduleKind, CronStorage,
    CronTaskRun, CronTaskStatus, HeartbeatJob, HeartbeatStorage, HeartbeatTaskRun,
    HeartbeatTaskStatus, LlmAuditFilterOptions, LlmAuditFilterOptionsQuery, LlmAuditQuery,
    LlmAuditRecord, LlmAuditSortOrder, LlmAuditStatus, LlmUsageRecord, LlmUsageSource,
    LlmUsageSummary, NewApprovalRecord, NewCronJob, NewCronTaskRun, NewHeartbeatJob,
    NewHeartbeatTaskRun, NewLlmAuditRecord, NewLlmUsageRecord, NewPendingQuestionRecord,
    NewToolAuditRecord, NewWebhookAgentRecord, NewWebhookEventRecord, PendingQuestionRecord,
    PendingQuestionStatus, SessionCompressionState, SessionIndex, SessionSortOrder,
    SessionStorage, StorageError, StoragePaths, ToolAuditFilterOptions,
    ToolAuditFilterOptionsQuery, ToolAuditQuery, ToolAuditRecord, ToolAuditStatus,
    UpdateCronJobPatch, UpdateHeartbeatJobPatch, UpdateWebhookAgentResult,
    UpdateWebhookEventResult, WebhookAgentQuery, WebhookAgentRecord, WebhookEventQuery,
    WebhookEventRecord, WebhookEventSortOrder, WebhookEventStatus, jsonl,
    memory_db::{DbRow, DbValue, MemoryDb},
    util::{now_ms, relative_or_absolute_jsonl},
};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use turso::{Builder, Connection, Database, Row, value::Value};

#[derive(Debug, Clone)]
pub struct TursoSessionStore {
    paths: StoragePaths,
    _db: Database,
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct TursoMemoryDb {
    _db: Database,
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct TursoArchiveDb {
    _db: Database,
    conn: Connection,
}

impl TursoSessionStore {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let db = Builder::new_local(&paths.db_path.to_string_lossy())
            .build()
            .await
            .map_err(StorageError::backend)?;
        let conn = db.connect().map_err(StorageError::backend)?;
        apply_sqlite_journal_mode(&conn).await?;
        apply_sqlite_connection_pragmas(&conn).await?;
        let store = Self {
            paths,
            _db: db,
            conn: Arc::new(Mutex::new(conn)),
        };
        store.init().await?;
        Ok(store)
    }

    async fn connection(&self) -> Result<tokio::sync::MutexGuard<'_, Connection>, StorageError> {
        Ok(self.conn.lock().await)
    }

    async fn init(&self) -> Result<(), StorageError> {
        {
            let conn = self.connection().await?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                    session_key TEXT PRIMARY KEY,
                    chat_id TEXT NOT NULL,
                    channel TEXT NOT NULL,
                    active_session_key TEXT,
                    model_provider TEXT,
                    model_provider_explicit INTEGER NOT NULL DEFAULT 0,
                    model TEXT,
                    model_explicit INTEGER NOT NULL DEFAULT 0,
                    delivery_metadata_json TEXT,
                    compression_last_len INTEGER NOT NULL DEFAULT 0,
                    compression_summary_json TEXT,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    last_message_at_ms INTEGER NOT NULL,
                    turn_count INTEGER NOT NULL DEFAULT 0,
                    jsonl_path TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_sessions_updated_at_ms
                ON sessions(updated_at_ms DESC);
                CREATE TABLE IF NOT EXISTS llm_usage (
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
                );
                CREATE INDEX IF NOT EXISTS idx_llm_usage_session_created
                ON llm_usage(session_key, created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_llm_usage_chat_created
                ON llm_usage(chat_id, created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_llm_usage_session_turn
                ON llm_usage(session_key, turn_index, request_seq);
                CREATE TABLE IF NOT EXISTS llm_audit (
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
                    metadata_json TEXT,
                    requested_at_ms INTEGER NOT NULL,
                    responded_at_ms INTEGER,
                    created_at_ms INTEGER NOT NULL,
                    FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_llm_audit_session_requested
                ON llm_audit(session_key, requested_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_llm_audit_provider_requested
                ON llm_audit(provider, requested_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_llm_audit_requested
                ON llm_audit(requested_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_llm_audit_session_turn
                ON llm_audit(session_key, turn_index, request_seq);
                CREATE TABLE IF NOT EXISTS tool_audit (
                    id TEXT PRIMARY KEY,
                    session_key TEXT NOT NULL,
                    chat_id TEXT NOT NULL,
                    turn_index INTEGER NOT NULL,
                    request_seq INTEGER NOT NULL,
                    tool_call_seq INTEGER NOT NULL,
                    tool_name TEXT NOT NULL,
                    status TEXT NOT NULL,
                    error_code TEXT,
                    error_message TEXT,
                    retryable INTEGER,
                    approval_required INTEGER NOT NULL,
                    arguments_json TEXT NOT NULL,
                    result_content TEXT NOT NULL,
                    error_details_json TEXT,
                    signals_json TEXT,
                    metadata_json TEXT,
                    started_at_ms INTEGER NOT NULL,
                    finished_at_ms INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_tool_audit_tool_started
                ON tool_audit(tool_name, started_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_tool_audit_session_started
                ON tool_audit(session_key, started_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_tool_audit_session_turn
                ON tool_audit(session_key, turn_index, request_seq, tool_call_seq);
                CREATE TABLE IF NOT EXISTS webhook_events (
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
                );
                CREATE INDEX IF NOT EXISTS idx_webhook_events_received
                ON webhook_events(received_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_webhook_events_source_received
                ON webhook_events(source, received_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_webhook_events_status_received
                ON webhook_events(status, received_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_webhook_events_session_received
                ON webhook_events(session_key, received_at_ms DESC);
                CREATE TABLE IF NOT EXISTS webhook_agents (
                    id TEXT PRIMARY KEY,
                    hook_id TEXT NOT NULL,
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
                );
                CREATE INDEX IF NOT EXISTS idx_webhook_agents_received
                ON webhook_agents(received_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_webhook_agents_hook_received
                ON webhook_agents(hook_id, received_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_webhook_agents_status_received
                ON webhook_agents(status, received_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_webhook_agents_session_received
                ON webhook_agents(session_key, received_at_ms DESC);
                CREATE TABLE IF NOT EXISTS cron (
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
                );
                CREATE TABLE IF NOT EXISTS cron_task (
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
                );
                CREATE INDEX IF NOT EXISTS idx_cron_enabled_next_run
                ON cron(enabled, next_run_at_ms);
                CREATE INDEX IF NOT EXISTS idx_cron_task_cron_created
                ON cron_task(cron_id, created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_cron_task_status_scheduled
                ON cron_task(status, scheduled_at_ms);
                CREATE TABLE IF NOT EXISTS heartbeat (
                    id TEXT PRIMARY KEY,
                    session_key TEXT NOT NULL UNIQUE,
                    channel TEXT NOT NULL,
                    chat_id TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    every TEXT NOT NULL,
                    prompt TEXT NOT NULL,
                    silent_ack_token TEXT NOT NULL,
                    recent_messages_limit INTEGER NOT NULL DEFAULT 12,
                    timezone TEXT NOT NULL DEFAULT 'UTC',
                    next_run_at_ms INTEGER NOT NULL,
                    last_run_at_ms INTEGER,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS heartbeat_task (
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
                );
                CREATE INDEX IF NOT EXISTS idx_heartbeat_enabled_next_run
                ON heartbeat(enabled, next_run_at_ms);
                CREATE INDEX IF NOT EXISTS idx_heartbeat_task_heartbeat_created
                ON heartbeat_task(heartbeat_id, created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_heartbeat_task_status_scheduled
                ON heartbeat_task(status, scheduled_at_ms);
                CREATE TABLE IF NOT EXISTS approvals (
                    id TEXT PRIMARY KEY,
                    session_key TEXT NOT NULL,
                    tool_name TEXT NOT NULL,
                    command_hash TEXT NOT NULL,
                    command_preview TEXT NOT NULL,
                    command_text TEXT NOT NULL DEFAULT '',
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
                );
                CREATE INDEX IF NOT EXISTS idx_approvals_session_status
                ON approvals(session_key, status, created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_approvals_expiry
                ON approvals(status, expires_at_ms);
                CREATE TABLE IF NOT EXISTS pending_questions (
                    id TEXT PRIMARY KEY,
                    session_key TEXT NOT NULL,
                    channel TEXT NOT NULL,
                    chat_id TEXT NOT NULL,
                    title TEXT,
                    question_text TEXT NOT NULL,
                    options_json TEXT NOT NULL,
                    status TEXT NOT NULL,
                    selected_option_id TEXT,
                    answered_by TEXT,
                    expires_at_ms INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    answered_at_ms INTEGER,
                    FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_pending_questions_session_status
                ON pending_questions(session_key, status, created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_pending_questions_expiry
                ON pending_questions(status, expires_at_ms);",
            )
            .await
            .map_err(StorageError::backend)?;
        }
        self.ensure_session_column("active_session_key", "TEXT")
            .await?;
        self.ensure_session_column("model_provider", "TEXT").await?;
        self.ensure_session_column("model_provider_explicit", "INTEGER NOT NULL DEFAULT 0")
            .await?;
        self.ensure_session_column("model", "TEXT").await?;
        self.ensure_session_column("model_explicit", "INTEGER NOT NULL DEFAULT 0")
            .await?;
        self.ensure_session_column("delivery_metadata_json", "TEXT")
            .await?;
        self.ensure_llm_audit_column("metadata_json", "TEXT")
            .await?;
        self.ensure_session_column("compression_last_len", "INTEGER NOT NULL DEFAULT 0")
            .await?;
        self.ensure_session_column("compression_summary_json", "TEXT")
            .await?;
        self.ensure_heartbeat_column("recent_messages_limit", "INTEGER NOT NULL DEFAULT 12")
            .await?;
        self.ensure_approval_column("command_text", "TEXT NOT NULL DEFAULT ''")
            .await?;
        Ok(())
    }

    async fn ensure_session_column(
        &self,
        column: &str,
        column_type: &str,
    ) -> Result<(), StorageError> {
        let sql = format!("ALTER TABLE sessions ADD COLUMN {column} {column_type}");
        let conn = self.connection().await?;
        let result = conn.execute(&sql, ()).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name")
                    || message.contains("already exists")
                    || message.contains("duplicate")
                {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure sessions.{column} column: {message}"
                )))
            }
        }
    }

    async fn ensure_approval_column(
        &self,
        column: &str,
        column_type: &str,
    ) -> Result<(), StorageError> {
        let sql = format!("ALTER TABLE approvals ADD COLUMN {column} {column_type}");
        let conn = self.connection().await?;
        let result = conn.execute(&sql, ()).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name")
                    || message.contains("already exists")
                    || message.contains("duplicate")
                    || message.contains("no such table")
                {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure approvals.{column} column: {message}"
                )))
            }
        }
    }

    async fn ensure_heartbeat_column(
        &self,
        column: &str,
        column_type: &str,
    ) -> Result<(), StorageError> {
        let sql = format!("ALTER TABLE heartbeat ADD COLUMN {column} {column_type}");
        let conn = self.connection().await?;
        let result = conn.execute(&sql, ()).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name")
                    || message.contains("already exists")
                    || message.contains("duplicate")
                    || message.contains("no such table")
                {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure heartbeat.{column} column: {message}"
                )))
            }
        }
    }

    async fn ensure_llm_audit_column(
        &self,
        column: &str,
        column_type: &str,
    ) -> Result<(), StorageError> {
        let sql = format!("ALTER TABLE llm_audit ADD COLUMN {column} {column_type}");
        let conn = self.connection().await?;
        let result = conn.execute(&sql, ()).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name")
                    || message.contains("already exists")
                    || message.contains("duplicate")
                    || message.contains("no such table")
                {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure llm_audit.{column} column: {message}"
                )))
            }
        }
    }
}

impl TursoMemoryDb {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let db = Builder::new_local(&paths.memory_db_path.to_string_lossy())
            .build()
            .await
            .map_err(StorageError::backend)?;
        let conn = db.connect().map_err(StorageError::backend)?;
        apply_sqlite_journal_mode(&conn).await?;
        apply_sqlite_connection_pragmas(&conn).await?;
        Ok(Self { _db: db, conn })
    }
}

impl TursoArchiveDb {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let db = Builder::new_local(&paths.archive_db_path.to_string_lossy())
            .build()
            .await
            .map_err(StorageError::backend)?;
        let conn = db.connect().map_err(StorageError::backend)?;
        apply_sqlite_journal_mode(&conn).await?;
        apply_sqlite_connection_pragmas(&conn).await?;
        Ok(Self { _db: db, conn })
    }
}

async fn apply_sqlite_journal_mode(conn: &Connection) -> Result<(), StorageError> {
    let mut rows = conn
        .query("PRAGMA journal_mode = WAL", ())
        .await
        .map_err(StorageError::backend)?;
    while rows.next().await.map_err(StorageError::backend)?.is_some() {}
    Ok(())
}

async fn apply_sqlite_connection_pragmas(conn: &Connection) -> Result<(), StorageError> {
    conn.execute("PRAGMA busy_timeout = 5000", ())
        .await
        .map_err(StorageError::backend)?;
    Ok(())
}

#[async_trait]
impl MemoryDb for TursoSessionStore {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        let conn = self.connection().await?;
        conn.execute_batch(sql).await.map_err(StorageError::backend)
    }

    async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, StorageError> {
        let turso_params = to_turso_params(params);
        let conn = self.connection().await?;
        conn.execute(sql, turso_params)
            .await
            .map_err(StorageError::backend)
    }

    async fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, StorageError> {
        let turso_params = to_turso_params(params);
        let conn = self.connection().await?;
        let mut rows = conn
            .query(sql, turso_params)
            .await
            .map_err(StorageError::backend)?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            let mut values = Vec::new();
            for idx in 0..row.column_count() {
                values.push(from_turso_value(
                    row.get_value(idx).map_err(StorageError::backend)?,
                ));
            }
            out.push(DbRow { values });
        }
        Ok(out)
    }
}

#[async_trait]
impl MemoryDb for TursoMemoryDb {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        self.conn
            .execute_batch(sql)
            .await
            .map_err(StorageError::backend)
    }

    async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, StorageError> {
        let turso_params = to_turso_params(params);
        self.conn
            .execute(sql, turso_params)
            .await
            .map_err(StorageError::backend)
    }

    async fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, StorageError> {
        let turso_params = to_turso_params(params);
        let mut rows = self
            .conn
            .query(sql, turso_params)
            .await
            .map_err(StorageError::backend)?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            let mut values = Vec::new();
            for idx in 0..row.column_count() {
                values.push(from_turso_value(
                    row.get_value(idx).map_err(StorageError::backend)?,
                ));
            }
            out.push(DbRow { values });
        }
        Ok(out)
    }
}

#[async_trait]
impl MemoryDb for TursoArchiveDb {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        self.conn
            .execute_batch(sql)
            .await
            .map_err(StorageError::backend)
    }

    async fn execute(&self, sql: &str, params: &[DbValue]) -> Result<u64, StorageError> {
        let turso_params = to_turso_params(params);
        self.conn
            .execute(sql, turso_params)
            .await
            .map_err(StorageError::backend)
    }

    async fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, StorageError> {
        let turso_params = to_turso_params(params);
        let mut rows = self
            .conn
            .query(sql, turso_params)
            .await
            .map_err(StorageError::backend)?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            let mut values = Vec::new();
            for idx in 0..row.column_count() {
                values.push(from_turso_value(
                    row.get_value(idx).map_err(StorageError::backend)?,
                ));
            }
            out.push(DbRow { values });
        }
        Ok(out)
    }
}

#[async_trait]
impl SessionStorage for TursoSessionStore {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        let sql = format!(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             ) VALUES ('{}', '{}', '{}', NULL, NULL, 0, NULL, 0, NULL, {}, {}, {}, 0, '{}')
             ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                last_message_at_ms=excluded.last_message_at_ms,
                jsonl_path=excluded.jsonl_path",
            escape_sql_text(session_key),
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now,
            now,
            now,
            escape_sql_text(&jsonl_path_str)
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
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
        let update_sql = format!(
            "UPDATE sessions
             SET
                chat_id = '{}',
                channel = '{}',
                updated_at_ms = {},
                last_message_at_ms = {},
                turn_count = turn_count + 1,
                jsonl_path = '{}'
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now,
            now,
            escape_sql_text(&jsonl_path_str),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&update_sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                let insert_sql = format!(
                    "INSERT INTO sessions (
                        session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
                    ) VALUES ('{}', '{}', '{}', NULL, NULL, 0, NULL, 0, NULL, {}, {}, {}, 1, '{}')",
                    escape_sql_text(session_key),
                    escape_sql_text(chat_id),
                    escape_sql_text(channel),
                    now,
                    now,
                    now,
                    escape_sql_text(&jsonl_path_str)
                );
                conn.execute(&insert_sql, ())
                    .await
                    .map_err(StorageError::backend)?;
            }
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
        let sql = format!(
            "SELECT session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE session_key = '{}'
             LIMIT 1",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("session not found"))?;
        row_to_session_index(&row)
    }

    async fn get_session_by_active_session_key(
        &self,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let sql = format!(
            "SELECT session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE active_session_key = '{}'
             ORDER BY CASE WHEN session_key = active_session_key THEN 1 ELSE 0 END, updated_at_ms DESC
             LIMIT 1",
            escape_sql_text(active_session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("session not found"))?;
        row_to_session_index(&row)
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
        let sql = format!(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             ) VALUES ('{}', '{}', '{}', '{}', NULL, 0, NULL, 0, NULL, {}, {}, {}, 0, '{}')
             ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                active_session_key=COALESCE(sessions.active_session_key, excluded.active_session_key),
                jsonl_path=excluded.jsonl_path",
            escape_sql_text(session_key),
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            escape_sql_text(session_key),
            now,
            now,
            now,
            escape_sql_text(&jsonl_path_str)
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_session(session_key).await
    }

    async fn set_active_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        active_session_key: &str,
    ) -> Result<SessionIndex, StorageError> {
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 active_session_key = '{}'
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(active_session_key),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting active_session_key"
                )));
            }
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
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model_provider = '{}',
                 model_provider_explicit = 1,
                 model = '{}',
                 model_explicit = 1
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(model_provider),
            escape_sql_text(model),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting model_provider"
                )));
            }
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
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model = '{}',
                 model_explicit = 0
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(model),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting model"
                )));
            }
        }
        self.get_session(session_key).await
    }

    async fn set_delivery_metadata(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
        delivery_metadata_json: Option<&str>,
    ) -> Result<SessionIndex, StorageError> {
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 delivery_metadata_json = {}
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            opt_sql_text(delivery_metadata_json),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when setting delivery_metadata"
                )));
            }
        }
        self.get_session(session_key).await
    }

    async fn clear_model_routing_override(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError> {
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model_provider = NULL,
                 model_provider_explicit = 0,
                 model = NULL,
                 model_explicit = 0
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(session_key)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "session '{session_key}' not found when clearing model routing override"
                )));
            }
        }
        self.get_session(session_key).await
    }

    async fn get_session_compression_state(
        &self,
        session_key: &str,
    ) -> Result<Option<SessionCompressionState>, StorageError> {
        let sql = format!(
            "SELECT compression_last_len, compression_summary_json
             FROM sessions
             WHERE session_key = '{}'",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let Some(row) = rows.next().await.map_err(StorageError::backend)? else {
            return Ok(None);
        };
        Ok(Some(SessionCompressionState {
            last_compressed_len: value_to_i64(row.get_value(0).map_err(StorageError::backend)?)?,
            summary_json: value_to_opt_string(row.get_value(1).map_err(StorageError::backend)?),
        }))
    }

    async fn set_session_compression_state(
        &self,
        session_key: &str,
        state: &SessionCompressionState,
    ) -> Result<(), StorageError> {
        let summary_sql = match state.summary_json.as_ref() {
            Some(value) => format!("'{}'", escape_sql_text(value)),
            None => "NULL".to_string(),
        };
        let sql = format!(
            "UPDATE sessions
             SET compression_last_len = {},
                 compression_summary_json = {},
                 updated_at_ms = {}
             WHERE session_key = '{}'",
            state.last_compressed_len,
            summary_sql,
            now_ms(),
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
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
        channel: Option<&str>,
        sort_order: SessionSortOrder,
    ) -> Result<Vec<SessionIndex>, StorageError> {
        let mut sql = String::from(
            "SELECT session_key, chat_id, channel, active_session_key, model_provider, model_provider_explicit, model, model_explicit, delivery_metadata_json, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions WHERE 1=1",
        );
        if let Some(from) = updated_from_ms {
            sql.push_str(&format!(" AND updated_at_ms >= {}", from));
        }
        if let Some(to) = updated_to_ms {
            sql.push_str(&format!(" AND updated_at_ms <= {}", to));
        }
        if let Some(channel) = channel {
            sql.push_str(&format!(" AND channel = '{}'", escape_sql_text(channel)));
        }
        sql.push_str(&format!(
            " ORDER BY {} LIMIT {} OFFSET {}",
            sort_order.sql_order_by(),
            limit.max(1),
            offset.max(0)
        ));
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_session_index(&row)?);
        }
        Ok(out)
    }

    async fn list_session_channels(&self) -> Result<Vec<String>, StorageError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT DISTINCT channel
                 FROM sessions
                 ORDER BY channel ASC",
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(value_to_string(
                row.get_value(0).map_err(StorageError::backend)?,
            )?);
        }
        Ok(out)
    }

    async fn append_llm_usage(
        &self,
        input: &NewLlmUsageRecord,
    ) -> Result<LlmUsageRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO llm_usage (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                source, provider_request_id, provider_response_id, created_at_ms
            ) VALUES ('{}', '{}', '{}', {}, {}, '{}', '{}', '{}', {}, {}, {}, {}, {}, '{}', {}, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            input.turn_index,
            input.request_seq,
            escape_sql_text(&input.provider),
            escape_sql_text(&input.model),
            escape_sql_text(&input.wire_api),
            input.input_tokens,
            input.output_tokens,
            input.total_tokens,
            opt_i64_sql(input.cached_input_tokens),
            opt_i64_sql(input.reasoning_tokens),
            input.source.as_str(),
            opt_string_sql(input.provider_request_id.as_deref()),
            opt_string_sql(input.provider_response_id.as_deref()),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let query_sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(&input.id)
        );
        let mut rows = conn
            .query(&query_sql, ())
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm usage not found"))?;
        row_to_llm_usage(&row)
    }

    async fn list_llm_usage(
        &self,
        session_key: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<LlmUsageRecord>, StorageError> {
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    input_tokens, output_tokens, total_tokens, cached_input_tokens, reasoning_tokens,
                    source, provider_request_id, provider_response_id, created_at_ms
             FROM llm_usage
             WHERE session_key = '{}'
             ORDER BY turn_index DESC, request_seq DESC, created_at_ms DESC
             LIMIT {} OFFSET {}",
            escape_sql_text(session_key),
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_llm_usage(&row)?);
        }
        Ok(out)
    }

    async fn sum_llm_usage_by_session(
        &self,
        session_key: &str,
    ) -> Result<LlmUsageSummary, StorageError> {
        let sql = format!(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = '{}'",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm usage summary not found"))?;
        row_to_llm_usage_summary(&row)
    }

    async fn sum_llm_usage_by_turn(
        &self,
        session_key: &str,
        turn_index: i64,
    ) -> Result<LlmUsageSummary, StorageError> {
        let sql = format!(
            "SELECT
                COUNT(*) as request_count,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COALESCE(SUM(total_tokens), 0) as total_tokens,
                COALESCE(SUM(cached_input_tokens), 0) as cached_input_tokens,
                COALESCE(SUM(reasoning_tokens), 0) as reasoning_tokens
             FROM llm_usage
             WHERE session_key = '{}' AND turn_index = {}",
            escape_sql_text(session_key),
            turn_index
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm usage summary not found"))?;
        row_to_llm_usage_summary(&row)
    }

    async fn append_llm_audit(
        &self,
        input: &NewLlmAuditRecord,
    ) -> Result<LlmAuditRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO llm_audit (
                id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                status, error_code, error_message, provider_request_id, provider_response_id,
                request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
            ) VALUES ('{}', '{}', '{}', {}, {}, '{}', '{}', '{}', '{}', {}, {}, {}, {}, '{}', {}, {}, {}, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            input.turn_index,
            input.request_seq,
            escape_sql_text(&input.provider),
            escape_sql_text(&input.model),
            escape_sql_text(&input.wire_api),
            input.status.as_str(),
            opt_string_sql(input.error_code.as_deref()),
            opt_string_sql(input.error_message.as_deref()),
            opt_string_sql(input.provider_request_id.as_deref()),
            opt_string_sql(input.provider_response_id.as_deref()),
            escape_sql_text(&input.request_body_json),
            opt_string_sql(input.response_body_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.requested_at_ms,
            opt_i64_sql(input.responded_at_ms),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let query_sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(&input.id)
        );
        let mut rows = conn
            .query(&query_sql, ())
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("llm audit not found"))?;
        row_to_llm_audit(&row)
    }

    async fn list_llm_audit(
        &self,
        query: &LlmAuditQuery,
    ) -> Result<Vec<LlmAuditRecord>, StorageError> {
        let sort_order = match query.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
            LlmAuditSortOrder::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
        };
        let mut conditions = Vec::new();
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(provider) = query
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("provider = '{}'", escape_sql_text(provider)));
        }
        if let Some(from_ms) = query.requested_from_ms {
            conditions.push(format!("requested_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.requested_to_ms {
            conditions.push(format!("requested_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, metadata_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_llm_audit(&row)?);
        }
        Ok(out)
    }

    async fn list_llm_audit_filter_options(
        &self,
        query: &LlmAuditFilterOptionsQuery,
    ) -> Result<LlmAuditFilterOptions, StorageError> {
        let where_clause =
            llm_audit_requested_range_where(query.requested_from_ms, query.requested_to_ms);
        let conn = self.connection().await?;
        let mut session_rows = conn
            .query(
                &format!(
                    "SELECT DISTINCT session_key
                     FROM llm_audit
                     {where_clause}
                     ORDER BY session_key ASC"
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let mut session_keys = Vec::new();
        while let Some(row) = session_rows.next().await.map_err(StorageError::backend)? {
            let value = row.get_value(0).map_err(StorageError::backend)?;
            session_keys.push(value_to_string(value)?);
        }

        let mut provider_rows = conn
            .query(
                &format!(
                    "SELECT DISTINCT provider
                     FROM llm_audit
                     {where_clause}
                     ORDER BY provider ASC"
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let mut providers = Vec::new();
        while let Some(row) = provider_rows.next().await.map_err(StorageError::backend)? {
            let value = row.get_value(0).map_err(StorageError::backend)?;
            providers.push(value_to_string(value)?);
        }

        Ok(LlmAuditFilterOptions {
            session_keys,
            providers,
        })
    }

    async fn append_tool_audit(
        &self,
        input: &NewToolAuditRecord,
    ) -> Result<ToolAuditRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO tool_audit (
                id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                status, error_code, error_message, retryable, approval_required, arguments_json,
                result_content, error_details_json, signals_json, metadata_json, started_at_ms,
                finished_at_ms, created_at_ms
            ) VALUES ('{}', '{}', '{}', {}, {}, {}, '{}', '{}', {}, {}, {}, {}, '{}', '{}', {}, {}, {}, {}, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            input.turn_index,
            input.request_seq,
            input.tool_call_seq,
            escape_sql_text(&input.tool_name),
            input.status.as_str(),
            opt_string_sql(input.error_code.as_deref()),
            opt_string_sql(input.error_message.as_deref()),
            opt_i64_sql(input.retryable.map(|flag| if flag { 1 } else { 0 })),
            if input.approval_required { 1 } else { 0 },
            escape_sql_text(&input.arguments_json),
            escape_sql_text(&input.result_content),
            opt_sql_text(input.error_details_json.as_deref()),
            opt_sql_text(input.signals_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.started_at_ms,
            input.finished_at_ms,
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let query_sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                    status, error_code, error_message, retryable, approval_required,
                    arguments_json, result_content, error_details_json, signals_json,
                    metadata_json, started_at_ms, finished_at_ms, created_at_ms
             FROM tool_audit
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(&input.id)
        );
        let mut rows = conn
            .query(&query_sql, ())
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("tool audit not found"))?;
        row_to_tool_audit(&row)
    }

    async fn list_tool_audit(
        &self,
        query: &ToolAuditQuery,
    ) -> Result<Vec<ToolAuditRecord>, StorageError> {
        let mut conditions = Vec::new();
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(tool_name) = query
            .tool_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            conditions.push(format!("tool_name = '{}'", escape_sql_text(tool_name)));
        }
        if let Some(from_ms) = query.started_from_ms {
            conditions.push(format!("started_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.started_to_ms {
            conditions.push(format!("started_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, tool_call_seq, tool_name,
                    status, error_code, error_message, retryable, approval_required,
                    arguments_json, result_content, error_details_json, signals_json,
                    metadata_json, started_at_ms, finished_at_ms, created_at_ms
             FROM tool_audit
             {where_clause}
             ORDER BY {}
             LIMIT {}
             OFFSET {}",
            query.sort_order.sql_order_by(),
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_tool_audit(&row)?);
        }
        Ok(out)
    }

    async fn list_tool_audit_filter_options(
        &self,
        query: &ToolAuditFilterOptionsQuery,
    ) -> Result<ToolAuditFilterOptions, StorageError> {
        let where_clause =
            tool_audit_started_range_where(query.started_from_ms, query.started_to_ms);
        let session_sql = format!(
            "SELECT DISTINCT session_key
             FROM tool_audit
             {where_clause}
             ORDER BY session_key ASC"
        );
        let tool_sql = format!(
            "SELECT DISTINCT tool_name
             FROM tool_audit
             {where_clause}
             ORDER BY tool_name ASC"
        );
        let conn = self.connection().await?;
        let session_keys = collect_string_column(&conn, &session_sql).await?;
        let tool_names = collect_string_column(&conn, &tool_sql).await?;
        Ok(ToolAuditFilterOptions {
            session_keys,
            tool_names,
        })
    }

    async fn append_webhook_event(
        &self,
        input: &NewWebhookEventRecord,
    ) -> Result<WebhookEventRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO webhook_events (
                id, source, event_type, session_key, chat_id, sender_id, content,
                payload_json, metadata_json, status, error_message, response_summary,
                received_at_ms, processed_at_ms, remote_addr, created_at_ms
            ) VALUES (
                '{}', '{}', '{}', '{}', '{}', '{}', '{}',
                {}, {}, '{}', {}, {}, {}, {}, {}, {}
            )",
            escape_sql_text(&input.id),
            escape_sql_text(&input.source),
            escape_sql_text(&input.event_type),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            escape_sql_text(&input.sender_id),
            escape_sql_text(&input.content),
            opt_sql_text(input.payload_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.status.as_str(),
            opt_sql_text(input.error_message.as_deref()),
            opt_sql_text(input.response_summary.as_deref()),
            input.received_at_ms,
            opt_sql_i64(input.processed_at_ms),
            opt_sql_text(input.remote_addr.as_deref()),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_events
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(&input.id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook event not found"))?;
        row_to_webhook_event(&row)
    }

    async fn update_webhook_event_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookEventResult,
    ) -> Result<WebhookEventRecord, StorageError> {
        let sql = format!(
            "UPDATE webhook_events
             SET status = '{}', error_message = {}, response_summary = {}, processed_at_ms = {}
             WHERE id = '{}'",
            update.status.as_str(),
            opt_sql_text(update.error_message.as_deref()),
            opt_sql_text(update.response_summary.as_deref()),
            opt_sql_i64(update.processed_at_ms),
            escape_sql_text(event_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_events
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(event_id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook event not found"))?;
        row_to_webhook_event(&row)
    }

    async fn list_webhook_events(
        &self,
        query: &WebhookEventQuery,
    ) -> Result<Vec<WebhookEventRecord>, StorageError> {
        let sort_order = match query.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "received_at_ms ASC, created_at_ms ASC",
            WebhookEventSortOrder::ReceivedAtDesc => "received_at_ms DESC, created_at_ms DESC",
        };
        let mut conditions = Vec::new();
        if let Some(source) = query
            .source
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("source = '{}'", escape_sql_text(source)));
        }
        if let Some(event_type) = query
            .event_type
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("event_type = '{}'", escape_sql_text(event_type)));
        }
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(status) = query.status {
            conditions.push(format!("status = '{}'", status.as_str()));
        }
        if let Some(from_ms) = query.received_from_ms {
            conditions.push(format!("received_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.received_to_ms {
            conditions.push(format!("received_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, source, event_type, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_events
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_webhook_event(&row)?);
        }
        Ok(out)
    }

    async fn append_webhook_agent(
        &self,
        input: &NewWebhookAgentRecord,
    ) -> Result<WebhookAgentRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO webhook_agents (
                id, hook_id, session_key, chat_id, sender_id, content,
                payload_json, metadata_json, status, error_message, response_summary,
                received_at_ms, processed_at_ms, remote_addr, created_at_ms
            ) VALUES (
                '{}', '{}', '{}', '{}', '{}', '{}',
                {}, {}, '{}', {}, {}, {}, {}, {}, {}
            )",
            escape_sql_text(&input.id),
            escape_sql_text(&input.hook_id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.chat_id),
            escape_sql_text(&input.sender_id),
            escape_sql_text(&input.content),
            opt_sql_text(input.payload_json.as_deref()),
            opt_sql_text(input.metadata_json.as_deref()),
            input.status.as_str(),
            opt_sql_text(input.error_message.as_deref()),
            opt_sql_text(input.response_summary.as_deref()),
            input.received_at_ms,
            opt_sql_i64(input.processed_at_ms),
            opt_sql_text(input.remote_addr.as_deref()),
            now
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_agents
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(&input.id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook agent not found"))?;
        row_to_webhook_agent(&row)
    }

    async fn update_webhook_agent_status(
        &self,
        event_id: &str,
        update: &UpdateWebhookAgentResult,
    ) -> Result<WebhookAgentRecord, StorageError> {
        let sql = format!(
            "UPDATE webhook_agents
             SET status = '{}', error_message = {}, response_summary = {}, processed_at_ms = {}
             WHERE id = '{}'",
            update.status.as_str(),
            opt_sql_text(update.error_message.as_deref()),
            opt_sql_text(update.response_summary.as_deref()),
            opt_sql_i64(update.processed_at_ms),
            escape_sql_text(event_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;

        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                            payload_json, metadata_json, status, error_message, response_summary,
                            received_at_ms, processed_at_ms, remote_addr, created_at_ms
                     FROM webhook_agents
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(event_id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("webhook agent not found"))?;
        row_to_webhook_agent(&row)
    }

    async fn list_webhook_agents(
        &self,
        query: &WebhookAgentQuery,
    ) -> Result<Vec<WebhookAgentRecord>, StorageError> {
        let sort_order = match query.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "received_at_ms ASC, created_at_ms ASC",
            WebhookEventSortOrder::ReceivedAtDesc => "received_at_ms DESC, created_at_ms DESC",
        };
        let mut conditions = Vec::new();
        if let Some(hook_id) = query
            .hook_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("hook_id = '{}'", escape_sql_text(hook_id)));
        }
        if let Some(session_key) = query
            .session_key
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
        }
        if let Some(status) = query.status {
            conditions.push(format!("status = '{}'", status.as_str()));
        }
        if let Some(from_ms) = query.received_from_ms {
            conditions.push(format!("received_at_ms >= {from_ms}"));
        }
        if let Some(to_ms) = query.received_to_ms {
            conditions.push(format!("received_at_ms <= {to_ms}"));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT id, hook_id, session_key, chat_id, sender_id, content,
                    payload_json, metadata_json, status, error_message, response_summary,
                    received_at_ms, processed_at_ms, remote_addr, created_at_ms
             FROM webhook_agents
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_webhook_agent(&row)?);
        }
        Ok(out)
    }

    async fn create_approval(
        &self,
        input: &NewApprovalRecord,
    ) -> Result<ApprovalRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO approvals (
                id, session_key, tool_name, command_hash, command_preview, command_text, risk_level, status,
                requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', NULL, {}, {}, {}, {}, NULL)",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.tool_name),
            escape_sql_text(&input.command_hash),
            escape_sql_text(&input.command_preview),
            escape_sql_text(&input.command_text),
            escape_sql_text(&input.risk_level),
            ApprovalStatus::Pending.as_str(),
            escape_sql_text(&input.requested_by),
            input.justification
                .as_deref()
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            input.expires_at_ms,
            now,
            now
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_approval(&input.id).await
    }

    async fn get_approval(&self, approval_id: &str) -> Result<ApprovalRecord, StorageError> {
        let sql = format!(
            "SELECT id, session_key, tool_name, command_hash, command_preview, risk_level, status,
                    command_text, requested_by, approved_by, justification, expires_at_ms, created_at_ms, updated_at_ms, consumed_at_ms
             FROM approvals
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(approval_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("approval not found"))?;
        row_to_approval(&row)
    }

    async fn update_approval_status(
        &self,
        approval_id: &str,
        status: ApprovalStatus,
        approved_by: Option<&str>,
    ) -> Result<ApprovalRecord, StorageError> {
        let sql = format!(
            "UPDATE approvals
             SET status = '{}',
                 approved_by = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            status.as_str(),
            approved_by
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            now_ms(),
            escape_sql_text(approval_id)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "approval '{approval_id}' not found when setting status"
                )));
            }
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
        let sql = format!(
            "UPDATE approvals
             SET status = '{}',
                 consumed_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'
               AND session_key = '{}'
               AND tool_name = 'shell'
               AND command_hash = '{}'
               AND status = '{}'
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= {}",
            ApprovalStatus::Consumed.as_str(),
            now_ms,
            now_ms,
            escape_sql_text(approval_id),
            escape_sql_text(session_key),
            escape_sql_text(command_hash),
            ApprovalStatus::Approved.as_str(),
            now_ms
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(affected > 0)
    }

    async fn consume_latest_approved_shell_command(
        &self,
        session_key: &str,
        command_hash: &str,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "SELECT id
             FROM approvals
             WHERE session_key = '{}'
               AND tool_name = 'shell'
               AND command_hash = '{}'
               AND status = '{}'
               AND consumed_at_ms IS NULL
               AND expires_at_ms >= {}
             ORDER BY created_at_ms DESC
             LIMIT 1",
            escape_sql_text(session_key),
            escape_sql_text(command_hash),
            ApprovalStatus::Approved.as_str(),
            now_ms
        );
        let approval_id = {
            let conn = self.connection().await?;
            let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
            let Some(row) = rows.next().await.map_err(StorageError::backend)? else {
                return Ok(false);
            };
            value_to_string(row.get_value(0).map_err(StorageError::backend)?)?
        };
        self.consume_approved_shell_command(&approval_id, session_key, command_hash, now_ms)
            .await
    }

    async fn create_pending_question(
        &self,
        input: &NewPendingQuestionRecord,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO pending_questions (
                id, session_key, channel, chat_id, title, question_text, options_json, status,
                selected_option_id, answered_by, expires_at_ms, created_at_ms, updated_at_ms, answered_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', {}, '{}', '{}', '{}', NULL, NULL, {}, {}, {}, NULL)",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.channel),
            escape_sql_text(&input.chat_id),
            input.title
                .as_deref()
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            escape_sql_text(&input.question_text),
            escape_sql_text(&input.options_json),
            PendingQuestionStatus::Pending.as_str(),
            input.expires_at_ms,
            now,
            now
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_pending_question(&input.id).await
    }

    async fn get_pending_question(
        &self,
        question_id: &str,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, title, question_text, options_json, status,
                    selected_option_id, answered_by, expires_at_ms, created_at_ms, updated_at_ms, answered_at_ms
             FROM pending_questions
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(question_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("pending question not found"))?;
        row_to_pending_question(&row)
    }

    async fn update_pending_question_answer(
        &self,
        question_id: &str,
        status: PendingQuestionStatus,
        selected_option_id: Option<&str>,
        answered_by: Option<&str>,
        answered_at_ms: Option<i64>,
    ) -> Result<PendingQuestionRecord, StorageError> {
        let sql = format!(
            "UPDATE pending_questions
             SET status = '{}',
                 selected_option_id = {},
                 answered_by = {},
                 answered_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            status.as_str(),
            selected_option_id
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            answered_by
                .map(|value| format!("'{}'", escape_sql_text(value)))
                .unwrap_or_else(|| "NULL".to_string()),
            answered_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "NULL".to_string()),
            now_ms(),
            escape_sql_text(question_id)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "pending question '{question_id}' not found when updating answer"
                )));
            }
        }
        self.get_pending_question(question_id).await
    }

    fn session_jsonl_path(&self, session_key: &str) -> PathBuf {
        jsonl::session_jsonl_path(&self.paths, session_key)
    }
}

#[async_trait]
impl CronStorage for TursoSessionStore {
    async fn create_cron(&self, input: &NewCronJob) -> Result<CronJob, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO cron (
                id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', '{}', {}, '{}', {}, NULL, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.name),
            input.schedule_kind.as_str(),
            escape_sql_text(&input.schedule_expr),
            escape_sql_text(&input.payload_json),
            if input.enabled { 1 } else { 0 },
            escape_sql_text(&input.timezone),
            input.next_run_at_ms,
            now,
            now
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_cron(&input.id).await
    }

    async fn update_cron(
        &self,
        cron_id: &str,
        patch: &UpdateCronJobPatch,
    ) -> Result<CronJob, StorageError> {
        let current = self.get_cron(cron_id).await?;
        let schedule_kind = patch
            .schedule_kind
            .unwrap_or(current.schedule_kind)
            .as_str();
        let sql = format!(
            "UPDATE cron
             SET name = '{}',
                 schedule_kind = '{}',
                 schedule_expr = '{}',
                 payload_json = '{}',
                 timezone = '{}',
                 next_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            escape_sql_text(patch.name.as_deref().unwrap_or(&current.name)),
            schedule_kind,
            escape_sql_text(
                patch
                    .schedule_expr
                    .as_deref()
                    .unwrap_or(&current.schedule_expr)
            ),
            escape_sql_text(
                patch
                    .payload_json
                    .as_deref()
                    .unwrap_or(&current.payload_json)
            ),
            escape_sql_text(patch.timezone.as_deref().unwrap_or(&current.timezone)),
            patch.next_run_at_ms.unwrap_or(current.next_run_at_ms),
            now_ms(),
            escape_sql_text(cron_id)
        );
        {
            let conn = self.connection().await?;
            let affected = conn
                .execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
            if affected == 0 {
                return Err(StorageError::backend(format!(
                    "cron job '{cron_id}' not found when updating"
                )));
            }
        }
        self.get_cron(cron_id).await
    }

    async fn set_enabled(&self, cron_id: &str, enabled: bool) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE cron
             SET enabled = {}, updated_at_ms = {}
             WHERE id = '{}'",
            if enabled { 1 } else { 0 },
            now_ms(),
            escape_sql_text(cron_id)
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
            return Err(StorageError::backend(format!(
                "cron job '{cron_id}' not found when setting enabled"
            )));
        }
        Ok(())
    }

    async fn delete_cron(&self, cron_id: &str) -> Result<(), StorageError> {
        let sql = format!("DELETE FROM cron WHERE id = '{}'", escape_sql_text(cron_id));
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
            return Err(StorageError::backend(format!(
                "cron job '{cron_id}' not found when deleting"
            )));
        }
        Ok(())
    }

    async fn get_cron(&self, cron_id: &str) -> Result<CronJob, StorageError> {
        let sql = format!(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(cron_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("cron job not found"))?;
        row_to_cron_job(&row)
    }

    async fn list_crons(&self, limit: i64, offset: i64) -> Result<Vec<CronJob>, StorageError> {
        let sql = format!(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             ORDER BY updated_at_ms DESC
             LIMIT {}
             OFFSET {}",
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_cron_job(&row)?);
        }
        Ok(out)
    }

    async fn list_due_crons(&self, now_ms: i64, limit: i64) -> Result<Vec<CronJob>, StorageError> {
        let sql = format!(
            "SELECT id, name, schedule_kind, schedule_expr, payload_json, enabled, timezone,
                    next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM cron
             WHERE enabled = 1 AND next_run_at_ms <= {}
             ORDER BY next_run_at_ms ASC
             LIMIT {}",
            now_ms,
            limit.max(1)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_cron_job(&row)?);
        }
        Ok(out)
    }

    async fn claim_next_run(
        &self,
        cron_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "UPDATE cron
             SET next_run_at_ms = {},
                 last_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}' AND enabled = 1 AND next_run_at_ms = {}",
            new_next_run_at_ms,
            expected_next_run_at_ms,
            now_ms,
            escape_sql_text(cron_id),
            expected_next_run_at_ms
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(affected == 1)
    }

    async fn append_task_run(&self, input: &NewCronTaskRun) -> Result<CronTaskRun, StorageError> {
        let sql = format!(
            "INSERT INTO cron_task (
                id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms,
                status, attempt, error_message, published_message_id, created_at_ms
            ) VALUES ('{}', '{}', {}, NULL, NULL, '{}', {}, NULL, NULL, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.cron_id),
            input.scheduled_at_ms,
            input.status.as_str(),
            input.attempt,
            input.created_at_ms
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                            attempt, error_message, published_message_id, created_at_ms
                     FROM cron_task
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(&input.id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("cron task not found"))?;
        row_to_cron_task_run(&row)
    }

    async fn mark_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE cron_task
             SET status = '{}', started_at_ms = {}
             WHERE id = '{}'",
            CronTaskStatus::Running.as_str(),
            started_at_ms,
            escape_sql_text(run_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
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
        let error_sql = error_message
            .map(|value| format!("'{}'", escape_sql_text(value)))
            .unwrap_or_else(|| "NULL".to_string());
        let publish_sql = published_message_id
            .map(|value| format!("'{}'", escape_sql_text(value)))
            .unwrap_or_else(|| "NULL".to_string());
        let sql = format!(
            "UPDATE cron_task
             SET status = '{}',
                 finished_at_ms = {},
                 error_message = {},
                 published_message_id = {}
             WHERE id = '{}'",
            status.as_str(),
            finished_at_ms,
            error_sql,
            publish_sql,
            escape_sql_text(run_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
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
        let sql = format!(
            "SELECT id, cron_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM cron_task
             WHERE cron_id = '{}'
             ORDER BY created_at_ms DESC
             LIMIT {} OFFSET {}",
            escape_sql_text(cron_id),
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_cron_task_run(&row)?);
        }
        Ok(out)
    }
}

#[async_trait]
impl HeartbeatStorage for TursoSessionStore {
    async fn create_heartbeat(
        &self,
        input: &NewHeartbeatJob,
    ) -> Result<HeartbeatJob, StorageError> {
        let now = now_ms();
        let sql = format!(
            "INSERT INTO heartbeat (
                id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
            ) VALUES ('{}', '{}', '{}', '{}', {}, '{}', '{}', '{}', {}, '{}', {}, NULL, {}, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.session_key),
            escape_sql_text(&input.channel),
            escape_sql_text(&input.chat_id),
            if input.enabled { 1 } else { 0 },
            escape_sql_text(&input.every),
            escape_sql_text(&input.prompt),
            escape_sql_text(&input.silent_ack_token),
            input.recent_messages_limit,
            escape_sql_text(&input.timezone),
            input.next_run_at_ms,
            now,
            now
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_heartbeat(&input.id).await
    }

    async fn update_heartbeat(
        &self,
        heartbeat_id: &str,
        patch: &UpdateHeartbeatJobPatch,
    ) -> Result<HeartbeatJob, StorageError> {
        let current = self.get_heartbeat(heartbeat_id).await?;
        let sql = format!(
            "UPDATE heartbeat
             SET session_key = '{}',
                 channel = '{}',
                 chat_id = '{}',
                 every = '{}',
                 prompt = '{}',
                 silent_ack_token = '{}',
                 recent_messages_limit = {},
                 timezone = '{}',
                 next_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}'",
            escape_sql_text(patch.session_key.as_deref().unwrap_or(&current.session_key)),
            escape_sql_text(patch.channel.as_deref().unwrap_or(&current.channel)),
            escape_sql_text(patch.chat_id.as_deref().unwrap_or(&current.chat_id)),
            escape_sql_text(patch.every.as_deref().unwrap_or(&current.every)),
            escape_sql_text(patch.prompt.as_deref().unwrap_or(&current.prompt)),
            escape_sql_text(
                patch
                    .silent_ack_token
                    .as_deref()
                    .unwrap_or(&current.silent_ack_token)
            ),
            patch
                .recent_messages_limit
                .unwrap_or(current.recent_messages_limit),
            escape_sql_text(patch.timezone.as_deref().unwrap_or(&current.timezone)),
            patch.next_run_at_ms.unwrap_or(current.next_run_at_ms),
            now_ms(),
            escape_sql_text(heartbeat_id)
        );
        {
            let conn = self.connection().await?;
            conn.execute(&sql, ())
                .await
                .map_err(StorageError::backend)?;
        }
        self.get_heartbeat(heartbeat_id).await
    }

    async fn set_heartbeat_enabled(
        &self,
        heartbeat_id: &str,
        enabled: bool,
    ) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE heartbeat
             SET enabled = {}, updated_at_ms = {}
             WHERE id = '{}'",
            if enabled { 1 } else { 0 },
            now_ms(),
            escape_sql_text(heartbeat_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn delete_heartbeat(&self, heartbeat_id: &str) -> Result<(), StorageError> {
        let sql = format!(
            "DELETE FROM heartbeat WHERE id = '{}'",
            escape_sql_text(heartbeat_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn get_heartbeat(&self, heartbeat_id: &str) -> Result<HeartbeatJob, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(heartbeat_id)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("heartbeat job not found"))?;
        row_to_heartbeat_job(&row)
    }

    async fn get_heartbeat_by_session_key(
        &self,
        session_key: &str,
    ) -> Result<HeartbeatJob, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE session_key = '{}'
             LIMIT 1",
            escape_sql_text(session_key)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("heartbeat job not found"))?;
        row_to_heartbeat_job(&row)
    }

    async fn list_heartbeats(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             ORDER BY updated_at_ms DESC
             LIMIT {}
             OFFSET {}",
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_heartbeat_job(&row)?);
        }
        Ok(out)
    }

    async fn list_due_heartbeats(
        &self,
        now_ms: i64,
        limit: i64,
    ) -> Result<Vec<HeartbeatJob>, StorageError> {
        let sql = format!(
            "SELECT id, session_key, channel, chat_id, enabled, every, prompt, silent_ack_token,
                    recent_messages_limit, timezone, next_run_at_ms, last_run_at_ms, created_at_ms, updated_at_ms
             FROM heartbeat
             WHERE enabled = 1 AND next_run_at_ms <= {}
             ORDER BY next_run_at_ms ASC
             LIMIT {}",
            now_ms,
            limit.max(1)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_heartbeat_job(&row)?);
        }
        Ok(out)
    }

    async fn claim_next_heartbeat_run(
        &self,
        heartbeat_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64,
    ) -> Result<bool, StorageError> {
        let sql = format!(
            "UPDATE heartbeat
             SET next_run_at_ms = {},
                 last_run_at_ms = {},
                 updated_at_ms = {}
             WHERE id = '{}' AND enabled = 1 AND next_run_at_ms = {}",
            new_next_run_at_ms,
            expected_next_run_at_ms,
            now_ms,
            escape_sql_text(heartbeat_id),
            expected_next_run_at_ms
        );
        let conn = self.connection().await?;
        let affected = conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(affected == 1)
    }

    async fn append_heartbeat_task_run(
        &self,
        input: &NewHeartbeatTaskRun,
    ) -> Result<HeartbeatTaskRun, StorageError> {
        let sql = format!(
            "INSERT INTO heartbeat_task (
                id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                attempt, error_message, published_message_id, created_at_ms
            ) VALUES ('{}', '{}', {}, NULL, NULL, '{}', {}, NULL, NULL, {})",
            escape_sql_text(&input.id),
            escape_sql_text(&input.heartbeat_id),
            input.scheduled_at_ms,
            input.status.as_str(),
            input.attempt,
            input.created_at_ms
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms,
                            status, attempt, error_message, published_message_id, created_at_ms
                     FROM heartbeat_task
                     WHERE id = '{}'
                     LIMIT 1",
                    escape_sql_text(&input.id)
                ),
                (),
            )
            .await
            .map_err(StorageError::backend)?;
        let row = rows
            .next()
            .await
            .map_err(StorageError::backend)?
            .ok_or_else(|| StorageError::backend("heartbeat task not found"))?;
        row_to_heartbeat_task_run(&row)
    }

    async fn mark_heartbeat_task_running(
        &self,
        run_id: &str,
        started_at_ms: i64,
    ) -> Result<(), StorageError> {
        let sql = format!(
            "UPDATE heartbeat_task
             SET status = '{}', started_at_ms = {}
             WHERE id = '{}'",
            HeartbeatTaskStatus::Running.as_str(),
            started_at_ms,
            escape_sql_text(run_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
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
        let error_sql = error_message
            .map(|value| format!("'{}'", escape_sql_text(value)))
            .unwrap_or_else(|| "NULL".to_string());
        let publish_sql = published_message_id
            .map(|value| format!("'{}'", escape_sql_text(value)))
            .unwrap_or_else(|| "NULL".to_string());
        let sql = format!(
            "UPDATE heartbeat_task
             SET status = '{}',
                 finished_at_ms = {},
                 error_message = {},
                 published_message_id = {}
             WHERE id = '{}'",
            status.as_str(),
            finished_at_ms,
            error_sql,
            publish_sql,
            escape_sql_text(run_id)
        );
        let conn = self.connection().await?;
        conn.execute(&sql, ())
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
        let sql = format!(
            "SELECT id, heartbeat_id, scheduled_at_ms, started_at_ms, finished_at_ms, status,
                    attempt, error_message, published_message_id, created_at_ms
             FROM heartbeat_task
             WHERE heartbeat_id = '{}'
             ORDER BY created_at_ms DESC
             LIMIT {} OFFSET {}",
            escape_sql_text(heartbeat_id),
            limit.max(1),
            offset.max(0)
        );
        let conn = self.connection().await?;
        let mut rows = conn.query(&sql, ()).await.map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_heartbeat_task_run(&row)?);
        }
        Ok(out)
    }
}

fn escape_sql_text(input: &str) -> String {
    input.replace('\'', "''")
}

fn opt_string_sql(value: Option<&str>) -> String {
    value
        .map(|inner| format!("'{}'", escape_sql_text(inner)))
        .unwrap_or_else(|| "NULL".to_string())
}

fn opt_i64_sql(value: Option<i64>) -> String {
    value
        .map(|inner| inner.to_string())
        .unwrap_or_else(|| "NULL".to_string())
}

fn to_turso_params(values: &[DbValue]) -> Vec<Value> {
    values
        .iter()
        .map(|value| match value {
            DbValue::Null => Value::Null,
            DbValue::Integer(v) => Value::Integer(*v),
            DbValue::Real(v) => Value::Real(*v),
            DbValue::Text(v) => Value::Text(v.clone()),
            DbValue::Blob(v) => Value::Blob(v.clone()),
        })
        .collect()
}

fn from_turso_value(value: Value) -> DbValue {
    match value {
        Value::Null => DbValue::Null,
        Value::Integer(v) => DbValue::Integer(v),
        Value::Real(v) => DbValue::Real(v),
        Value::Text(v) => DbValue::Text(v),
        Value::Blob(v) => DbValue::Blob(v),
    }
}

fn row_to_session_index(row: &Row) -> Result<SessionIndex, StorageError> {
    Ok(SessionIndex {
        session_key: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        channel: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        active_session_key: value_to_opt_string(row.get_value(3).map_err(StorageError::backend)?),
        model_provider: value_to_opt_string(row.get_value(4).map_err(StorageError::backend)?),
        model_provider_explicit: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)?
            != 0,
        model: value_to_opt_string(row.get_value(6).map_err(StorageError::backend)?),
        model_explicit: value_to_i64(row.get_value(7).map_err(StorageError::backend)?)? != 0,
        delivery_metadata_json: value_to_opt_string(
            row.get_value(8).map_err(StorageError::backend)?,
        ),
        created_at_ms: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
        last_message_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        turn_count: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        jsonl_path: value_to_string(row.get_value(13).map_err(StorageError::backend)?)?,
    })
}

fn row_to_cron_job(row: &Row) -> Result<CronJob, StorageError> {
    let kind_raw = value_to_string(row.get_value(2).map_err(StorageError::backend)?)?;
    let schedule_kind = CronScheduleKind::parse(&kind_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid cron schedule kind: {kind_raw}")))?;
    Ok(CronJob {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        name: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        schedule_kind,
        schedule_expr: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        payload_json: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        enabled: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)? != 0,
        timezone: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        next_run_at_ms: value_to_i64(row.get_value(7).map_err(StorageError::backend)?)?,
        last_run_at_ms: value_to_opt_i64(row.get_value(8).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
    })
}

fn row_to_cron_task_run(row: &Row) -> Result<CronTaskRun, StorageError> {
    let status_raw = value_to_string(row.get_value(5).map_err(StorageError::backend)?)?;
    let status = CronTaskStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid cron task status: {status_raw}")))?;
    Ok(CronTaskRun {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        cron_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        scheduled_at_ms: value_to_i64(row.get_value(2).map_err(StorageError::backend)?)?,
        started_at_ms: value_to_opt_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        finished_at_ms: value_to_opt_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        status,
        attempt: value_to_i64(row.get_value(6).map_err(StorageError::backend)?)?,
        error_message: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        published_message_id: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
    })
}

fn row_to_heartbeat_job(row: &Row) -> Result<HeartbeatJob, StorageError> {
    Ok(HeartbeatJob {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        channel: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        enabled: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)? != 0,
        every: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        prompt: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        silent_ack_token: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        recent_messages_limit: value_to_i64(row.get_value(8).map_err(StorageError::backend)?)?,
        timezone: value_to_string(row.get_value(9).map_err(StorageError::backend)?)?,
        next_run_at_ms: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
        last_run_at_ms: value_to_opt_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(13).map_err(StorageError::backend)?)?,
    })
}

fn row_to_heartbeat_task_run(row: &Row) -> Result<HeartbeatTaskRun, StorageError> {
    let status_raw = value_to_string(row.get_value(5).map_err(StorageError::backend)?)?;
    let status = HeartbeatTaskStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid heartbeat task status: {status_raw}"))
    })?;
    Ok(HeartbeatTaskRun {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        heartbeat_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        scheduled_at_ms: value_to_i64(row.get_value(2).map_err(StorageError::backend)?)?,
        started_at_ms: value_to_opt_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        finished_at_ms: value_to_opt_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        status,
        attempt: value_to_i64(row.get_value(6).map_err(StorageError::backend)?)?,
        error_message: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        published_message_id: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
    })
}

fn row_to_approval(row: &Row) -> Result<ApprovalRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(6).map_err(StorageError::backend)?)?;
    let status = ApprovalStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid approval status: {status_raw}")))?;
    Ok(ApprovalRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        tool_name: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        command_hash: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        command_preview: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        command_text: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        risk_level: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        status,
        requested_by: value_to_string(row.get_value(8).map_err(StorageError::backend)?)?,
        approved_by: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        justification: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        expires_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(13).map_err(StorageError::backend)?)?,
        consumed_at_ms: value_to_opt_i64(row.get_value(14).map_err(StorageError::backend)?)?,
    })
}

fn row_to_pending_question(row: &Row) -> Result<PendingQuestionRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(7).map_err(StorageError::backend)?)?;
    let status = PendingQuestionStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid pending question status: {status_raw}"))
    })?;
    Ok(PendingQuestionRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        channel: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        title: value_to_opt_string(row.get_value(4).map_err(StorageError::backend)?),
        question_text: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        options_json: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        status,
        selected_option_id: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        answered_by: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        expires_at_ms: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        answered_at_ms: value_to_opt_i64(row.get_value(13).map_err(StorageError::backend)?)?,
    })
}

fn row_to_llm_usage(row: &Row) -> Result<LlmUsageRecord, StorageError> {
    let source_raw = value_to_string(row.get_value(13).map_err(StorageError::backend)?)?;
    let source = LlmUsageSource::parse(&source_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid llm usage source: {source_raw}")))?;
    Ok(LlmUsageRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        turn_index: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        request_seq: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        provider: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        model: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        wire_api: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        input_tokens: value_to_i64(row.get_value(8).map_err(StorageError::backend)?)?,
        output_tokens: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
        total_tokens: value_to_i64(row.get_value(10).map_err(StorageError::backend)?)?,
        cached_input_tokens: value_to_opt_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        reasoning_tokens: value_to_opt_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        source,
        provider_request_id: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        provider_response_id: value_to_opt_string(
            row.get_value(15).map_err(StorageError::backend)?,
        ),
        created_at_ms: value_to_i64(row.get_value(16).map_err(StorageError::backend)?)?,
    })
}

fn row_to_llm_usage_summary(row: &Row) -> Result<LlmUsageSummary, StorageError> {
    Ok(LlmUsageSummary {
        request_count: value_to_i64(row.get_value(0).map_err(StorageError::backend)?)?,
        input_tokens: value_to_i64(row.get_value(1).map_err(StorageError::backend)?)?,
        output_tokens: value_to_i64(row.get_value(2).map_err(StorageError::backend)?)?,
        total_tokens: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        cached_input_tokens: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        reasoning_tokens: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)?,
    })
}

fn row_to_llm_audit(row: &Row) -> Result<LlmAuditRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(8).map_err(StorageError::backend)?)?;
    let status = LlmAuditStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid llm audit status: {status_raw}")))?;
    Ok(LlmAuditRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        turn_index: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        request_seq: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        provider: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        model: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        wire_api: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
        status,
        error_code: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        error_message: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        provider_request_id: value_to_opt_string(row.get_value(11).map_err(StorageError::backend)?),
        provider_response_id: value_to_opt_string(
            row.get_value(12).map_err(StorageError::backend)?,
        ),
        request_body_json: value_to_string(row.get_value(13).map_err(StorageError::backend)?)?,
        response_body_json: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(15).map_err(StorageError::backend)?),
        requested_at_ms: value_to_i64(row.get_value(16).map_err(StorageError::backend)?)?,
        responded_at_ms: value_to_opt_i64(row.get_value(17).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(18).map_err(StorageError::backend)?)?,
    })
}

fn row_to_tool_audit(row: &Row) -> Result<ToolAuditRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(7).map_err(StorageError::backend)?)?;
    let status = ToolAuditStatus::parse(&status_raw)
        .ok_or_else(|| StorageError::backend(format!("invalid tool audit status: {status_raw}")))?;
    Ok(ToolAuditRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        turn_index: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        request_seq: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        tool_call_seq: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)?,
        tool_name: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        status,
        error_code: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        error_message: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        retryable: value_to_opt_i64(row.get_value(10).map_err(StorageError::backend)?)?
            .map(|value| value != 0),
        approval_required: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)? != 0,
        arguments_json: value_to_string(row.get_value(12).map_err(StorageError::backend)?)?,
        result_content: value_to_string(row.get_value(13).map_err(StorageError::backend)?)?,
        error_details_json: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        signals_json: value_to_opt_string(row.get_value(15).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(16).map_err(StorageError::backend)?),
        started_at_ms: value_to_i64(row.get_value(17).map_err(StorageError::backend)?)?,
        finished_at_ms: value_to_i64(row.get_value(18).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(19).map_err(StorageError::backend)?)?,
    })
}

fn row_to_webhook_event(row: &Row) -> Result<WebhookEventRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(9).map_err(StorageError::backend)?)?;
    let status = WebhookEventStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid webhook event status: {status_raw}"))
    })?;
    Ok(WebhookEventRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        source: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        event_type: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        sender_id: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        content: value_to_string(row.get_value(6).map_err(StorageError::backend)?)?,
        payload_json: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(8).map_err(StorageError::backend)?),
        status,
        error_message: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        response_summary: value_to_opt_string(row.get_value(11).map_err(StorageError::backend)?),
        received_at_ms: value_to_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        processed_at_ms: value_to_opt_i64(row.get_value(13).map_err(StorageError::backend)?)?,
        remote_addr: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(15).map_err(StorageError::backend)?)?,
    })
}

fn row_to_webhook_agent(row: &Row) -> Result<WebhookAgentRecord, StorageError> {
    let status_raw = value_to_string(row.get_value(8).map_err(StorageError::backend)?)?;
    let status = WebhookEventStatus::parse(&status_raw).ok_or_else(|| {
        StorageError::backend(format!("invalid webhook agent status: {status_raw}"))
    })?;
    Ok(WebhookAgentRecord {
        id: value_to_string(row.get_value(0).map_err(StorageError::backend)?)?,
        hook_id: value_to_string(row.get_value(1).map_err(StorageError::backend)?)?,
        session_key: value_to_string(row.get_value(2).map_err(StorageError::backend)?)?,
        chat_id: value_to_string(row.get_value(3).map_err(StorageError::backend)?)?,
        sender_id: value_to_string(row.get_value(4).map_err(StorageError::backend)?)?,
        content: value_to_string(row.get_value(5).map_err(StorageError::backend)?)?,
        payload_json: value_to_opt_string(row.get_value(6).map_err(StorageError::backend)?),
        metadata_json: value_to_opt_string(row.get_value(7).map_err(StorageError::backend)?),
        status,
        error_message: value_to_opt_string(row.get_value(9).map_err(StorageError::backend)?),
        response_summary: value_to_opt_string(row.get_value(10).map_err(StorageError::backend)?),
        received_at_ms: value_to_i64(row.get_value(11).map_err(StorageError::backend)?)?,
        processed_at_ms: value_to_opt_i64(row.get_value(12).map_err(StorageError::backend)?)?,
        remote_addr: value_to_opt_string(row.get_value(13).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(14).map_err(StorageError::backend)?)?,
    })
}

fn value_to_string(value: Value) -> Result<String, StorageError> {
    match value {
        Value::Text(v) => Ok(v),
        Value::Integer(v) => Ok(v.to_string()),
        Value::Real(v) => Ok(v.to_string()),
        Value::Null => Ok(String::new()),
        Value::Blob(_) => Err(StorageError::backend("unexpected blob value")),
    }
}

fn value_to_i64(value: Value) -> Result<i64, StorageError> {
    match value {
        Value::Integer(v) => Ok(v),
        Value::Text(v) => v
            .parse::<i64>()
            .map_err(|err| StorageError::backend(format!("invalid integer text: {err}"))),
        Value::Real(v) => Ok(v as i64),
        Value::Null => Ok(0),
        Value::Blob(_) => Err(StorageError::backend("unexpected blob value")),
    }
}

fn value_to_opt_i64(value: Value) -> Result<Option<i64>, StorageError> {
    match value {
        Value::Null => Ok(None),
        other => value_to_i64(other).map(Some),
    }
}

fn llm_audit_requested_range_where(
    requested_from_ms: Option<i64>,
    requested_to_ms: Option<i64>,
) -> String {
    let mut conditions = Vec::new();
    if let Some(from_ms) = requested_from_ms {
        conditions.push(format!("requested_at_ms >= {from_ms}"));
    }
    if let Some(to_ms) = requested_to_ms {
        conditions.push(format!("requested_at_ms <= {to_ms}"));
    }
    if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    }
}

fn tool_audit_started_range_where(
    started_from_ms: Option<i64>,
    started_to_ms: Option<i64>,
) -> String {
    let mut conditions = Vec::new();
    if let Some(from_ms) = started_from_ms {
        conditions.push(format!("started_at_ms >= {from_ms}"));
    }
    if let Some(to_ms) = started_to_ms {
        conditions.push(format!("started_at_ms <= {to_ms}"));
    }
    if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    }
}

async fn collect_string_column(conn: &Connection, sql: &str) -> Result<Vec<String>, StorageError> {
    let mut rows = conn.query(sql, ()).await.map_err(StorageError::backend)?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
        values.push(value_to_string(
            row.get_value(0).map_err(StorageError::backend)?,
        )?);
    }
    Ok(values)
}

fn opt_sql_text(value: Option<&str>) -> String {
    value
        .map(|value| format!("'{}'", escape_sql_text(value)))
        .unwrap_or_else(|| "NULL".to_string())
}

fn opt_sql_i64(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "NULL".to_string())
}

fn value_to_opt_string(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Text(v) => Some(v),
        Value::Integer(v) => Some(v.to_string()),
        Value::Real(v) => Some(v.to_string()),
        Value::Blob(_) => None,
    }
}
