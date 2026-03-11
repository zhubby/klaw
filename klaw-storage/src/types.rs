use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRecord {
    pub ts_ms: i64,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

impl ChatRecord {
    pub fn new(
        role: impl Into<String>,
        content: impl Into<String>,
        message_id: Option<String>,
    ) -> Self {
        Self {
            ts_ms: crate::util::now_ms(),
            role: role.into(),
            content: content.into(),
            message_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIndex {
    pub session_key: String,
    pub chat_id: String,
    pub channel: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_message_at_ms: i64,
    pub turn_count: i64,
    pub jsonl_path: String,
}
