use crate::media::{attach_declared_media_metadata, build_media_reference};
use klaw_core::{MediaReference, MediaSourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Instant;
use tokio::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<TelegramMessage>,
    #[serde(default)]
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramMessage {
    pub message_id: i64,
    pub chat: TelegramChat,
    #[serde(default)]
    pub from: Option<TelegramUser>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub photo: Vec<TelegramPhotoSize>,
    #[serde(default)]
    pub document: Option<TelegramDocument>,
    #[serde(default)]
    pub audio: Option<TelegramAudio>,
    #[serde(default)]
    pub voice: Option<TelegramVoice>,
    #[serde(default)]
    pub video: Option<TelegramVideo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub from: TelegramUser,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub message: Option<TelegramMessage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramChat {
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUser {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramPhotoSize {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: String,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramDocument {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramAudio {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramVoice {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: String,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramVideo {
    pub file_id: String,
    #[serde(default)]
    pub file_unique_id: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramFile {
    #[serde(default)]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TelegramInbound {
    pub update_id: i64,
    pub message_id: String,
    pub chat_id: String,
    pub text: String,
    pub media_references: Vec<MediaReference>,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct TelegramCallbackInbound {
    pub callback_id: String,
    pub chat_id: String,
    pub command: String,
}

impl TelegramInbound {
    pub fn from_message(update_id: i64, message: TelegramMessage) -> Option<Self> {
        let text = message_text(&message)?;
        Some(Self {
            update_id,
            message_id: message.message_id.to_string(),
            chat_id: message.chat.id.to_string(),
            text,
            media_references: extract_media_references(&message),
            metadata: BTreeMap::new(),
        })
    }
}

impl TelegramCallbackInbound {
    pub fn from_callback(query: TelegramCallbackQuery) -> Option<Self> {
        let action = query
            .data
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let (verb, approval_id) = action.split_once(':')?;
        let command = match verb {
            "approve" | "reject" => format!("/{verb} {}", approval_id.trim()),
            _ => return None,
        };
        Some(Self {
            callback_id: query.id,
            chat_id: query.message?.chat.id.to_string(),
            command,
        })
    }
}

pub fn message_text(message: &TelegramMessage) -> Option<String> {
    if let Some(text) = message
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(text.to_string());
    }
    if let Some(caption) = message
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(caption.to_string());
    }
    if !message.photo.is_empty() {
        return Some("Received photo attachment.".to_string());
    }
    if message.document.is_some() {
        return Some("Received document attachment.".to_string());
    }
    if message.audio.is_some() {
        return Some("Received audio attachment.".to_string());
    }
    if message.voice.is_some() {
        return Some("Received voice attachment.".to_string());
    }
    if message.video.is_some() {
        return Some("Received video attachment.".to_string());
    }
    None
}

pub fn extract_media_references(message: &TelegramMessage) -> Vec<MediaReference> {
    let mut out = Vec::new();

    if let Some(photo) = message
        .photo
        .iter()
        .max_by_key(|photo| (photo.width, photo.height))
    {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "telegram.file_id".to_string(),
            Value::String(photo.file_id.clone()),
        );
        if !photo.file_unique_id.trim().is_empty() {
            metadata.insert(
                "telegram.file_unique_id".to_string(),
                Value::String(photo.file_unique_id.clone()),
            );
        }
        attach_declared_media_metadata(
            &mut metadata,
            Some("image/jpeg"),
            Some("jpg"),
            "telegram.declared_mime_type",
            "telegram.declared_file_extension",
        );
        out.push(build_media_reference(
            MediaSourceKind::ChannelInbound,
            &message.message_id.to_string(),
            Some(format!("telegram-photo-{}.jpg", message.message_id)),
            Some("image/jpeg".to_string()),
            metadata,
        ));
    }

    if let Some(document) = message.document.as_ref() {
        out.push(build_file_media_reference(
            &message.message_id.to_string(),
            &document.file_id,
            &document.file_unique_id,
            document.file_name.clone(),
            document.mime_type.clone(),
        ));
    }

    if let Some(audio) = message.audio.as_ref() {
        out.push(build_file_media_reference(
            &message.message_id.to_string(),
            &audio.file_id,
            &audio.file_unique_id,
            audio.file_name.clone(),
            audio.mime_type.clone(),
        ));
    }

    if let Some(voice) = message.voice.as_ref() {
        out.push(build_file_media_reference(
            &message.message_id.to_string(),
            &voice.file_id,
            &voice.file_unique_id,
            Some(format!("telegram-voice-{}.ogg", message.message_id)),
            voice
                .mime_type
                .clone()
                .or_else(|| Some("audio/ogg".to_string())),
        ));
    }

    if let Some(video) = message.video.as_ref() {
        out.push(build_file_media_reference(
            &message.message_id.to_string(),
            &video.file_id,
            &video.file_unique_id,
            video
                .file_name
                .clone()
                .or_else(|| Some(format!("telegram-video-{}.mp4", message.message_id))),
            video
                .mime_type
                .clone()
                .or_else(|| Some("video/mp4".to_string())),
        ));
    }

    out
}

fn build_file_media_reference(
    message_id: &str,
    file_id: &str,
    file_unique_id: &str,
    file_name: Option<String>,
    mime_type: Option<String>,
) -> MediaReference {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "telegram.file_id".to_string(),
        Value::String(file_id.to_string()),
    );
    if !file_unique_id.trim().is_empty() {
        metadata.insert(
            "telegram.file_unique_id".to_string(),
            Value::String(file_unique_id.to_string()),
        );
    }
    attach_declared_media_metadata(
        &mut metadata,
        mime_type.as_deref(),
        file_name.as_deref().and_then(file_extension_from_name),
        "telegram.declared_mime_type",
        "telegram.declared_file_extension",
    );
    build_media_reference(
        MediaSourceKind::ChannelInbound,
        message_id,
        file_name,
        mime_type,
        metadata,
    )
}

fn file_extension_from_name(name: &str) -> Option<&str> {
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.trim();
    if ext.is_empty() { None } else { Some(ext) }
}

pub fn is_sender_allowed(allowlist: &[String], sender_id: &str) -> bool {
    if allowlist.is_empty() {
        return true;
    }
    allowlist.iter().any(|allowed| {
        let allowed = allowed.trim();
        allowed == "*" || allowed == sender_id
    })
}

#[derive(Debug, Clone)]
pub struct EventDeduper {
    ttl: Duration,
    max_entries: usize,
    timestamps: HashMap<String, Instant>,
    order: VecDeque<String>,
}

impl EventDeduper {
    pub fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries,
            timestamps: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn insert_if_new(&mut self, key: &str) -> bool {
        let now = Instant::now();
        self.evict_expired(now);
        if self.timestamps.contains_key(key) {
            return false;
        }
        let key = key.to_string();
        self.timestamps.insert(key.clone(), now);
        self.order.push_back(key);
        while self.order.len() > self.max_entries {
            if let Some(front) = self.order.pop_front() {
                self.timestamps.remove(&front);
            }
        }
        true
    }

    fn evict_expired(&mut self, now: Instant) {
        while let Some(front) = self.order.front().cloned() {
            let Some(timestamp) = self.timestamps.get(&front).copied() else {
                self.order.pop_front();
                continue;
            };
            if now.duration_since(timestamp) < self.ttl {
                break;
            }
            self.order.pop_front();
            self.timestamps.remove(&front);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramInlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<TelegramInlineKeyboardButton>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramInlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

impl TelegramInlineKeyboardMarkup {
    pub fn approval(approval_id: &str) -> Self {
        Self {
            inline_keyboard: vec![vec![
                TelegramInlineKeyboardButton {
                    text: "Approve".to_string(),
                    callback_data: format!("approve:{approval_id}"),
                },
                TelegramInlineKeyboardButton {
                    text: "Reject".to_string(),
                    callback_data: format!("reject:{approval_id}"),
                },
            ]],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_text_falls_back_for_additional_media_types() {
        let message = TelegramMessage {
            message_id: 1,
            chat: TelegramChat { id: 10 },
            from: None,
            text: None,
            caption: None,
            photo: Vec::new(),
            document: None,
            audio: Some(TelegramAudio {
                file_id: "a1".to_string(),
                file_unique_id: String::new(),
                file_name: Some("clip.mp3".to_string()),
                mime_type: Some("audio/mpeg".to_string()),
            }),
            voice: None,
            video: None,
        };
        assert_eq!(
            message_text(&message).as_deref(),
            Some("Received audio attachment.")
        );
    }

    #[test]
    fn callback_query_maps_to_command() {
        let inbound = TelegramCallbackInbound::from_callback(TelegramCallbackQuery {
            id: "cb-1".to_string(),
            from: TelegramUser {
                id: 1,
                is_bot: false,
            },
            data: Some("approve:approval-1".to_string()),
            message: Some(TelegramMessage {
                message_id: 1,
                chat: TelegramChat { id: 10 },
                from: None,
                text: None,
                caption: None,
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
            }),
        })
        .expect("callback inbound");

        assert_eq!(inbound.chat_id, "10");
        assert_eq!(inbound.command, "/approve approval-1");
    }
}
