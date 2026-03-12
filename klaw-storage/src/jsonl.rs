use crate::{ChatRecord, StorageError, StoragePaths};
use std::path::PathBuf;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

pub fn session_jsonl_path(paths: &StoragePaths, session_key: &str) -> PathBuf {
    paths.sessions_dir.join(format!(
        "{}.jsonl",
        session_id_from_session_key(session_key)
    ))
}

fn session_id_from_session_key(session_key: &str) -> &str {
    session_key
        .split_once(':')
        .map(|(_, session_id)| session_id)
        .filter(|session_id| !session_id.is_empty())
        .unwrap_or(session_key)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_jsonl_path_uses_session_id_as_filename() {
        let paths = StoragePaths::from_root("/tmp/klaw-storage-jsonl-test".into());
        let file_path = session_jsonl_path(&paths, "stdio:test3");
        assert_eq!(file_path, paths.sessions_dir.join("test3.jsonl"));
    }

    #[test]
    fn session_jsonl_path_falls_back_to_session_key_when_no_channel_prefix() {
        let paths = StoragePaths::from_root("/tmp/klaw-storage-jsonl-test".into());
        let file_path = session_jsonl_path(&paths, "test3");
        assert_eq!(file_path, paths.sessions_dir.join("test3.jsonl"));
    }
}
