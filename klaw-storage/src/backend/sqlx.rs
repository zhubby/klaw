use crate::{
    jsonl,
    memory_db::{DbRow, DbValue, MemoryDb},
    util::{now_ms, relative_or_absolute_jsonl},
    ChatRecord, CronJob, CronScheduleKind, CronStorage, CronTaskRun, CronTaskStatus, NewCronJob,
    NewCronTaskRun, SessionIndex, SessionStorage, StorageError, StoragePaths, UpdateCronJobPatch,
};
use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Column, FromRow, Row, SqlitePool, TypeInfo,
};
use std::path::PathBuf;

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

impl From<SessionIndexRow> for SessionIndex {
    fn from(value: SessionIndexRow) -> Self {
        Self {
            session_key: value.session_key,
            chat_id: value.chat_id,
            channel: value.channel,
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

impl SqlxSessionStore {
    pub async fn open(paths: StoragePaths) -> Result<Self, StorageError> {
        paths.ensure_dirs().await?;
        let connect_options = SqliteConnectOptions::new()
            .filename(&paths.db_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options)
            .await
            .map_err(StorageError::backend)?;
        let store = Self { paths, pool };
        store.init().await?;
        Ok(store)
    }

    async fn init(&self) -> Result<(), StorageError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                session_key TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                channel TEXT NOT NULL,
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
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_sessions_updated_at_ms
             ON sessions(updated_at_ms DESC)",
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
        Ok(())
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
impl MemoryDb for SqlxMemoryDb {
    async fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        sqlx::query(sql)
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
        sqlx::query(sql)
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
                session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)
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
                    session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
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
            "SELECT session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
             WHERE session_key = ?1",
        )
        .bind(session_key)
        .fetch_one(&self.pool)
        .await
        .map_err(StorageError::backend)?;
        Ok(row.into())
    }

    async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SessionIndex>, StorageError> {
        let rows = sqlx::query_as::<_, SessionIndexRow>(
            "SELECT session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             FROM sessions
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
