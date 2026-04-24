use crate::{
    StorageError, StoragePaths,
    memory_db::{DbRow, DbValue, MemoryDb},
};
use async_trait::async_trait;
use sqlx::{
    Column, Row, SqlitePool, TypeInfo,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SqlxSessionStore {
    pub(crate) paths: StoragePaths,
    pub(crate) pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct SqlxMemoryDb {
    pub(crate) pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct SqlxArchiveDb {
    pub(crate) pool: SqlitePool,
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
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.ensure_session_column("title", "ALTER TABLE sessions ADD COLUMN title TEXT")
            .await?;
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
        self.ensure_session_column(
            "model_provider_explicit",
            "ALTER TABLE sessions ADD COLUMN model_provider_explicit INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        self.ensure_session_column("model", "ALTER TABLE sessions ADD COLUMN model TEXT")
            .await?;
        self.ensure_session_column(
            "model_explicit",
            "ALTER TABLE sessions ADD COLUMN model_explicit INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        self.ensure_session_column(
            "delivery_metadata_json",
            "ALTER TABLE sessions ADD COLUMN delivery_metadata_json TEXT",
        )
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
                metadata_json TEXT,
                requested_at_ms INTEGER NOT NULL,
                responded_at_ms INTEGER,
                created_at_ms INTEGER NOT NULL,
                FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        self.ensure_llm_audit_column(
            "metadata_json",
            "ALTER TABLE llm_audit ADD COLUMN metadata_json TEXT",
        )
        .await?;
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
            "CREATE TABLE IF NOT EXISTS tool_audit (
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
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tool_audit_tool_started
             ON tool_audit(tool_name, started_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tool_audit_session_started
             ON tool_audit(session_key, started_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tool_audit_session_turn
             ON tool_audit(session_key, turn_index, request_seq, tool_call_seq)",
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
            "CREATE TABLE IF NOT EXISTS webhook_agents (
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
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_agents_received
             ON webhook_agents(received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_agents_hook_received
             ON webhook_agents(hook_id, received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_agents_status_received
             ON webhook_agents(status, received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_webhook_agents_session_received
             ON webhook_agents(session_key, received_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pending_questions (
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
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pending_questions_session_status
             ON pending_questions(session_key, status, created_at_ms DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pending_questions_expiry
             ON pending_questions(status, expires_at_ms)",
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
                recent_messages_limit INTEGER NOT NULL DEFAULT 12,
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
        self.ensure_heartbeat_column(
            "recent_messages_limit",
            "ALTER TABLE heartbeat ADD COLUMN recent_messages_limit INTEGER NOT NULL DEFAULT 12",
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

    async fn ensure_heartbeat_column(&self, column: &str, sql: &str) -> Result<(), StorageError> {
        let result = sqlx::query(sql).execute(&self.pool).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name") || message.contains("already exists") {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure heartbeat.{column} column: {message}"
                )))
            }
        }
    }

    async fn ensure_llm_audit_column(&self, column: &str, sql: &str) -> Result<(), StorageError> {
        let result = sqlx::query(sql).execute(&self.pool).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                let message = err.to_string();
                if message.contains("duplicate column name") {
                    return Ok(());
                }
                Err(StorageError::backend(format!(
                    "failed to ensure llm_audit.{column} column: {message}"
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
            query = bind_db_value(query, param.clone());
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
            query = bind_db_value(query, param.clone());
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
