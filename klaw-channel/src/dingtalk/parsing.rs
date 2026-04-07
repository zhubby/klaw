use crate::{
    ChannelResponse,
    im_card::{ImCard, ImCardActionKind, ImCardKind, parse_im_card_action_token, resolve_im_card},
    media::{
        attach_declared_media_metadata, build_media_reference, first_object_string_value,
        first_string_value, resolve_metadata_value_candidates,
    },
};
use klaw_core::{MediaReference, MediaSourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Instant;
use tokio::time::Duration;

#[derive(Debug, Deserialize)]
pub(super) struct StreamEnvelope {
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub data: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct StreamAck<'a> {
    pub code: i32,
    pub headers: HashMap<&'a str, String>,
    pub message: &'a str,
    pub data: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct InboundEvent {
    pub event_id: String,
    pub chat_id: String,
    pub robot_code: String,
    pub msg_type: String,
    pub sender_id: String,
    pub session_webhook: String,
    pub text: String,
    pub audio_recognition: Option<String>,
    pub media_references: Vec<MediaReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DingtalkConversationType {
    Private,
    Group,
}

pub(super) type ApprovalAction = ImCardActionKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CardCallbackEvent {
    pub event_id: Option<String>,
    pub action: ApprovalAction,
    pub approval_id: String,
    pub sender_id: String,
    pub chat_id: String,
    pub session_webhook: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct EventDeduper {
    ttl: Duration,
    max_entries: usize,
    seen_at: HashMap<String, Instant>,
    order: VecDeque<(Instant, String)>,
}

impl EventDeduper {
    pub(super) fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries: max_entries.max(1),
            seen_at: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(super) fn insert_if_new(&mut self, event_id: &str) -> bool {
        self.prune();
        if self.seen_at.contains_key(event_id) {
            return false;
        }
        let now = Instant::now();
        let event_id = event_id.to_string();
        self.seen_at.insert(event_id.clone(), now);
        self.order.push_back((now, event_id));
        self.prune();
        true
    }

    fn prune(&mut self) {
        let now = Instant::now();
        while let Some((seen_at, event_id)) = self.order.front().cloned() {
            let expired = now.duration_since(seen_at) >= self.ttl;
            let overflowed = self.seen_at.len() > self.max_entries;
            if !expired && !overflowed {
                break;
            }
            self.order.pop_front();
            if self
                .seen_at
                .get(event_id.as_str())
                .is_some_and(|recorded_at| *recorded_at == seen_at)
            {
                self.seen_at.remove(event_id.as_str());
            }
        }
    }
}

pub(super) fn parse_stream_data(data: &Value) -> Option<Value> {
    match data {
        Value::String(raw) => serde_json::from_str(raw).ok(),
        Value::Object(_) => Some(data.clone()),
        _ => None,
    }
}

pub(super) fn dingtalk_command_action_url(action: &str, approval_id: &str) -> String {
    let command = format!("/{action} {approval_id}");
    format!(
        "dtmd://dingtalkclient/sendMessage?content={}",
        urlencoding::encode(&command)
    )
}

pub(super) fn resolve_chat_id(data: &Value, sender_id: &str) -> String {
    let is_private_chat = conversation_type(data) == DingtalkConversationType::Private;

    if is_private_chat {
        sender_id.to_string()
    } else {
        data.get("conversationId")
            .and_then(Value::as_str)
            .unwrap_or(sender_id)
            .to_string()
    }
}

fn conversation_type(data: &Value) -> DingtalkConversationType {
    let is_private_chat = data
        .get("conversationType")
        .and_then(|value| {
            value
                .as_str()
                .map(|v| v == "1")
                .or_else(|| value.as_i64().map(|v| v == 1))
        })
        .unwrap_or(true);
    if is_private_chat {
        DingtalkConversationType::Private
    } else {
        DingtalkConversationType::Group
    }
}

pub(super) fn parse_inbound_event(value: &Value, bot_title: &str) -> Option<InboundEvent> {
    let sender_id = value
        .get("senderStaffId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let chat_id = resolve_chat_id(value, &sender_id);
    let session_webhook = value
        .get("sessionWebhook")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let robot_code = value
        .get("robotCode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let event_id = value
        .get("msgId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let msg_type = value
        .get("msgtype")
        .or_else(|| value.get("msgType"))
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "text".to_string());
    let audio_recognition = extract_audio_recognition_text(value);
    let text = extract_dingtalk_message_text(
        value,
        &msg_type,
        audio_recognition.as_deref(),
        &robot_code,
        bot_title,
    )?;
    let media_references = extract_dingtalk_media_references(value, &msg_type, &event_id);

    Some(InboundEvent {
        event_id,
        chat_id,
        robot_code,
        msg_type,
        sender_id,
        session_webhook,
        text,
        audio_recognition,
        media_references,
    })
}

pub(super) fn parse_card_callback_event(value: &Value) -> Option<CardCallbackEvent> {
    let action_raw = extract_callback_action_value(value)?;
    let (action, approval_id) = parse_approval_action_token(&action_raw)?;
    let sender_id = callback_sender_id(value);
    let chat_id = callback_chat_id(value, &sender_id);
    let session_webhook = callback_session_webhook(value);
    let event_id = callback_event_id(value);
    Some(CardCallbackEvent {
        event_id,
        action,
        approval_id,
        sender_id,
        chat_id,
        session_webhook,
    })
}

fn extract_callback_action_value(value: &Value) -> Option<String> {
    [
        "/value",
        "/actionValue",
        "/action/value",
        "/callbackData/value",
        "/callbackData/action",
        "/cardPrivateData/value",
        "/cardPrivateData/action",
        "/content/value",
        "/content/action",
    ]
    .iter()
    .find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn parse_approval_action_token(value: &str) -> Option<(ApprovalAction, String)> {
    let action = parse_im_card_action_token(value)?;
    Some((action.kind, action.approval_id()?.to_string()))
}

fn callback_sender_id(value: &Value) -> String {
    [
        "/senderStaffId",
        "/staffId",
        "/senderId",
        "/userId",
        "/operatorStaffId",
        "/eventOperatorStaffId",
    ]
    .iter()
    .find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
    .unwrap_or_else(|| "unknown-user".to_string())
}

fn callback_chat_id(value: &Value, sender_id: &str) -> String {
    value
        .pointer("/conversationId")
        .or_else(|| value.pointer("/chatbotConversationId"))
        .or_else(|| value.pointer("/chatId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| sender_id.to_string())
}

fn callback_session_webhook(value: &Value) -> Option<String> {
    value
        .pointer("/sessionWebhook")
        .or_else(|| value.pointer("/conversation/sessionWebhook"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn callback_event_id(value: &Value) -> Option<String> {
    [
        "/eventId",
        "/event_id",
        "/msgId",
        "/messageId",
        "/callbackId",
        "/processQueryKey",
    ]
    .iter()
    .find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub(super) fn extract_dingtalk_message_text(
    value: &Value,
    msg_type: &str,
    audio_recognition: Option<&str>,
    robot_code: &str,
    bot_title: &str,
) -> Option<String> {
    let conversation_type = conversation_type(value);
    if conversation_type == DingtalkConversationType::Group
        && !group_message_targets_bot(value, msg_type, robot_code, bot_title)
    {
        return None;
    }

    if msg_type == "text" {
        return value
            .pointer("/text/content")
            .and_then(Value::as_str)
            .map(|text| normalize_dingtalk_text_content(text, bot_title))
            .map(|text| text.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    if (msg_type == "richtext" || msg_type == "rich_text")
        && let Some(rich_blocks) = value.pointer("/content/richText").and_then(Value::as_array)
    {
        let rich_text = rich_blocks
            .iter()
            .filter_map(|block| {
                let block_type = block
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .map(|ty| ty.to_ascii_lowercase())
                    .unwrap_or_default();
                if matches!(block_type.as_str(), "at" | "mention")
                    && block_targets_bot(block, robot_code, bot_title)
                {
                    return None;
                }
                if block_type != "text" {
                    return None;
                }
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| normalize_dingtalk_text_content(text, bot_title))
            })
            .collect::<Vec<_>>()
            .join("");
        let normalized_text = rich_text.trim();
        if !normalized_text.is_empty() {
            return Some(normalized_text.to_string());
        }

        let picture_count = rich_blocks
            .iter()
            .filter(|block| {
                block
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .map(|ty| ty.eq_ignore_ascii_case("picture"))
                    .unwrap_or(false)
            })
            .count();
        if picture_count > 0 {
            return Some(format!(
                "[DingTalk富文本消息]\n用户发送了 {picture_count} 张图片。请结合图片内容回答用户。"
            ));
        }
    }

    let fallback = match msg_type {
        "audio" | "voice" => {
            if let Some(recognition) = audio_recognition
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(recognition.to_string());
            }
            let duration = value
                .pointer("/audio/duration")
                .or_else(|| value.pointer("/voice/duration"))
                .and_then(Value::as_i64)
                .map(|seconds| format!("，时长约 {seconds} 秒"))
                .unwrap_or_default();
            format!(
                "[DingTalk语音消息]\n用户发送了一条语音消息{duration}。当前通道暂不支持直接转写原始语音内容，请引导用户补充文字摘要。"
            )
        }
        "picture" | "image" | "photo" => {
            let title = value
                .pointer("/picture/fileName")
                .or_else(|| value.pointer("/image/fileName"))
                .or_else(|| value.pointer("/photo/fileName"))
                .or_else(|| value.pointer("/content/fileName"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|name| format!("（{name}）"))
                .unwrap_or_default();
            format!("[DingTalk图片消息{title}]\n用户发送了一张图片。")
        }
        "video" => {
            let title = first_string_value(
                value,
                &["/video/fileName", "/video/videoName", "/content/fileName"],
            )
            .map(|name| format!("（{name}）"))
            .unwrap_or_default();
            format!("[DingTalk视频消息{title}]\n用户发送了一个视频。")
        }
        "file" | "document" | "doc" | "attachment" => {
            let title = first_string_value(
                value,
                &[
                    "/file/fileName",
                    "/document/fileName",
                    "/doc/fileName",
                    "/attachment/fileName",
                    "/content/fileName",
                ],
            )
            .map(|name| format!("（{name}）"))
            .unwrap_or_default();
            format!("[DingTalk文件消息{title}]\n用户发送了一个文件。")
        }
        other => format!(
            "[DingTalk非文本消息]\n用户发送了类型为 `{other}` 的消息。当前通道仅保证文本可直接处理，请引导用户补充文字内容。"
        ),
    };

    Some(fallback)
}

fn group_message_targets_bot(
    value: &Value,
    msg_type: &str,
    robot_code: &str,
    bot_title: &str,
) -> bool {
    if has_structured_bot_mention(value, robot_code, bot_title) {
        return true;
    }

    if (msg_type == "richtext" || msg_type == "rich_text")
        && value
            .pointer("/content/richText")
            .and_then(Value::as_array)
            .is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    let block_type = block
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(|ty| ty.to_ascii_lowercase())
                        .unwrap_or_default();
                    matches!(block_type.as_str(), "at" | "mention")
                        && block_targets_bot(block, robot_code, bot_title)
                })
            })
    {
        return true;
    }

    let Some(bot_title) = normalized_match_target(bot_title) else {
        return false;
    };

    first_string_value(
        value,
        &[
            "/text/content",
            "/content/text",
            "/content/title",
            "/content/markdown",
        ],
    )
    .is_some_and(|text| contains_textual_bot_mention(&text, &bot_title))
}

fn has_structured_bot_mention(value: &Value, robot_code: &str, bot_title: &str) -> bool {
    [
        "/atUsers",
        "/atOpenIds",
        "/atMobiles",
        "/text/atUsers",
        "/text/atOpenIds",
        "/text/atMobiles",
        "/content/atUsers",
        "/content/atOpenIds",
        "/content/atMobiles",
    ]
    .iter()
    .filter_map(|pointer| value.pointer(pointer))
    .any(|node| node_contains_bot_target(node, robot_code, bot_title))
}

fn block_targets_bot(block: &Value, robot_code: &str, bot_title: &str) -> bool {
    node_contains_bot_target(block, robot_code, bot_title)
}

fn node_contains_bot_target(node: &Value, robot_code: &str, bot_title: &str) -> bool {
    let targets = [
        normalized_match_target(robot_code),
        normalized_match_target(bot_title),
    ];
    collect_strings(node)
        .into_iter()
        .filter_map(|value| normalized_match_target(value.as_str()))
        .any(|candidate| targets.iter().flatten().any(|target| candidate == *target))
}

fn collect_strings(node: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match node {
        Value::String(text) => out.push(text.to_string()),
        Value::Array(items) => {
            for item in items {
                out.extend(collect_strings(item));
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                out.extend(collect_strings(value));
            }
        }
        _ => {}
    }
    out
}

fn normalized_match_target(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_start_matches('@')
        .trim_start_matches('＠');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_ascii_lowercase())
    }
}

fn contains_textual_bot_mention(text: &str, bot_title: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains(format!("@{bot_title}").as_str())
        || lowered.contains(format!("＠{bot_title}").as_str())
}

pub(super) fn normalize_dingtalk_text_content(text: &str, bot_title: &str) -> String {
    let Some(bot_title) = normalized_match_target(bot_title) else {
        return text.to_string();
    };
    let lowered = text.to_ascii_lowercase();
    let mentions = [format!("@{bot_title}"), format!("＠{bot_title}")];
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;

    while cursor < text.len() {
        let haystack = &lowered[cursor..];
        let matched = mentions
            .iter()
            .filter_map(|mention| haystack.find(mention).map(|offset| (offset, mention.len())))
            .min_by_key(|(offset, _)| *offset);
        let Some((offset, len)) = matched else {
            output.push_str(&text[cursor..]);
            break;
        };
        let start = cursor + offset;
        output.push_str(&text[cursor..start]);
        output.push(' ');
        cursor = start + len;
    }

    output.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn extract_dingtalk_media_references(
    value: &Value,
    msg_type: &str,
    event_id: &str,
) -> Vec<MediaReference> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "dingtalk.msg_type".to_string(),
        Value::String(msg_type.to_string()),
    );

    match msg_type {
        "richtext" | "rich_text" => {
            let Some(rich_blocks) = value.pointer("/content/richText").and_then(Value::as_array)
            else {
                return Vec::new();
            };

            rich_blocks
                .iter()
                .enumerate()
                .filter_map(|(index, block)| {
                    let block_type = block
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(|ty| ty.to_ascii_lowercase())
                        .unwrap_or_default();
                    if !is_supported_richtext_media_type(&block_type) {
                        return None;
                    }

                    let filename = block
                        .get("fileName")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    let mut item_metadata = metadata.clone();
                    item_metadata.insert("dingtalk.richtext_index".to_string(), Value::from(index));
                    item_metadata.insert(
                        "dingtalk.richtext_block_type".to_string(),
                        Value::String(block_type),
                    );
                    let mime_type =
                        first_object_string_value(block, &["mimeType", "mime_type", "contentType"]);
                    let file_extension =
                        first_object_string_value(block, &["fileType", "fileExt", "extension"]);
                    attach_declared_media_metadata(
                        &mut item_metadata,
                        mime_type.as_deref(),
                        file_extension.as_deref(),
                        "dingtalk.declared_mime_type",
                        "dingtalk.file_extension",
                    );
                    if let Some(download_code) = block
                        .get("downloadCode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|code| !code.is_empty())
                    {
                        item_metadata.insert(
                            "dingtalk.download_code".to_string(),
                            Value::String(download_code.to_string()),
                        );
                    }
                    if let Some(picture_download_code) = block
                        .get("pictureDownloadCode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|code| !code.is_empty())
                    {
                        item_metadata.insert(
                            "dingtalk.picture_download_code".to_string(),
                            Value::String(picture_download_code.to_string()),
                        );
                    }
                    Some(build_media_reference(
                        MediaSourceKind::ChannelInbound,
                        event_id,
                        filename,
                        mime_type,
                        item_metadata,
                    ))
                })
                .collect()
        }
        "audio" | "voice" => {
            if let Some(duration) = value
                .pointer("/audio/duration")
                .or_else(|| value.pointer("/voice/duration"))
                .and_then(Value::as_i64)
            {
                metadata.insert(
                    "dingtalk.duration_seconds".to_string(),
                    Value::from(duration),
                );
            }
            if let Some(download_code) = value
                .pointer("/audio/downloadCode")
                .or_else(|| value.pointer("/voice/downloadCode"))
                .or_else(|| value.pointer("/content/downloadCode"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|code| !code.is_empty())
            {
                metadata.insert(
                    "dingtalk.download_code".to_string(),
                    Value::String(download_code.to_string()),
                );
            }
            if let Some(picture_download_code) = value
                .pointer("/audio/pictureDownloadCode")
                .or_else(|| value.pointer("/voice/pictureDownloadCode"))
                .or_else(|| value.pointer("/content/pictureDownloadCode"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|code| !code.is_empty())
            {
                metadata.insert(
                    "dingtalk.picture_download_code".to_string(),
                    Value::String(picture_download_code.to_string()),
                );
            }
            let filename = value
                .pointer("/audio/fileName")
                .or_else(|| value.pointer("/voice/fileName"))
                .or_else(|| value.pointer("/content/fileName"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let mime_type = first_string_value(
                value,
                &[
                    "/audio/mimeType",
                    "/audio/contentType",
                    "/voice/mimeType",
                    "/voice/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            );
            let file_extension = first_string_value(
                value,
                &[
                    "/audio/fileType",
                    "/audio/fileExt",
                    "/audio/extension",
                    "/voice/fileType",
                    "/voice/fileExt",
                    "/voice/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            );
            attach_declared_media_metadata(
                &mut metadata,
                mime_type.as_deref(),
                file_extension.as_deref(),
                "dingtalk.declared_mime_type",
                "dingtalk.file_extension",
            );
            vec![build_media_reference(
                MediaSourceKind::ChannelInbound,
                event_id,
                filename,
                mime_type,
                metadata,
            )]
        }
        "video" => build_media_references(
            &metadata,
            event_id,
            first_string_value(
                value,
                &["/video/fileName", "/video/videoName", "/content/fileName"],
            ),
            first_string_value(
                value,
                &[
                    "/video/downloadCode",
                    "/video/videoDownloadCode",
                    "/content/downloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/video/pictureDownloadCode",
                    "/video/coverDownloadCode",
                    "/content/pictureDownloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/video/mimeType",
                    "/video/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/video/fileType",
                    "/video/fileExt",
                    "/video/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            ),
        ),
        "picture" | "image" | "photo" => {
            let filename = value
                .pointer("/picture/fileName")
                .or_else(|| value.pointer("/image/fileName"))
                .or_else(|| value.pointer("/photo/fileName"))
                .or_else(|| value.pointer("/content/fileName"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let mime_type = first_string_value(
                value,
                &[
                    "/picture/mimeType",
                    "/picture/contentType",
                    "/image/mimeType",
                    "/image/contentType",
                    "/photo/mimeType",
                    "/photo/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            );
            let file_extension = first_string_value(
                value,
                &[
                    "/picture/fileType",
                    "/picture/fileExt",
                    "/picture/extension",
                    "/image/fileType",
                    "/image/fileExt",
                    "/image/extension",
                    "/photo/fileType",
                    "/photo/fileExt",
                    "/photo/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            );
            if let Some(download_code) = value
                .pointer("/picture/downloadCode")
                .or_else(|| value.pointer("/image/downloadCode"))
                .or_else(|| value.pointer("/content/downloadCode"))
                .and_then(Value::as_str)
                .filter(|code| !code.trim().is_empty())
            {
                metadata.insert(
                    "dingtalk.download_code".to_string(),
                    Value::String(download_code.to_string()),
                );
            }
            if let Some(picture_download_code) = value
                .pointer("/picture/pictureDownloadCode")
                .or_else(|| value.pointer("/image/pictureDownloadCode"))
                .or_else(|| value.pointer("/content/pictureDownloadCode"))
                .and_then(Value::as_str)
                .filter(|code| !code.trim().is_empty())
            {
                metadata.insert(
                    "dingtalk.picture_download_code".to_string(),
                    Value::String(picture_download_code.to_string()),
                );
            }
            attach_declared_media_metadata(
                &mut metadata,
                mime_type.as_deref(),
                file_extension.as_deref(),
                "dingtalk.declared_mime_type",
                "dingtalk.file_extension",
            );
            vec![build_media_reference(
                MediaSourceKind::ChannelInbound,
                event_id,
                filename,
                mime_type,
                metadata,
            )]
        }
        "file" | "document" | "doc" | "attachment" => build_media_references(
            &metadata,
            event_id,
            first_string_value(
                value,
                &[
                    "/file/fileName",
                    "/document/fileName",
                    "/doc/fileName",
                    "/attachment/fileName",
                    "/content/fileName",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/downloadCode",
                    "/document/downloadCode",
                    "/doc/downloadCode",
                    "/attachment/downloadCode",
                    "/content/downloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/pictureDownloadCode",
                    "/document/pictureDownloadCode",
                    "/doc/pictureDownloadCode",
                    "/attachment/pictureDownloadCode",
                    "/content/pictureDownloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/mimeType",
                    "/file/contentType",
                    "/document/mimeType",
                    "/document/contentType",
                    "/doc/mimeType",
                    "/doc/contentType",
                    "/attachment/mimeType",
                    "/attachment/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/fileType",
                    "/file/fileExt",
                    "/file/extension",
                    "/document/fileType",
                    "/document/fileExt",
                    "/document/extension",
                    "/doc/fileType",
                    "/doc/fileExt",
                    "/doc/extension",
                    "/attachment/fileType",
                    "/attachment/fileExt",
                    "/attachment/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            ),
        ),
        _ => Vec::new(),
    }
}

fn is_supported_richtext_media_type(block_type: &str) -> bool {
    matches!(
        block_type,
        "picture" | "image" | "video" | "file" | "document" | "doc" | "attachment"
    )
}

fn build_media_references(
    metadata: &BTreeMap<String, Value>,
    event_id: &str,
    filename: Option<String>,
    download_code: Option<String>,
    picture_download_code: Option<String>,
    mime_type: Option<String>,
    file_extension: Option<String>,
) -> Vec<MediaReference> {
    let mut metadata = metadata.clone();
    if let Some(download_code) = download_code {
        metadata.insert(
            "dingtalk.download_code".to_string(),
            Value::String(download_code),
        );
    }
    if let Some(picture_download_code) = picture_download_code {
        metadata.insert(
            "dingtalk.picture_download_code".to_string(),
            Value::String(picture_download_code),
        );
    }
    attach_declared_media_metadata(
        &mut metadata,
        mime_type.as_deref(),
        file_extension.as_deref(),
        "dingtalk.declared_mime_type",
        "dingtalk.file_extension",
    );
    vec![build_media_reference(
        MediaSourceKind::ChannelInbound,
        event_id,
        filename,
        mime_type,
        metadata,
    )]
}

fn extract_audio_recognition_text(value: &Value) -> Option<String> {
    value
        .pointer("/content/recognition")
        .or_else(|| value.pointer("/audio/recognition"))
        .or_else(|| value.pointer("/voice/recognition"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn resolve_download_code_candidates(
    metadata: &BTreeMap<String, Value>,
) -> Vec<(String, &'static str)> {
    resolve_metadata_value_candidates(
        metadata,
        &[
            ("dingtalk.download_code", "download_code"),
            ("dingtalk.picture_download_code", "picture_download_code"),
        ],
    )
}

pub(super) fn is_sender_allowed(allowlist: &[String], sender_id: &str) -> bool {
    if allowlist.is_empty() {
        return true;
    }

    allowlist
        .iter()
        .any(|entry| entry == "*" || entry == sender_id)
}

pub(super) fn resolve_channel_card(output: &ChannelResponse) -> Option<ImCard> {
    resolve_im_card(output)
}

#[cfg(test)]
pub(super) fn resolve_approval_card(output: &ChannelResponse) -> Option<ImCard> {
    resolve_channel_card(output).filter(|card| matches!(card.kind, ImCardKind::Approval))
}

pub(super) fn build_im_card_action_card_body(card: &ImCard) -> String {
    match card.kind {
        ImCardKind::Approval => build_approval_action_card_body(card),
        ImCardKind::QuestionSingleSelect => build_question_single_select_action_card_body(card),
    }
}

pub(super) fn build_im_card_action_buttons(card: &ImCard) -> Vec<(String, String)> {
    card.actions
        .iter()
        .filter_map(|action| {
            let title = action.label_or_default().to_string();
            match action.kind {
                ImCardActionKind::Approve | ImCardActionKind::Reject => {
                    let approval_id = action.approval_id()?;
                    let verb = action.kind.approval_verb()?;
                    Some((title, dingtalk_command_action_url(verb, approval_id)))
                }
                ImCardActionKind::SubmitCommand => action
                    .to_runtime_command()
                    .map(|command| (title, dingtalk_command_message_url(&command))),
                ImCardActionKind::OpenUrl => action
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|url| (title, url.to_string())),
            }
        })
        .collect()
}

pub(super) fn build_approval_action_card_body(card: &ImCard) -> String {
    let approval_id = card.approval_id().unwrap_or_default();
    let mut sections = base_card_sections(card, "需要审批");
    sections.push(format!("审批单: `{approval_id}`"));
    sections.push("点击按钮后将发送审批指令。".to_string());
    sections.join("\n\n---\n\n")
}

fn build_question_single_select_action_card_body(card: &ImCard) -> String {
    let mut sections = base_card_sections(card, "问题");
    let options = card
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| format!("{}. {}", index + 1, action.label_or_default()))
        .collect::<Vec<_>>();
    if !options.is_empty() {
        sections.push(format!(
            "**选项**\n\n{}",
            escape_markdown_for_action_card(&options.join("\n"))
        ));
    }
    sections.join("\n\n---\n\n")
}

fn base_card_sections(card: &ImCard, fallback_title: &str) -> Vec<String> {
    let mut sections = vec![format!(
        "### {}",
        escape_markdown_for_action_card(card.title_or(fallback_title))
    )];
    if let Some(command_preview) = card.command_preview() {
        sections.push(format!(
            "**待执行命令**\n\n`{}`",
            escape_markdown_for_action_card(command_preview)
        ));
    }
    let escaped_content = escape_markdown_for_action_card(card.body_or(card.fallback_text_or("")));
    if !escaped_content.trim().is_empty() {
        sections.push(escaped_content);
    }
    sections
}

fn dingtalk_command_message_url(command: &str) -> String {
    format!(
        "dtmd://dingtalkclient/sendMessage?content={}",
        urlencoding::encode(command)
    )
}

#[cfg(test)]
pub(super) fn extract_shell_approval_id(content: &str) -> Option<String> {
    resolve_im_card(&ChannelResponse {
        content: content.to_string(),
        reasoning: None,
        metadata: BTreeMap::new(),
        attachments: Vec::new(),
    })
    .and_then(|card| card.approval_id().map(ToOwned::to_owned))
}

fn escape_markdown_for_action_card(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
