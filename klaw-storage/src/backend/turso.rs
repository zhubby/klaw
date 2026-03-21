use crate::{
    jsonl,
    memory_db::{DbRow, DbValue, MemoryDb},
    util::{now_ms, relative_or_absolute_jsonl},
    ApprovalRecord, ApprovalStatus, ChatRecord, CronJob, CronScheduleKind, CronStorage,
    CronTaskRun, CronTaskStatus, LlmAuditQuery, LlmAuditRecord, LlmAuditSortOrder,
    LlmAuditStatus, LlmUsageRecord, LlmUsageSource, LlmUsageSummary, NewApprovalRecord,
    NewCronJob, NewCronTaskRun, NewLlmAuditRecord, NewLlmUsageRecord, SessionCompressionState,
    SessionIndex, SessionStorage, StorageError, StoragePaths, UpdateCronJobPatch,
};
use async_trait::async_trait;
use std::path::PathBuf;
use turso::{value::Value, Builder, Connection, Database, Row};

#[derive(Debug, Clone)]
pub struct TursoSessionStore {
    paths: StoragePaths,
    _db: Database,
    conn: Connection,
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
        let store = Self {
            paths,
            _db: db,
            conn,
        };
        store.init().await?;
        Ok(store)
    }

    async fn init(&self) -> Result<(), StorageError> {
        self.conn
            .execute_batch(
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
                ON approvals(status, expires_at_ms);",
            )
            .await
            .map_err(StorageError::backend)?;
        self.ensure_session_column("active_session_key", "TEXT")
            .await?;
        self.ensure_session_column("model_provider", "TEXT").await?;
        self.ensure_session_column("model", "TEXT").await?;
        self.ensure_session_column("compression_last_len", "INTEGER NOT NULL DEFAULT 0")
            .await?;
        self.ensure_session_column("compression_summary_json", "TEXT")
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
        let result = self.conn.execute(&sql, ()).await;
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
        let result = self.conn.execute(&sql, ()).await;
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
}

impl TursoMemoryDb {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let db = Builder::new_local(&paths.memory_db_path.to_string_lossy())
            .build()
            .await
            .map_err(StorageError::backend)?;
        let conn = db.connect().map_err(StorageError::backend)?;
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
        Ok(Self { _db: db, conn })
    }
}

#[async_trait]
impl MemoryDb for TursoSessionStore {
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
                session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             ) VALUES ('{}', '{}', '{}', NULL, NULL, NULL, {}, {}, {}, 0, '{}')
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
        self.conn
            .execute(&sql, ())
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
        let affected = self
            .conn
            .execute(&update_sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
            let insert_sql = format!(
                "INSERT INTO sessions (
                    session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
                ) VALUES ('{}', '{}', '{}', NULL, NULL, NULL, {}, {}, {}, 1, '{}')",
                escape_sql_text(session_key),
                escape_sql_text(chat_id),
                escape_sql_text(channel),
                now,
                now,
                now,
                escape_sql_text(&jsonl_path_str)
            );
            self.conn
                .execute(&insert_sql, ())
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
        let sql = format!(
            "SELECT session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE session_key = '{}'
             LIMIT 1",
            escape_sql_text(session_key)
        );
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        default_provider: &str,
        default_model: &str,
    ) -> Result<SessionIndex, StorageError> {
        let now = now_ms();
        let jsonl_path = self.session_jsonl_path(session_key);
        let jsonl_path_str = relative_or_absolute_jsonl(&self.paths.root_dir, &jsonl_path);
        let sql = format!(
            "INSERT INTO sessions (
                session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             ) VALUES ('{}', '{}', '{}', '{}', '{}', '{}', {}, {}, {}, 0, '{}')
             ON CONFLICT(session_key) DO UPDATE SET
                chat_id=excluded.chat_id,
                channel=excluded.channel,
                updated_at_ms=excluded.updated_at_ms,
                active_session_key=COALESCE(sessions.active_session_key, excluded.active_session_key),
                model_provider=COALESCE(sessions.model_provider, excluded.model_provider),
                model=COALESCE(sessions.model, excluded.model),
                jsonl_path=excluded.jsonl_path",
            escape_sql_text(session_key),
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            escape_sql_text(session_key),
            escape_sql_text(default_provider),
            escape_sql_text(default_model),
            now,
            now,
            now,
            escape_sql_text(&jsonl_path_str)
        );
        self.conn
            .execute(&sql, ())
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
        let affected = self
            .conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
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
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model_provider = '{}',
                 model = '{}'
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(model_provider),
            escape_sql_text(model),
            escape_sql_text(session_key)
        );
        let affected = self
            .conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
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
        let sql = format!(
            "UPDATE sessions
             SET chat_id = '{}',
                 channel = '{}',
                 updated_at_ms = {},
                 model = '{}'
             WHERE session_key = '{}'",
            escape_sql_text(chat_id),
            escape_sql_text(channel),
            now_ms(),
            escape_sql_text(model),
            escape_sql_text(session_key)
        );
        let affected = self
            .conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
            return Err(StorageError::backend(format!(
                "session '{session_key}' not found when setting model"
            )));
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let affected = self
            .conn
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
    ) -> Result<Vec<SessionIndex>, StorageError> {
        let sql = format!(
            "SELECT session_key, chat_id, channel, active_session_key, model_provider, model, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             ORDER BY updated_at_ms DESC
             LIMIT {} OFFSET {}",
            limit.max(1),
            offset.max(0)
        );
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_session_index(&row)?);
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
        self.conn
            .execute(&sql, ())
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
        let mut rows = self
            .conn
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
                request_body_json, response_body_json, requested_at_ms, responded_at_ms, created_at_ms
            ) VALUES ('{}', '{}', '{}', {}, {}, '{}', '{}', '{}', '{}', {}, {}, {}, {}, '{}', {}, {}, {}, {})",
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
            input.requested_at_ms,
            opt_i64_sql(input.responded_at_ms),
            now
        );
        self.conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let query_sql = format!(
            "SELECT id, session_key, chat_id, turn_index, request_seq, provider, model, wire_api,
                    status, error_code, error_message, provider_request_id, provider_response_id,
                    request_body_json, response_body_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             WHERE id = '{}'
             LIMIT 1",
            escape_sql_text(&input.id)
        );
        let mut rows = self
            .conn
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
                    request_body_json, response_body_json, requested_at_ms, responded_at_ms, created_at_ms
             FROM llm_audit
             {where_clause}
             ORDER BY {sort_order}
             LIMIT {} OFFSET {}",
            query.limit.max(1),
            query.offset.max(0)
        );
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_llm_audit(&row)?);
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
        self.conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let affected = self
            .conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        if affected == 0 {
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
        let affected = self
            .conn
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let Some(row) = rows.next().await.map_err(StorageError::backend)? else {
            return Ok(false);
        };
        let approval_id = value_to_string(row.get_value(0).map_err(StorageError::backend)?)?;
        self.consume_approved_shell_command(&approval_id, session_key, command_hash, now_ms)
            .await
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
        self.conn
            .execute(&sql, ())
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
        self.conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        self.conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn delete_cron(&self, cron_id: &str) -> Result<(), StorageError> {
        let sql = format!("DELETE FROM cron WHERE id = '{}'", escape_sql_text(cron_id));
        self.conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
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
        let affected = self
            .conn
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
        self.conn
            .execute(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let mut rows = self
            .conn
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
        self.conn
            .execute(&sql, ())
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
        self.conn
            .execute(&sql, ())
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
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(StorageError::backend)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.map_err(StorageError::backend)? {
            out.push(row_to_cron_task_run(&row)?);
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
        model: value_to_opt_string(row.get_value(5).map_err(StorageError::backend)?),
        created_at_ms: value_to_i64(row.get_value(6).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(7).map_err(StorageError::backend)?)?,
        last_message_at_ms: value_to_i64(row.get_value(8).map_err(StorageError::backend)?)?,
        turn_count: value_to_i64(row.get_value(9).map_err(StorageError::backend)?)?,
        jsonl_path: value_to_string(row.get_value(10).map_err(StorageError::backend)?)?,
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
        provider_response_id: value_to_opt_string(row.get_value(12).map_err(StorageError::backend)?),
        request_body_json: value_to_string(row.get_value(13).map_err(StorageError::backend)?)?,
        response_body_json: value_to_opt_string(row.get_value(14).map_err(StorageError::backend)?),
        requested_at_ms: value_to_i64(row.get_value(15).map_err(StorageError::backend)?)?,
        responded_at_ms: value_to_opt_i64(row.get_value(16).map_err(StorageError::backend)?)?,
        created_at_ms: value_to_i64(row.get_value(17).map_err(StorageError::backend)?)?,
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

fn value_to_opt_string(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Text(v) => Some(v),
        Value::Integer(v) => Some(v.to_string()),
        Value::Real(v) => Some(v.to_string()),
        Value::Blob(_) => None,
    }
}
