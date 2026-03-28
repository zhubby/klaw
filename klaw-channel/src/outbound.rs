use crate::{ChannelResult, OutboundAttachment, OutboundAttachmentKind};
use klaw_archive::{ArchiveBlob, ArchiveRecord, ArchiveService};

#[derive(Debug, Clone)]
pub struct ResolvedOutboundAttachment {
    pub archive_id: String,
    pub kind: OutboundAttachmentKind,
    pub filename: String,
    pub mime_type: Option<String>,
    pub caption: Option<String>,
    pub bytes: Vec<u8>,
    pub record: ArchiveRecord,
}

pub async fn resolve_outbound_attachment(
    archive_service: &dyn ArchiveService,
    attachment: &OutboundAttachment,
) -> ChannelResult<ResolvedOutboundAttachment> {
    let ArchiveBlob { record, bytes, .. } = archive_service
        .open_download(&attachment.archive_id)
        .await?;
    if bytes.is_empty() {
        return Err(format!(
            "archive attachment {} has no content",
            attachment.archive_id
        )
        .into());
    }

    let filename = attachment
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            record
                .original_filename
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("{}.bin", attachment.archive_id));

    let caption = attachment
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    Ok(ResolvedOutboundAttachment {
        archive_id: attachment.archive_id.clone(),
        kind: attachment.kind,
        filename,
        mime_type: record.mime_type.clone(),
        caption,
        bytes,
        record,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use klaw_archive::{
        ArchiveError, ArchiveMediaKind, ArchiveRecord, ArchiveService, ArchiveSourceKind,
    };
    use std::path::{Path, PathBuf};

    #[derive(Clone)]
    struct FakeArchiveService {
        record: Option<ArchiveRecord>,
        bytes: Vec<u8>,
    }

    #[async_trait]
    impl ArchiveService for FakeArchiveService {
        async fn ingest_path(
            &self,
            _input: klaw_archive::ArchiveIngestInput,
            _source_path: &Path,
        ) -> Result<ArchiveRecord, ArchiveError> {
            unreachable!()
        }

        async fn ingest_bytes(
            &self,
            _input: klaw_archive::ArchiveIngestInput,
            _bytes: &[u8],
        ) -> Result<ArchiveRecord, ArchiveError> {
            unreachable!()
        }

        async fn find(
            &self,
            _query: klaw_archive::ArchiveQuery,
        ) -> Result<Vec<ArchiveRecord>, ArchiveError> {
            unreachable!()
        }

        async fn get(&self, archive_id: &str) -> Result<ArchiveRecord, ArchiveError> {
            self.record
                .clone()
                .filter(|record| record.id == archive_id)
                .ok_or_else(|| ArchiveError::NotFound(archive_id.to_string()))
        }

        async fn open_download(&self, archive_id: &str) -> Result<ArchiveBlob, ArchiveError> {
            let record = self.get(archive_id).await?;
            Ok(ArchiveBlob {
                record,
                absolute_path: PathBuf::from("/tmp/fake"),
                bytes: self.bytes.clone(),
            })
        }

        async fn list_session_keys(&self) -> Result<Vec<String>, ArchiveError> {
            unreachable!()
        }
    }

    fn sample_record(id: &str) -> ArchiveRecord {
        ArchiveRecord {
            id: id.to_string(),
            source_kind: ArchiveSourceKind::ModelGenerated,
            media_kind: ArchiveMediaKind::Image,
            mime_type: Some("image/png".to_string()),
            extension: Some("png".to_string()),
            original_filename: Some("sample.png".to_string()),
            content_sha256: "sha".to_string(),
            size_bytes: 4,
            storage_rel_path: "archives/sample.png".to_string(),
            session_key: None,
            channel: None,
            chat_id: None,
            message_id: None,
            metadata_json: "{}".to_string(),
            created_at_ms: 0,
        }
    }

    #[tokio::test]
    async fn resolves_attachment_with_record_filename() {
        let service = FakeArchiveService {
            record: Some(sample_record("arch-1")),
            bytes: vec![1, 2, 3, 4],
        };
        let attachment = OutboundAttachment {
            archive_id: "arch-1".to_string(),
            kind: OutboundAttachmentKind::Image,
            filename: None,
            caption: Some("  hello ".to_string()),
        };

        let resolved = resolve_outbound_attachment(&service, &attachment)
            .await
            .expect("should resolve");

        assert_eq!(resolved.filename, "sample.png");
        assert_eq!(resolved.caption.as_deref(), Some("hello"));
        assert_eq!(resolved.mime_type.as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn rejects_empty_bytes() {
        let service = FakeArchiveService {
            record: Some(sample_record("arch-1")),
            bytes: Vec::new(),
        };
        let attachment = OutboundAttachment {
            archive_id: "arch-1".to_string(),
            kind: OutboundAttachmentKind::File,
            filename: None,
            caption: None,
        };

        let error = resolve_outbound_attachment(&service, &attachment)
            .await
            .expect_err("should reject empty blob");

        assert!(error.to_string().contains("has no content"));
    }
}
