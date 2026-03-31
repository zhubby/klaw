use crate::media::{attach_declared_media_metadata, build_media_reference};
use klaw_core::{MediaReference, MediaSourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ops::Range;
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
    pub entities: Vec<TelegramMessageEntity>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub caption_entities: Vec<TelegramMessageEntity>,
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
    #[serde(default)]
    pub reply_to_message: Option<Box<TelegramMessage>>,
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
    #[serde(rename = "type")]
    pub kind: TelegramChatType,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUser {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramChatType {
    Private,
    Group,
    Supergroup,
    Channel,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramMessageEntityType {
    Mention,
    BotCommand,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramMessageEntity {
    #[serde(rename = "type")]
    pub kind: TelegramMessageEntityType,
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramBotProfile {
    pub user_id: i64,
    pub username: String,
}

impl TelegramBotProfile {
    pub fn from_user(user: TelegramUser) -> Option<Self> {
        let username = user.username?.trim().to_string();
        if username.is_empty() {
            return None;
        }
        Some(Self {
            user_id: user.id,
            username,
        })
    }

    pub fn mention_handle(&self) -> String {
        format!("@{}", self.username)
    }
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
    pub fn from_message(
        update_id: i64,
        message: TelegramMessage,
        bot_profile: Option<&TelegramBotProfile>,
    ) -> Option<Self> {
        let text = normalized_message_text(&message, bot_profile)?;
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

pub fn normalized_message_text(
    message: &TelegramMessage,
    bot_profile: Option<&TelegramBotProfile>,
) -> Option<String> {
    if should_ignore_chat_kind(message.chat.kind) {
        return None;
    }

    let is_group_chat = matches!(
        message.chat.kind,
        TelegramChatType::Group | TelegramChatType::Supergroup
    );
    if is_group_chat && !group_message_targets_bot(message, bot_profile) {
        return None;
    }

    let text = normalized_text_or_caption(message, bot_profile)
        .or_else(|| message_text(message))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    Some(text)
}

fn normalized_text_or_caption(
    message: &TelegramMessage,
    bot_profile: Option<&TelegramBotProfile>,
) -> Option<String> {
    let (content, entities) = text_with_entities(message)?;
    let normalized = normalize_message_content(content, entities, bot_profile);
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn text_with_entities(message: &TelegramMessage) -> Option<(&str, &[TelegramMessageEntity])> {
    if let Some(text) = message.text.as_deref() {
        return Some((text, &message.entities));
    }
    if let Some(caption) = message.caption.as_deref() {
        return Some((caption, &message.caption_entities));
    }
    None
}

fn should_ignore_chat_kind(kind: TelegramChatType) -> bool {
    matches!(kind, TelegramChatType::Channel)
}

pub fn group_message_targets_bot(
    message: &TelegramMessage,
    bot_profile: Option<&TelegramBotProfile>,
) -> bool {
    let Some(bot_profile) = bot_profile else {
        return false;
    };

    if message
        .reply_to_message
        .as_deref()
        .and_then(|reply| reply.from.as_ref())
        .is_some_and(|from| from.is_bot && from.id == bot_profile.user_id)
    {
        return true;
    }

    let Some((content, entities)) = text_with_entities(message) else {
        return false;
    };

    entities
        .iter()
        .any(|entity| entity_targets_bot(content, entity, bot_profile))
}

fn entity_targets_bot(
    content: &str,
    entity: &TelegramMessageEntity,
    bot_profile: &TelegramBotProfile,
) -> bool {
    let Some(fragment) = entity_text(content, entity) else {
        return false;
    };

    match entity.kind {
        TelegramMessageEntityType::Mention => {
            fragment.eq_ignore_ascii_case(bot_profile.mention_handle().as_str())
        }
        TelegramMessageEntityType::BotCommand => command_targets_bot(fragment, bot_profile),
        TelegramMessageEntityType::Unknown => false,
    }
}

fn normalize_message_content(
    content: &str,
    entities: &[TelegramMessageEntity],
    bot_profile: Option<&TelegramBotProfile>,
) -> String {
    let mut ranges = Vec::new();
    let mut replacements: Vec<(Range<usize>, String)> = Vec::new();

    for entity in entities {
        let Some(range) = entity_byte_range(content, entity) else {
            continue;
        };
        match entity.kind {
            TelegramMessageEntityType::Mention => {
                if bot_profile.is_some_and(|profile| {
                    content[range.clone()].eq_ignore_ascii_case(profile.mention_handle().as_str())
                }) {
                    ranges.push(range);
                }
            }
            TelegramMessageEntityType::BotCommand => {
                if let Some(profile) = bot_profile {
                    let fragment = &content[range.clone()];
                    if let Some(normalized) = normalized_targeted_command(fragment, profile) {
                        replacements.push((range, normalized));
                    }
                }
            }
            TelegramMessageEntityType::Unknown => {}
        }
    }

    ranges.sort_by_key(|range| range.start);
    replacements.sort_by_key(|(range, _)| range.start);

    let stripped = strip_ranges(content, &ranges);
    apply_replacements(&stripped, &adjusted_replacements(&ranges, replacements))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn adjusted_replacements(
    removed_ranges: &[Range<usize>],
    replacements: Vec<(Range<usize>, String)>,
) -> Vec<(Range<usize>, String)> {
    replacements
        .into_iter()
        .filter_map(|(range, replacement)| {
            let mut removed_before_start = 0usize;
            let mut removed_before_end = 0usize;
            for removed in removed_ranges {
                if removed.end <= range.start {
                    removed_before_start += removed.len();
                }
                if removed.end <= range.end {
                    removed_before_end += removed.len();
                }
            }
            let start = range.start.checked_sub(removed_before_start)?;
            let end = range.end.checked_sub(removed_before_end)?;
            Some((start..end, replacement))
        })
        .collect()
}

fn apply_replacements(content: &str, replacements: &[(Range<usize>, String)]) -> String {
    if replacements.is_empty() {
        return content.to_string();
    }

    let mut output = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for (range, replacement) in replacements {
        if range.start < cursor || range.end > content.len() {
            continue;
        }
        output.push_str(&content[cursor..range.start]);
        output.push_str(replacement);
        cursor = range.end;
    }
    output.push_str(&content[cursor..]);
    output
}

fn strip_ranges(content: &str, ranges: &[Range<usize>]) -> String {
    if ranges.is_empty() {
        return content.to_string();
    }

    let mut output = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for range in ranges {
        if range.start < cursor || range.end > content.len() {
            continue;
        }
        output.push_str(&content[cursor..range.start]);
        cursor = range.end;
    }
    output.push_str(&content[cursor..]);
    output
}

fn command_targets_bot(fragment: &str, bot_profile: &TelegramBotProfile) -> bool {
    normalized_targeted_command(fragment, bot_profile).is_some()
}

fn normalized_targeted_command(fragment: &str, bot_profile: &TelegramBotProfile) -> Option<String> {
    let (command, target) = fragment.split_once('@')?;
    if target.eq_ignore_ascii_case(bot_profile.username.as_str()) {
        Some(command.to_string())
    } else {
        None
    }
}

fn entity_text<'a>(content: &'a str, entity: &TelegramMessageEntity) -> Option<&'a str> {
    let range = entity_byte_range(content, entity)?;
    content.get(range)
}

fn entity_byte_range(content: &str, entity: &TelegramMessageEntity) -> Option<Range<usize>> {
    let start = utf16_offset_to_byte_index(content, entity.offset)?;
    let end = utf16_offset_to_byte_index(content, entity.offset + entity.length)?;
    Some(start..end)
}

fn utf16_offset_to_byte_index(content: &str, target: usize) -> Option<usize> {
    if target == 0 {
        return Some(0);
    }

    let mut seen = 0usize;
    for (index, ch) in content.char_indices() {
        if seen == target {
            return Some(index);
        }
        seen += ch.len_utf16();
    }
    if seen == target {
        Some(content.len())
    } else {
        None
    }
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

    fn private_chat() -> TelegramChat {
        TelegramChat {
            id: 10,
            kind: TelegramChatType::Private,
        }
    }

    fn group_chat() -> TelegramChat {
        TelegramChat {
            id: 10,
            kind: TelegramChatType::Supergroup,
        }
    }

    fn bot_profile() -> TelegramBotProfile {
        TelegramBotProfile {
            user_id: 42,
            username: "klawbot".to_string(),
        }
    }

    fn bot_user() -> TelegramUser {
        TelegramUser {
            id: 42,
            is_bot: true,
            username: Some("klawbot".to_string()),
        }
    }

    #[test]
    fn message_text_falls_back_for_additional_media_types() {
        let message = TelegramMessage {
            message_id: 1,
            chat: private_chat(),
            from: None,
            text: None,
            entities: Vec::new(),
            caption: None,
            caption_entities: Vec::new(),
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
            reply_to_message: None,
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
                username: None,
            },
            data: Some("approve:approval-1".to_string()),
            message: Some(TelegramMessage {
                message_id: 1,
                chat: private_chat(),
                from: None,
                text: None,
                entities: Vec::new(),
                caption: None,
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            }),
        })
        .expect("callback inbound");

        assert_eq!(inbound.chat_id, "10");
        assert_eq!(inbound.command, "/approve approval-1");
    }

    #[test]
    fn normalized_message_text_strips_group_mention() {
        let message = TelegramMessage {
            message_id: 7,
            chat: group_chat(),
            from: None,
            text: Some("@klawbot 帮我看下".to_string()),
            entities: vec![TelegramMessageEntity {
                kind: TelegramMessageEntityType::Mention,
                offset: 0,
                length: 8,
            }],
            caption: None,
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };

        assert_eq!(
            normalized_message_text(&message, Some(&bot_profile())).as_deref(),
            Some("帮我看下")
        );
    }

    #[test]
    fn normalized_message_text_strips_targeted_group_command() {
        let message = TelegramMessage {
            message_id: 8,
            chat: group_chat(),
            from: None,
            text: Some("/help@klawbot".to_string()),
            entities: vec![TelegramMessageEntity {
                kind: TelegramMessageEntityType::BotCommand,
                offset: 0,
                length: 13,
            }],
            caption: None,
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };

        assert_eq!(
            normalized_message_text(&message, Some(&bot_profile())).as_deref(),
            Some("/help")
        );
    }

    #[test]
    fn normalized_message_text_ignores_untargeted_group_messages() {
        let message = TelegramMessage {
            message_id: 9,
            chat: group_chat(),
            from: None,
            text: Some("大家好".to_string()),
            entities: Vec::new(),
            caption: None,
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };

        assert_eq!(
            normalized_message_text(&message, Some(&bot_profile())),
            None
        );
    }

    #[test]
    fn normalized_message_text_accepts_reply_to_bot_in_group() {
        let message = TelegramMessage {
            message_id: 10,
            chat: group_chat(),
            from: None,
            text: Some("继续说".to_string()),
            entities: Vec::new(),
            caption: None,
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: Some(Box::new(TelegramMessage {
                message_id: 11,
                chat: group_chat(),
                from: Some(bot_user()),
                text: Some("前文".to_string()),
                entities: Vec::new(),
                caption: None,
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            })),
        };

        assert_eq!(
            normalized_message_text(&message, Some(&bot_profile())).as_deref(),
            Some("继续说")
        );
    }

    #[test]
    fn normalized_message_text_uses_caption_entities() {
        let message = TelegramMessage {
            message_id: 12,
            chat: group_chat(),
            from: None,
            text: None,
            entities: Vec::new(),
            caption: Some("@klawbot 看图".to_string()),
            caption_entities: vec![TelegramMessageEntity {
                kind: TelegramMessageEntityType::Mention,
                offset: 0,
                length: 8,
            }],
            photo: vec![TelegramPhotoSize {
                file_id: "f1".to_string(),
                file_unique_id: "u1".to_string(),
                width: 10,
                height: 10,
            }],
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };

        assert_eq!(
            normalized_message_text(&message, Some(&bot_profile())).as_deref(),
            Some("看图")
        );
    }

    #[test]
    fn normalized_message_text_handles_utf16_offsets() {
        let message = TelegramMessage {
            message_id: 13,
            chat: group_chat(),
            from: None,
            text: Some("🙂 @klawbot 帮忙".to_string()),
            entities: vec![TelegramMessageEntity {
                kind: TelegramMessageEntityType::Mention,
                offset: 3,
                length: 8,
            }],
            caption: None,
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };

        assert_eq!(
            normalized_message_text(&message, Some(&bot_profile())).as_deref(),
            Some("🙂 帮忙")
        );
    }
}
