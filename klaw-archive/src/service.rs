use crate::{
    fs_store::{
        archive_absolute_path, fingerprint_bytes, fingerprint_path, write_archived_bytes,
        write_archived_copy,
    },
    sniff::sniff_media,
    ArchiveBlob, ArchiveError, ArchiveIngestInput, ArchiveMediaKind, ArchiveQuery, ArchiveRecord,
    ArchiveService, ArchiveSourceKind,
};
use async_trait::async_trait;
use klaw_storage::{
    open_default_archive_db, DbRow, DbValue, DefaultArchiveDb, MemoryDb, StoragePaths,
};
use std::{path::Path, sync::Arc};
use time::OffsetDateTime;
use tokio::fs;
use uuid::Uuid;

pub struct SqliteArchiveService {
    db: Arc<dyn MemoryDb>,
    paths: StoragePaths,
}

impl SqliteArchiveService {
    pub async fn open_default() -> Result<Self, ArchiveError> {
        let paths = StoragePaths::from_home_dir()?;
        let db = open_default_archive_db().await?;
        Self::new(Arc::new(db), paths).await
    }

    pub async fn new(db: Arc<dyn MemoryDb>, paths: StoragePaths) -> Result<Self, ArchiveError> {
        let service = Self { db, paths };
        service.init_schema().await?;
        Ok(service)
    }

    pub async fn from_default_db(
        db: DefaultArchiveDb,
        paths: StoragePaths,
    ) -> Result<Self, ArchiveError> {
        Self::new(Arc::new(db), paths).await
    }

    async fn init_schema(&self) -> Result<(), ArchiveError> {
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS archives (
                    id TEXT PRIMARY KEY,
                    source_kind TEXT NOT NULL,
                    media_kind TEXT NOT NULL,
                    mime_type TEXT,
                    extension TEXT,
                    original_filename TEXT,
                    content_sha256 TEXT NOT NULL,
                    size_bytes INTEGER NOT NULL,
                    storage_rel_path TEXT NOT NULL,
                    session_key TEXT,
                    channel TEXT,
                    chat_id TEXT,
                    message_id TEXT,
                    metadata_json TEXT NOT NULL,
                    created_at_ms INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_archives_created_at_ms
                ON archives(created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS idx_archives_content_sha256
                ON archives(content_sha256);
                CREATE INDEX IF NOT EXISTS idx_archives_session_key
                ON archives(session_key);
                CREATE INDEX IF NOT EXISTS idx_archives_chat_id
                ON archives(chat_id);
                CREATE INDEX IF NOT EXISTS idx_archives_source_kind
                ON archives(source_kind);
                CREATE INDEX IF NOT EXISTS idx_archives_media_kind
                ON archives(media_kind);",
            )
            .await?;
        Ok(())
    }

    async fn lookup_existing_by_hash(
        &self,
        content_sha256: &str,
    ) -> Result<Option<ArchiveRecord>, ArchiveError> {
        let rows = self
            .db
            .query(
                "SELECT id, source_kind, media_kind, mime_type, extension, original_filename,
                        content_sha256, size_bytes, storage_rel_path, session_key, channel,
                        chat_id, message_id, metadata_json, created_at_ms
                 FROM archives
                 WHERE content_sha256 = ?1
                 ORDER BY created_at_ms ASC
                 LIMIT 1",
                &[DbValue::Text(content_sha256.to_string())],
            )
            .await?;
        rows.into_iter().next().map(row_to_record).transpose()
    }

    async fn insert_record(
        &self,
        input: &ArchiveIngestInput,
        media_kind: ArchiveMediaKind,
        mime_type: Option<String>,
        extension: Option<String>,
        content_sha256: String,
        size_bytes: i64,
        storage_rel_path: String,
    ) -> Result<ArchiveRecord, ArchiveError> {
        let id = Uuid::new_v4().to_string();
        let created_at_ms = now_ms();
        let metadata_json = serde_json::to_string(&input.metadata)?;
        self.db
            .execute(
                "INSERT INTO archives (
                    id, source_kind, media_kind, mime_type, extension, original_filename,
                    content_sha256, size_bytes, storage_rel_path, session_key, channel,
                    chat_id, message_id, metadata_json, created_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                &[
                    DbValue::Text(id.clone()),
                    DbValue::Text(input.source_kind.as_str().to_string()),
                    DbValue::Text(media_kind.as_str().to_string()),
                    opt_text(mime_type.clone()),
                    opt_text(extension.clone()),
                    opt_text(input.filename.clone()),
                    DbValue::Text(content_sha256.clone()),
                    DbValue::Integer(size_bytes),
                    DbValue::Text(storage_rel_path.clone()),
                    opt_text(input.session_key.clone()),
                    opt_text(input.channel.clone()),
                    opt_text(input.chat_id.clone()),
                    opt_text(input.message_id.clone()),
                    DbValue::Text(metadata_json.clone()),
                    DbValue::Integer(created_at_ms),
                ],
            )
            .await?;

        Ok(ArchiveRecord {
            id,
            source_kind: input.source_kind,
            media_kind,
            mime_type,
            extension,
            original_filename: input.filename.clone(),
            content_sha256,
            size_bytes,
            storage_rel_path,
            session_key: input.session_key.clone(),
            channel: input.channel.clone(),
            chat_id: input.chat_id.clone(),
            message_id: input.message_id.clone(),
            metadata_json,
            created_at_ms,
        })
    }

    async fn insert_with_cleanup(
        &self,
        input: &ArchiveIngestInput,
        media_kind: ArchiveMediaKind,
        mime_type: Option<String>,
        extension: Option<String>,
        content_sha256: String,
        size_bytes: i64,
        storage_rel_path: String,
        created_file: bool,
    ) -> Result<ArchiveRecord, ArchiveError> {
        match self
            .insert_record(
                input,
                media_kind,
                mime_type,
                extension,
                content_sha256,
                size_bytes,
                storage_rel_path.clone(),
            )
            .await
        {
            Ok(record) => Ok(record),
            Err(err) => {
                if created_file {
                    let file_path = archive_absolute_path(&self.paths.root_dir, &storage_rel_path);
                    let _ = fs::remove_file(&file_path).await;
                }
                Err(err)
            }
        }
    }
}

#[async_trait]
impl ArchiveService for SqliteArchiveService {
    async fn ingest_path(
        &self,
        input: ArchiveIngestInput,
        source_path: &Path,
    ) -> Result<ArchiveRecord, ArchiveError> {
        let fingerprint = fingerprint_path(source_path).await?;
        let fallback_extension = input.filename.as_deref().and_then(extension_from_filename);
        let sniffed = sniff_media(&fingerprint.header, fallback_extension);

        if let Some(existing) = self
            .lookup_existing_by_hash(&fingerprint.content_sha256)
            .await?
        {
            return self
                .insert_record(
                    &input,
                    existing.media_kind,
                    existing.mime_type,
                    existing.extension,
                    fingerprint.content_sha256,
                    fingerprint.size_bytes,
                    existing.storage_rel_path,
                )
                .await;
        }

        let storage_rel_path =
            write_archived_copy(&self.paths.archives_dir, source_path, &sniffed).await?;
        self.insert_with_cleanup(
            &input,
            sniffed.media_kind,
            sniffed.mime_type,
            sniffed.extension,
            fingerprint.content_sha256,
            fingerprint.size_bytes,
            storage_rel_path,
            true,
        )
        .await
    }

    async fn ingest_bytes(
        &self,
        input: ArchiveIngestInput,
        bytes: &[u8],
    ) -> Result<ArchiveRecord, ArchiveError> {
        let fingerprint = fingerprint_bytes(bytes);
        let fallback_extension = input.filename.as_deref().and_then(extension_from_filename);
        let sniffed = sniff_media(&fingerprint.header, fallback_extension);

        if let Some(existing) = self
            .lookup_existing_by_hash(&fingerprint.content_sha256)
            .await?
        {
            return self
                .insert_record(
                    &input,
                    existing.media_kind,
                    existing.mime_type,
                    existing.extension,
                    fingerprint.content_sha256,
                    fingerprint.size_bytes,
                    existing.storage_rel_path,
                )
                .await;
        }

        let storage_rel_path =
            write_archived_bytes(&self.paths.archives_dir, bytes, &sniffed).await?;
        self.insert_with_cleanup(
            &input,
            sniffed.media_kind,
            sniffed.mime_type,
            sniffed.extension,
            fingerprint.content_sha256,
            fingerprint.size_bytes,
            storage_rel_path,
            true,
        )
        .await
    }

    async fn find(&self, query: ArchiveQuery) -> Result<Vec<ArchiveRecord>, ArchiveError> {
        let limit = if query.limit <= 0 { 20 } else { query.limit };
        let offset = query.offset.max(0);
        let mut sql = "SELECT id, source_kind, media_kind, mime_type, extension, original_filename,
                              content_sha256, size_bytes, storage_rel_path, session_key, channel,
                              chat_id, message_id, metadata_json, created_at_ms
                       FROM archives"
            .to_string();
        let mut filters = Vec::new();
        let mut params = Vec::new();

        if let Some(session_key) = query.session_key {
            filters.push(format!("session_key = ?{}", params.len() + 1));
            params.push(DbValue::Text(session_key));
        }
        if let Some(chat_id) = query.chat_id {
            filters.push(format!("chat_id = ?{}", params.len() + 1));
            params.push(DbValue::Text(chat_id));
        }
        if let Some(source_kind) = query.source_kind {
            filters.push(format!("source_kind = ?{}", params.len() + 1));
            params.push(DbValue::Text(source_kind.as_str().to_string()));
        }
        if let Some(media_kind) = query.media_kind {
            filters.push(format!("media_kind = ?{}", params.len() + 1));
            params.push(DbValue::Text(media_kind.as_str().to_string()));
        }
        if let Some(filename) = query.filename {
            let pattern = format!("%{}%", filename.replace('%', "\\%").replace('_', "\\_"));
            filters.push(format!("original_filename LIKE ?{} ESCAPE '\\'", params.len() + 1));
            params.push(DbValue::Text(pattern));
        }
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        sql.push_str(&format!(
            " ORDER BY created_at_ms DESC LIMIT ?{} OFFSET ?{}",
            params.len() + 1,
            params.len() + 2
        ));
        params.push(DbValue::Integer(limit));
        params.push(DbValue::Integer(offset));

        let rows = self.db.query(&sql, &params).await?;
        rows.into_iter().map(row_to_record).collect()
    }

    async fn get(&self, archive_id: &str) -> Result<ArchiveRecord, ArchiveError> {
        let rows = self
            .db
            .query(
                "SELECT id, source_kind, media_kind, mime_type, extension, original_filename,
                        content_sha256, size_bytes, storage_rel_path, session_key, channel,
                        chat_id, message_id, metadata_json, created_at_ms
                 FROM archives
                 WHERE id = ?1
                 LIMIT 1",
                &[DbValue::Text(archive_id.to_string())],
            )
            .await?;
        let Some(row) = rows.into_iter().next() else {
            return Err(ArchiveError::NotFound(archive_id.to_string()));
        };
        row_to_record(row)
    }

    async fn open_download(&self, archive_id: &str) -> Result<ArchiveBlob, ArchiveError> {
        let record = self.get(archive_id).await?;
        let absolute_path = archive_absolute_path(&self.paths.root_dir, &record.storage_rel_path);
        let bytes = fs::read(&absolute_path)
            .await
            .map_err(|err| ArchiveError::read_file(&absolute_path, err))?;
        Ok(ArchiveBlob {
            record,
            absolute_path,
            bytes,
        })
    }

    async fn list_session_keys(&self) -> Result<Vec<String>, ArchiveError> {
        let rows = self
            .db
            .query(
                "SELECT DISTINCT session_key FROM archives WHERE session_key IS NOT NULL ORDER BY session_key",
                &[],
            )
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                row.get(0).and_then(|v| match v {
                    DbValue::Text(s) => Some(s.clone()),
                    _ => None,
                })
            })
            .collect())
    }
}

fn opt_text(value: Option<String>) -> DbValue {
    value.map(DbValue::Text).unwrap_or(DbValue::Null)
}

fn row_to_record(row: DbRow) -> Result<ArchiveRecord, ArchiveError> {
    let source_kind_raw = row_string(&row, 1)?;
    let media_kind_raw = row_string(&row, 2)?;
    let source_kind = ArchiveSourceKind::parse(&source_kind_raw).ok_or_else(|| {
        ArchiveError::InvalidQuery(format!("invalid source kind `{source_kind_raw}`"))
    })?;
    let media_kind = ArchiveMediaKind::parse(&media_kind_raw).ok_or_else(|| {
        ArchiveError::InvalidQuery(format!("invalid media kind `{media_kind_raw}`"))
    })?;
    Ok(ArchiveRecord {
        id: row_string(&row, 0)?,
        source_kind,
        media_kind,
        mime_type: row_opt_string(&row, 3)?,
        extension: row_opt_string(&row, 4)?,
        original_filename: row_opt_string(&row, 5)?,
        content_sha256: row_string(&row, 6)?,
        size_bytes: row_i64(&row, 7)?,
        storage_rel_path: row_string(&row, 8)?,
        session_key: row_opt_string(&row, 9)?,
        channel: row_opt_string(&row, 10)?,
        chat_id: row_opt_string(&row, 11)?,
        message_id: row_opt_string(&row, 12)?,
        metadata_json: row_string(&row, 13)?,
        created_at_ms: row_i64(&row, 14)?,
    })
}

fn row_string(row: &DbRow, index: usize) -> Result<String, ArchiveError> {
    match row.get(index) {
        Some(DbValue::Text(value)) => Ok(value.clone()),
        Some(DbValue::Integer(value)) => Ok(value.to_string()),
        Some(DbValue::Null) | None => Err(ArchiveError::InvalidQuery(format!(
            "missing text column at index {index}"
        ))),
        Some(other) => Err(ArchiveError::InvalidQuery(format!(
            "unexpected column type at index {index}: {other:?}"
        ))),
    }
}

fn row_opt_string(row: &DbRow, index: usize) -> Result<Option<String>, ArchiveError> {
    match row.get(index) {
        Some(DbValue::Text(value)) => Ok(Some(value.clone())),
        Some(DbValue::Null) | None => Ok(None),
        Some(DbValue::Integer(value)) => Ok(Some(value.to_string())),
        Some(other) => Err(ArchiveError::InvalidQuery(format!(
            "unexpected optional column type at index {index}: {other:?}"
        ))),
    }
}

fn row_i64(row: &DbRow, index: usize) -> Result<i64, ArchiveError> {
    match row.get(index) {
        Some(DbValue::Integer(value)) => Ok(*value),
        Some(DbValue::Text(value)) => value.parse().map_err(|_| {
            ArchiveError::InvalidQuery(format!("invalid integer text at index {index}"))
        }),
        Some(DbValue::Null) | None => Err(ArchiveError::InvalidQuery(format!(
            "missing integer column at index {index}"
        ))),
        Some(other) => Err(ArchiveError::InvalidQuery(format!(
            "unexpected integer column type at index {index}: {other:?}"
        ))),
    }
}

fn extension_from_filename(filename: &str) -> Option<&str> {
    filename
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty())
}

fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

#[cfg(test)]
mod tests {
    use super::SqliteArchiveService;
    use crate::{
        ArchiveIngestInput, ArchiveMediaKind, ArchiveQuery, ArchiveService, ArchiveSourceKind,
    };
    use klaw_storage::{DefaultArchiveDb, StoragePaths};
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tokio::fs;
    use uuid::Uuid;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    async fn create_service() -> SqliteArchiveService {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("klaw-archive-test-{suffix}-{}", Uuid::new_v4()));
        let paths = StoragePaths::from_root(base);
        let db = DefaultArchiveDb::open(paths.clone())
            .await
            .expect("archive db should open");
        SqliteArchiveService::from_default_db(db, paths)
            .await
            .expect("archive service should open")
    }

    fn sample_input() -> ArchiveIngestInput {
        ArchiveIngestInput {
            source_kind: ArchiveSourceKind::UserUpload,
            filename: Some("sample.pdf".to_string()),
            declared_mime_type: Some("application/pdf".to_string()),
            session_key: Some("stdio:test".to_string()),
            channel: Some("stdio".to_string()),
            chat_id: Some("test".to_string()),
            message_id: Some("msg-1".to_string()),
            metadata: json!({"purpose": "test"}),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ingest_bytes_writes_file_and_indexes_record() {
        let service = create_service().await;
        let record = service
            .ingest_bytes(sample_input(), b"%PDF-1.7\nhello")
            .await
            .expect("ingest should succeed");
        assert_eq!(record.media_kind, ArchiveMediaKind::Pdf);
        assert!(record.storage_rel_path.starts_with("archives/"));

        let blob = service
            .open_download(&record.id)
            .await
            .expect("download should succeed");
        assert_eq!(blob.bytes, b"%PDF-1.7\nhello");
        assert!(blob.absolute_path.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ingest_path_copies_external_file() {
        let service = create_service().await;
        let temp_path = std::env::temp_dir().join(format!(
            "klaw-archive-source-{}.png",
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&temp_path, [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
            .await
            .expect("source file should be written");
        let mut input = sample_input();
        input.filename = Some("screen.png".to_string());
        let record = service
            .ingest_path(input, &temp_path)
            .await
            .expect("ingest path should succeed");
        assert_eq!(record.media_kind, ArchiveMediaKind::Image);
        let blob = service
            .open_download(&record.id)
            .await
            .expect("download should succeed");
        assert_eq!(blob.bytes.len(), 8);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn duplicate_content_reuses_storage_path() {
        let service = create_service().await;
        let first = service
            .ingest_bytes(sample_input(), b"ID3test-mp3")
            .await
            .expect("first ingest should succeed");
        let mut second_input = sample_input();
        second_input.message_id = Some("msg-2".to_string());
        let second = service
            .ingest_bytes(second_input, b"ID3test-mp3")
            .await
            .expect("second ingest should succeed");
        assert_ne!(first.id, second.id);
        assert_eq!(first.storage_rel_path, second.storage_rel_path);
        assert_eq!(first.content_sha256, second.content_sha256);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn find_filters_by_chat_and_media_kind() {
        let service = create_service().await;
        let first = service
            .ingest_bytes(sample_input(), b"%PDF-1.7\nhello")
            .await
            .expect("first ingest should succeed");
        let mut second_input = sample_input();
        second_input.chat_id = Some("other".to_string());
        second_input.filename = Some("audio.mp3".to_string());
        service
            .ingest_bytes(second_input, b"ID3music")
            .await
            .expect("second ingest should succeed");

        let results = service
            .find(ArchiveQuery {
                chat_id: Some("test".to_string()),
                media_kind: Some(ArchiveMediaKind::Pdf),
                limit: 10,
                offset: 0,
                ..ArchiveQuery::default()
            })
            .await
            .expect("query should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, first.id);
    }
}
