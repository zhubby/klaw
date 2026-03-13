use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 媒体来源类型，供 channel/tool/archive 之间共享。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSourceKind {
    UserUpload,
    ChannelInbound,
    ModelGenerated,
}

/// 标准化的媒体引用，占位表示可归档或可下载的媒体对象。
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
