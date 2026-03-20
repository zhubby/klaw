use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::ArchiveError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveSourceKind {
    UserUpload,
    ChannelInbound,
    ModelGenerated,
}

impl ArchiveSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserUpload => "user_upload",
            Self::ChannelInbound => "channel_inbound",
            Self::ModelGenerated => "model_generated",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user_upload" => Some(Self::UserUpload),
            "channel_inbound" => Some(Self::ChannelInbound),
            "model_generated" => Some(Self::ModelGenerated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveMediaKind {
    Pdf,
    Image,
    Video,
    Audio,
    Other,
}

impl ArchiveMediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pdf" => Some(Self::Pdf),
            "image" => Some(Self::Image),
            "video" => Some(Self::Video),
            "audio" => Some(Self::Audio),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveRecord {
    pub id: String,
    pub source_kind: ArchiveSourceKind,
    pub media_kind: ArchiveMediaKind,
    pub mime_type: Option<String>,
    pub extension: Option<String>,
    pub original_filename: Option<String>,
    pub content_sha256: String,
    pub size_bytes: i64,
    pub storage_rel_path: String,
    pub session_key: Option<String>,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
    pub message_id: Option<String>,
    pub metadata_json: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveIngestInput {
    pub source_kind: ArchiveSourceKind,
    pub filename: Option<String>,
    pub declared_mime_type: Option<String>,
    pub session_key: Option<String>,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
    pub message_id: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArchiveQuery {
    pub session_key: Option<String>,
    pub chat_id: Option<String>,
    pub source_kind: Option<ArchiveSourceKind>,
    pub media_kind: Option<ArchiveMediaKind>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct ArchiveBlob {
    pub record: ArchiveRecord,
    pub absolute_path: PathBuf,
    pub bytes: Vec<u8>,
}

#[async_trait]
pub trait ArchiveService: Send + Sync {
    async fn ingest_path(
        &self,
        input: ArchiveIngestInput,
        source_path: &Path,
    ) -> Result<ArchiveRecord, ArchiveError>;

    async fn ingest_bytes(
        &self,
        input: ArchiveIngestInput,
        bytes: &[u8],
    ) -> Result<ArchiveRecord, ArchiveError>;

    async fn find(&self, query: ArchiveQuery) -> Result<Vec<ArchiveRecord>, ArchiveError>;

    async fn get(&self, archive_id: &str) -> Result<ArchiveRecord, ArchiveError>;

    async fn open_download(&self, archive_id: &str) -> Result<ArchiveBlob, ArchiveError>;
}
