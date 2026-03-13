use crate::{
    jsonl,
    memory_db::{DbRow, DbValue, MemoryDb},
    util::{now_ms, relative_or_absolute_jsonl},
    ChatRecord, CronJob, CronScheduleKind, CronStorage, CronTaskRun, CronTaskStatus, NewCronJob,
    NewCronTaskRun, SessionIndex, SessionStorage, StorageError, StoragePaths, UpdateCronJobPatch,
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
                    created_at_ms INTEGER NOT NULL,
                    updated_at_ms INTEGER NOT NULL,
                    last_message_at_ms INTEGER NOT NULL,
                    turn_count INTEGER NOT NULL DEFAULT 0,
                    jsonl_path TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_sessions_updated_at_ms
                ON sessions(updated_at_ms DESC);
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
                ON cron_task(status, scheduled_at_ms);",
            )
            .await
            .map_err(StorageError::backend)?;
        Ok(())
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
                session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
             ) VALUES ('{}', '{}', '{}', {}, {}, {}, 0, '{}')
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
                    session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
                ) VALUES ('{}', '{}', '{}', {}, {}, {}, 1, '{}')",
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
            "SELECT session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
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

    async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SessionIndex>, StorageError> {
        let sql = format!(
            "SELECT session_key, chat_id, channel, created_at_ms, updated_at_ms, last_message_at_ms, turn_count, jsonl_path
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
        created_at_ms: value_to_i64(row.get_value(3).map_err(StorageError::backend)?)?,
        updated_at_ms: value_to_i64(row.get_value(4).map_err(StorageError::backend)?)?,
        last_message_at_ms: value_to_i64(row.get_value(5).map_err(StorageError::backend)?)?,
        turn_count: value_to_i64(row.get_value(6).map_err(StorageError::backend)?)?,
        jsonl_path: value_to_string(row.get_value(7).map_err(StorageError::backend)?)?,
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
