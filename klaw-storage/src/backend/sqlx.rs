use crate::{
    jsonl,
    memory_db::{DbRow, DbValue, MemoryDb},
    util::{now_ms, relative_or_absolute_jsonl},
    ChatRecord, SessionIndex, SessionStorage, StorageError, StoragePaths,
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
