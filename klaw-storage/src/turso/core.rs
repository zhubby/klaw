use crate::{
    StorageError, StoragePaths,
    memory_db::{DbRow, DbValue, MemoryDb},
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use turso::{Builder, Connection, Database};

use super::mapping::{from_turso_value, to_turso_params};

#[derive(Debug, Clone)]
pub struct TursoSessionStore {
    pub(crate) paths: StoragePaths,
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

    pub(crate) async fn connection(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Connection>, StorageError> {
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
                    title TEXT,
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
        self.ensure_session_column("title", "TEXT").await?;
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
