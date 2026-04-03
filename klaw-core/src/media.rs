use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shared media source categories used across channels, tools, and archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSourceKind {
    UserUpload,
    ChannelInbound,
    ModelGenerated,
}

/// Standardized media reference that can point to archival or downloadable media.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaReference {
    pub source_kind: MediaSourceKind,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub remote_url: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub message_id: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}
