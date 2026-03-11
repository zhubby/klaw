use crate::{ChatRecord, SessionIndex, StorageError};
use async_trait::async_trait;
use std::path::PathBuf;

#[async_trait]
pub trait SessionStorage: Send + Sync {
    async fn touch_session(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn complete_turn(
        &self,
        session_key: &str,
        chat_id: &str,
        channel: &str,
    ) -> Result<SessionIndex, StorageError>;

    async fn append_chat_record(
        &self,
        session_key: &str,
        record: &ChatRecord,
    ) -> Result<(), StorageError>;

    async fn get_session(&self, session_key: &str) -> Result<SessionIndex, StorageError>;

    async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<SessionIndex>, StorageError>;

    fn session_jsonl_path(&self, session_key: &str) -> PathBuf;
}
