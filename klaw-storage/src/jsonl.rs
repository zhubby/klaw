use crate::{util::encode_session_key, ChatRecord, StorageError, StoragePaths};
use std::path::PathBuf;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

pub fn session_jsonl_path(paths: &StoragePaths, session_key: &str) -> PathBuf {
    paths
        .sessions_dir
        .join(format!("{}.jsonl", encode_session_key(session_key)))
}

pub async fn append_chat_record(
    paths: &StoragePaths,
    session_key: &str,
    record: &ChatRecord,
) -> Result<(), StorageError> {
    let file_path = session_jsonl_path(paths, session_key);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .await
        .map_err(StorageError::WriteJsonl)?;
    let mut line = serde_json::to_vec(record).map_err(StorageError::SerializeJson)?;
    line.push(b'\n');
    file.write_all(&line)
        .await
        .map_err(StorageError::WriteJsonl)?;
    Ok(())
}
