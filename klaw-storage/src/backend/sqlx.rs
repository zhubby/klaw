use crate::{
    jsonl,
    util::{now_ms, relative_or_absolute_jsonl},
    ChatRecord, SessionIndex, SessionStorage, StorageError, StoragePaths,
};
use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    FromRow, SqlitePool,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SqlxSessionStore {
    paths: StoragePaths,
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
