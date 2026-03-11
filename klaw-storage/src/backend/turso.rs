use crate::{
    jsonl,
    memory_db::{DbRow, DbValue, MemoryDb},
    util::{now_ms, relative_or_absolute_jsonl},
    ChatRecord, SessionIndex, SessionStorage, StorageError, StoragePaths,
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
                ON sessions(updated_at_ms DESC);",
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
