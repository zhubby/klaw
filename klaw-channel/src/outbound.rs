use crate::{
    ChannelResult, LocalAttachmentPolicy, OutboundAttachment, OutboundAttachmentKind,
    OutboundAttachmentSource,
};
use klaw_archive::{ArchiveBlob, ArchiveService};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct ResolvedOutboundAttachment {
    pub source_label: String,
    pub kind: OutboundAttachmentKind,
    pub filename: String,
    pub mime_type: Option<String>,
    pub caption: Option<String>,
    pub bytes: Vec<u8>,
}

pub async fn resolve_outbound_attachment(
    archive_service: &dyn ArchiveService,
    local_policy: &LocalAttachmentPolicy,
    attachment: &OutboundAttachment,
) -> ChannelResult<ResolvedOutboundAttachment> {
    match &attachment.source {
        OutboundAttachmentSource::ArchiveId { archive_id } => {
            resolve_archive_attachment(archive_service, attachment, archive_id).await
        }
        OutboundAttachmentSource::LocalPath { path } => {
            resolve_local_attachment(local_policy, attachment, path).await
        }
    }
}

async fn resolve_archive_attachment(
    archive_service: &dyn ArchiveService,
    attachment: &OutboundAttachment,
    archive_id: &str,
) -> ChannelResult<ResolvedOutboundAttachment> {
    let archive_id = archive_id.trim();
    let ArchiveBlob { record, bytes, .. } = archive_service.open_download(archive_id).await?;
    if bytes.is_empty() {
        return Err(format!("archive attachment {archive_id} has no content").into());
    }

    Ok(ResolvedOutboundAttachment {
        source_label: archive_id.to_string(),
        kind: attachment.kind,
        filename: resolve_filename(
            attachment.filename.as_deref(),
            record.original_filename.as_deref(),
            Some(archive_id),
        ),
        mime_type: record.mime_type.clone(),
        caption: normalize_optional_string(attachment.caption.as_deref()),
        bytes,
    })
}

async fn resolve_local_attachment(
    local_policy: &LocalAttachmentPolicy,
    attachment: &OutboundAttachment,
    path: &str,
) -> ChannelResult<ResolvedOutboundAttachment> {
    let requested_path = PathBuf::from(path.trim());
    if !requested_path.is_absolute() {
        return Err(format!("local attachment path '{}' must be absolute", path).into());
    }

    let canonical_path = fs::canonicalize(&requested_path).await.map_err(|err| {
        format!(
            "failed to resolve local attachment path '{}': {err}",
            requested_path.display()
        )
    })?;
    let metadata = fs::metadata(&canonical_path).await.map_err(|err| {
        format!(
            "failed to read local attachment metadata '{}': {err}",
            canonical_path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "local attachment path '{}' must be a file",
            canonical_path.display()
        )
        .into());
    }
    let size_bytes = metadata.len();
    if size_bytes == 0 {
        return Err(format!(
            "local attachment path '{}' has no content",
            canonical_path.display()
        )
        .into());
    }
    if size_bytes > local_policy.max_bytes {
        return Err(format!(
            "local attachment '{}' exceeds max_bytes limit ({} > {})",
            canonical_path.display(),
            size_bytes,
            local_policy.max_bytes
        )
        .into());
    }
    if !path_allowed(&canonical_path, local_policy) {
        return Err(format!(
            "local attachment '{}' is outside the workspace and channel allowlist",
            canonical_path.display()
        )
        .into());
    }

    let bytes = fs::read(&canonical_path).await.map_err(|err| {
        format!(
            "failed to read local attachment file '{}': {err}",
            canonical_path.display()
        )
    })?;
    let original_filename = canonical_path.file_name().and_then(|value| value.to_str());

    Ok(ResolvedOutboundAttachment {
        source_label: canonical_path.display().to_string(),
        kind: attachment.kind,
        filename: resolve_filename(
            attachment.filename.as_deref(),
            original_filename,
            original_filename,
        ),
        mime_type: None,
        caption: normalize_optional_string(attachment.caption.as_deref()),
        bytes,
    })
}

fn path_allowed(candidate: &Path, policy: &LocalAttachmentPolicy) -> bool {
    if candidate.starts_with(&policy.workspace_root) {
        return true;
    }
    policy
        .allowlist
        .iter()
        .any(|entry| candidate.starts_with(entry))
}

fn resolve_filename(
    requested: Option<&str>,
    fallback: Option<&str>,
    default_stem: Option<&str>,
) -> String {
    normalize_optional_string(requested)
        .or_else(|| normalize_optional_string(fallback))
        .unwrap_or_else(|| default_stem.unwrap_or("attachment.bin").to_string())
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use klaw_archive::{
        ArchiveError, ArchiveMediaKind, ArchiveRecord, ArchiveService, ArchiveSourceKind,
    };
    use std::path::PathBuf;

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

    fn local_policy() -> LocalAttachmentPolicy {
        LocalAttachmentPolicy {
            workspace_root: std::env::temp_dir(),
            allowlist: Vec::new(),
            max_bytes: 1024,
        }
    }

    #[tokio::test]
    async fn resolves_attachment_with_record_filename() {
        let service = FakeArchiveService {
            record: Some(sample_record("arch-1")),
            bytes: vec![1, 2, 3, 4],
        };
        let attachment = OutboundAttachment {
            source: OutboundAttachmentSource::ArchiveId {
                archive_id: "arch-1".to_string(),
            },
            kind: OutboundAttachmentKind::Image,
            filename: None,
            caption: Some("  hello ".to_string()),
        };

        let resolved = resolve_outbound_attachment(&service, &local_policy(), &attachment)
            .await
            .expect("should resolve");

        assert_eq!(resolved.filename, "sample.png");
        assert_eq!(resolved.caption.as_deref(), Some("hello"));
        assert_eq!(resolved.mime_type.as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn rejects_empty_archive_bytes() {
        let service = FakeArchiveService {
            record: Some(sample_record("arch-1")),
            bytes: Vec::new(),
        };
        let attachment = OutboundAttachment {
            source: OutboundAttachmentSource::ArchiveId {
                archive_id: "arch-1".to_string(),
            },
            kind: OutboundAttachmentKind::File,
            filename: None,
            caption: None,
        };

        let error = resolve_outbound_attachment(&service, &local_policy(), &attachment)
            .await
            .expect_err("should reject empty blob");

        assert!(error.to_string().contains("has no content"));
    }
}
