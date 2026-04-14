use crate::{ChatRecord, ChatRecordPage, StorageError, StoragePaths};
use std::{io::ErrorKind, path::PathBuf};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

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

pub async fn read_chat_records(
    paths: &StoragePaths,
    session_key: &str,
) -> Result<Vec<ChatRecord>, StorageError> {
    let file_path = session_jsonl_path(paths, session_key);
    let contents = match fs::read_to_string(file_path).await {
        Ok(contents) => contents,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(StorageError::ReadJsonl(err)),
    };

    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<ChatRecord>(line).map_err(StorageError::SerializeJson))
        .collect()
}

pub async fn read_chat_records_page(
    paths: &StoragePaths,
    session_key: &str,
    before_message_id: Option<&str>,
    limit: usize,
) -> Result<ChatRecordPage, StorageError> {
    let limit = limit.max(1);
    let file_path = session_jsonl_path(paths, session_key);
    let mut file = match File::open(file_path).await {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Ok(ChatRecordPage {
                records: Vec::new(),
                has_more: false,
                oldest_message_id: None,
            });
        }
        Err(err) => return Err(StorageError::ReadJsonl(err)),
    };
    let file_len = file
        .metadata()
        .await
        .map_err(StorageError::ReadJsonl)?
        .len();
    if file_len == 0 {
        return Ok(ChatRecordPage {
            records: Vec::new(),
            has_more: false,
            oldest_message_id: None,
        });
    }

    const CHUNK_SIZE: usize = 8 * 1024;
    let mut position = file_len;
    let mut remainder = Vec::new();
    let mut records = Vec::new();
    let mut has_more = false;
    let mut found_cursor = before_message_id.is_none();
    let mut stop = false;

    while position > 0 && !stop {
        let chunk_len = usize::try_from(position.min(CHUNK_SIZE as u64)).unwrap_or(CHUNK_SIZE);
        position -= chunk_len as u64;
        file.seek(std::io::SeekFrom::Start(position))
            .await
            .map_err(StorageError::ReadJsonl)?;
        let mut chunk = vec![0; chunk_len];
        file.read_exact(&mut chunk)
            .await
            .map_err(StorageError::ReadJsonl)?;
        chunk.extend_from_slice(&remainder);

        let mut lines = chunk.split(|byte| *byte == b'\n');
        remainder = lines.next().unwrap_or_default().to_vec();
        for line in lines.rev() {
            if process_history_line(
                line,
                before_message_id,
                &mut found_cursor,
                limit,
                &mut records,
                &mut has_more,
            )? {
                stop = true;
                break;
            }
        }
    }

    if !stop && !remainder.is_empty() {
        let _ = process_history_line(
            &remainder,
            before_message_id,
            &mut found_cursor,
            limit,
            &mut records,
            &mut has_more,
        )?;
    }

    if before_message_id.is_some() && !found_cursor {
        return Err(StorageError::InvalidHistoryCursor(
            before_message_id.unwrap_or_default().to_string(),
        ));
    }

    records.reverse();
    let oldest_message_id = records.first().and_then(|record| record.message_id.clone());
    Ok(ChatRecordPage {
        records,
        has_more,
        oldest_message_id,
    })
}

fn process_history_line(
    line: &[u8],
    before_message_id: Option<&str>,
    found_cursor: &mut bool,
    limit: usize,
    records: &mut Vec<ChatRecord>,
    has_more: &mut bool,
) -> Result<bool, StorageError> {
    if line.iter().all(|byte| byte.is_ascii_whitespace()) {
        return Ok(false);
    }
    let record = serde_json::from_slice::<ChatRecord>(line).map_err(StorageError::SerializeJson)?;
    if !*found_cursor {
        if record.message_id.as_deref() == before_message_id {
            *found_cursor = true;
        }
        return Ok(false);
    }
    if records.len() < limit {
        records.push(record);
        return Ok(false);
    }
    *has_more = true;
    Ok(true)
}

pub async fn delete_chat_records(
    paths: &StoragePaths,
    session_key: &str,
) -> Result<(), StorageError> {
    let file_path = session_jsonl_path(paths, session_key);
    match fs::remove_file(file_path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(StorageError::WriteJsonl(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs;

    async fn create_test_paths(root: &str) -> StoragePaths {
        let paths = StoragePaths::from_root(root.into());
        fs::create_dir_all(&paths.sessions_dir)
            .await
            .expect("sessions dir should exist");
        paths
    }

    #[test]
    fn session_jsonl_path_uses_session_id_as_filename() {
        let paths = StoragePaths::from_root("/tmp/klaw-storage-jsonl-test".into());
        let file_path = session_jsonl_path(&paths, "terminal:test3");
        assert_eq!(file_path, paths.sessions_dir.join("test3.jsonl"));
    }

    #[test]
    fn session_jsonl_path_falls_back_to_session_key_when_no_channel_prefix() {
        let paths = StoragePaths::from_root("/tmp/klaw-storage-jsonl-test".into());
        let file_path = session_jsonl_path(&paths, "test3");
        assert_eq!(file_path, paths.sessions_dir.join("test3.jsonl"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_chat_records_returns_empty_when_session_file_is_missing() {
        let paths = create_test_paths("/tmp/klaw-storage-jsonl-test-missing").await;
        let records = read_chat_records(&paths, "terminal:missing")
            .await
            .expect("missing file should read as empty");
        assert!(records.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_chat_records_page_returns_latest_records_first_page() {
        let paths = create_test_paths("/tmp/klaw-storage-jsonl-test-page-latest").await;
        for idx in 0..5 {
            append_chat_record(
                &paths,
                "terminal:paged",
                &ChatRecord::new("user", format!("m{idx}"), Some(format!("msg-{idx}"))),
            )
            .await
            .expect("append should succeed");
        }

        let page = read_chat_records_page(&paths, "terminal:paged", None, 2)
            .await
            .expect("page should load");
        let contents = page
            .records
            .iter()
            .map(|record| record.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(contents, vec!["m3", "m4"]);
        assert!(page.has_more);
        assert_eq!(page.oldest_message_id.as_deref(), Some("msg-3"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_chat_records_page_returns_records_before_cursor() {
        let paths = create_test_paths("/tmp/klaw-storage-jsonl-test-page-cursor").await;
        for idx in 0..5 {
            append_chat_record(
                &paths,
                "terminal:paged",
                &ChatRecord::new("user", format!("m{idx}"), Some(format!("msg-{idx}"))),
            )
            .await
            .expect("append should succeed");
        }

        let page = read_chat_records_page(&paths, "terminal:paged", Some("msg-3"), 2)
            .await
            .expect("page should load");
        let contents = page
            .records
            .iter()
            .map(|record| record.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(contents, vec!["m1", "m2"]);
        assert!(page.has_more);
        assert_eq!(page.oldest_message_id.as_deref(), Some("msg-1"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_chat_records_page_errors_when_cursor_missing() {
        let paths = create_test_paths("/tmp/klaw-storage-jsonl-test-page-missing").await;
        append_chat_record(
            &paths,
            "terminal:paged",
            &ChatRecord::new("user", "hello", Some("msg-1".to_string())),
        )
        .await
        .expect("append should succeed");

        let err = read_chat_records_page(&paths, "terminal:paged", Some("msg-missing"), 2)
            .await
            .expect_err("missing cursor should fail");
        assert!(matches!(err, StorageError::InvalidHistoryCursor(_)));
    }
}
