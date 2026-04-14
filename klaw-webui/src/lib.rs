//! Browser chat UI for Klaw gateway (egui + WebSocket).
//!
//! Refresh embedded assets from the workspace root: `make webui-wasm`

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) use klaw_ui_kit::ThemeMode;

#[cfg(any(test, target_arch = "wasm32"))]
use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::VecDeque;
#[cfg(any(test, target_arch = "wasm32"))]
use std::ops::Range;

#[cfg(any(test, target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};
#[cfg(any(test, target_arch = "wasm32"))]
use serde_json::{Value, json};

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImCardKind {
    Approval,
    QuestionSingleSelect,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImCardActionKind {
    Approve,
    Reject,
    OpenUrl,
    SubmitCommand,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl ImCardActionKind {
    fn default_label(&self) -> &'static str {
        match self {
            Self::Approve => "Approve",
            Self::Reject => "Reject",
            Self::OpenUrl => "Open",
            Self::SubmitCommand => "Select",
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImCardAction {
    pub(crate) kind: ImCardActionKind,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) value: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) command: Option<String>,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl ImCardAction {
    fn label_or_default(&self) -> &str {
        self.label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.kind.default_label())
    }

    fn approval_id(&self) -> Option<&str> {
        match self.kind {
            ImCardActionKind::Approve | ImCardActionKind::Reject => self
                .value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            ImCardActionKind::OpenUrl | ImCardActionKind::SubmitCommand => None,
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImCard {
    pub(crate) kind: ImCardKind,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) actions: Vec<ImCardAction>,
    #[serde(default)]
    pub(crate) fallback_text: Option<String>,
    #[serde(default)]
    pub(crate) metadata: BTreeMap<String, Value>,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl ImCard {
    pub(crate) fn title_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback)
    }

    pub(crate) fn body_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        let body = self.body.trim();
        if body.is_empty() { fallback } else { body }
    }

    pub(crate) fn fallback_text_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.fallback_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback)
    }

    pub(crate) fn approval_id(&self) -> Option<&str> {
        self.actions
            .iter()
            .find_map(ImCardAction::approval_id)
            .or_else(|| {
                self.metadata
                    .get("approval_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
    }

    pub(crate) fn command_preview(&self) -> Option<&str> {
        self.metadata
            .get("command_preview")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn resolve_im_card(content: &str, metadata: &BTreeMap<String, Value>) -> Option<ImCard> {
    metadata
        .get("im.card")
        .cloned()
        .and_then(|value| serde_json::from_value::<ImCard>(value).ok())
        .or_else(|| resolve_approval_card(content, metadata))
}

#[cfg(any(test, target_arch = "wasm32"))]
fn resolve_approval_card(content: &str, metadata: &BTreeMap<String, Value>) -> Option<ImCard> {
    let approval_id = metadata
        .get("approval.id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            metadata
                .get("approval.signal")
                .and_then(Value::as_object)
                .and_then(|value| value.get("approval_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| extract_shell_approval_id(content))?;
    let mut card_metadata = BTreeMap::new();
    card_metadata.insert(
        "approval_id".to_string(),
        Value::String(approval_id.clone()),
    );
    if let Some(command_preview) = metadata
        .get("approval.signal")
        .and_then(Value::as_object)
        .and_then(|value| value.get("command_preview"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        card_metadata.insert(
            "command_preview".to_string(),
            Value::String(command_preview.to_string()),
        );
    }
    Some(ImCard {
        kind: ImCardKind::Approval,
        title: None,
        body: content.trim().to_string(),
        actions: vec![
            ImCardAction {
                kind: ImCardActionKind::Approve,
                label: None,
                value: Some(approval_id.clone()),
                url: None,
                command: None,
            },
            ImCardAction {
                kind: ImCardActionKind::Reject,
                label: None,
                value: Some(approval_id),
                url: None,
                command: None,
            },
        ],
        fallback_text: (!content.trim().is_empty()).then(|| content.trim().to_string()),
        metadata: card_metadata,
    })
}

#[cfg(any(test, target_arch = "wasm32"))]
fn extract_shell_approval_id(content: &str) -> Option<String> {
    let marker = "approval_id=";
    if let Some(idx) = content.find(marker) {
        let rest = &content[idx + marker.len()..];
        let token = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            .collect::<String>();
        if !token.is_empty() {
            return Some(token);
        }
    }
    None
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

#[cfg(any(test, target_arch = "wasm32"))]
impl ConnectionState {
    pub(crate) fn composer_hint_text(&self) -> &'static str {
        match self {
            Self::Connected => "Message Klaw…",
            Self::Connecting => "Connecting to Klaw…",
            Self::Disconnected => "Reconnect to message Klaw…",
            Self::Error(_) => "Fix the connection to keep chatting…",
        }
    }

    pub(crate) fn can_send(&self) -> bool {
        matches!(self, Self::Connected)
    }

    pub(crate) fn empty_state_copy(&self) -> EmptyStateCopy {
        match self {
            Self::Connected => EmptyStateCopy {
                title: "Start a conversation with Klaw".to_string(),
                body: "Send a message below to begin this chat.".to_string(),
            },
            Self::Connecting => EmptyStateCopy {
                title: "Connecting to Klaw".to_string(),
                body: "Waiting for the chat room to come online.".to_string(),
            },
            Self::Disconnected => EmptyStateCopy {
                title: "Reconnect to Klaw".to_string(),
                body: "Reconnect from the toolbar, then send your next message.".to_string(),
            },
            Self::Error(error) => EmptyStateCopy {
                title: "Connection error".to_string(),
                body: format!("Klaw could not keep the chat connection alive: {error}"),
            },
        }
    }
}

#[cfg(test)]
impl ConnectionState {
    fn status_text(&self) -> &'static str {
        match self {
            Self::Disconnected => "Offline",
            Self::Connecting => "Connecting…",
            Self::Connected => "Ready",
            Self::Error(_) => "Connection error",
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageRole {
    System,
    Assistant,
    User,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamMessageAction {
    IgnoreEmpty,
    ReplaceLastAssistant,
    PushAssistant,
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) struct EmptyStateCopy {
    pub(crate) title: String,
    pub(crate) body: String,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionListEntry {
    pub(crate) session_key: String,
    pub(crate) title: String,
    pub(crate) created_at_ms: i64,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkspaceSessionEntry {
    pub(crate) session_key: String,
    pub(crate) title: String,
    pub(crate) created_at_ms: i64,
    #[serde(default)]
    pub(crate) model_provider: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProviderCatalogEntry {
    pub(crate) id: String,
    pub(crate) default_model: String,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProviderCatalog {
    #[serde(default)]
    pub(crate) default_provider: Option<String>,
    #[serde(default)]
    pub(crate) providers: Vec<ProviderCatalogEntry>,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolvedSessionRoute {
    pub(crate) model_provider: String,
    pub(crate) model: String,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SlashCommandCompletion {
    pub(crate) command: &'static str,
    pub(crate) insert_text: &'static str,
    pub(crate) description: &'static str,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveSlashCommand {
    pub(crate) replace_range: Range<usize>,
    pub(crate) query: String,
}

#[cfg(any(test, target_arch = "wasm32"))]
const SLASH_COMMANDS: [SlashCommandCompletion; 9] = [
    SlashCommandCompletion {
        command: "/new",
        insert_text: "/new",
        description: "Start a new session context",
    },
    SlashCommandCompletion {
        command: "/start",
        insert_text: "/start",
        description: "Alias of /new for a fresh session",
    },
    SlashCommandCompletion {
        command: "/help",
        insert_text: "/help",
        description: "Show available session commands",
    },
    SlashCommandCompletion {
        command: "/stop",
        insert_text: "/stop",
        description: "Stop the current turn without calling the agent",
    },
    SlashCommandCompletion {
        command: "/model_provider",
        insert_text: "/model_provider ",
        description: "List or switch the provider for this session",
    },
    SlashCommandCompletion {
        command: "/model",
        insert_text: "/model ",
        description: "Show or update the current session model",
    },
    SlashCommandCompletion {
        command: "/approve",
        insert_text: "/approve ",
        description: "Approve a pending tool action",
    },
    SlashCommandCompletion {
        command: "/reject",
        insert_text: "/reject ",
        description: "Reject a pending tool action",
    },
    SlashCommandCompletion {
        command: "/card_answer",
        insert_text: "/card_answer ",
        description: "Answer an interactive question card",
    },
];

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) const fn slash_command_catalog() -> &'static [SlashCommandCompletion] {
    &SLASH_COMMANDS
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn resolve_session_route_inputs(
    model_provider: Option<&str>,
    model: Option<&str>,
    catalog: &ProviderCatalog,
) -> ResolvedSessionRoute {
    let requested_provider = model_provider
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let fallback_provider = catalog
        .default_provider
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| catalog.providers.first().map(|entry| entry.id.clone()))
        .unwrap_or_default();
    let model_provider = requested_provider.unwrap_or(fallback_provider);
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            catalog
                .providers
                .iter()
                .find(|entry| entry.id == model_provider)
                .map(|entry| entry.default_model.clone())
        })
        .unwrap_or_default();
    ResolvedSessionRoute {
        model_provider,
        model,
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn build_websocket_submit_params(
    session_key: &str,
    input: &str,
    stream: bool,
    archive_id: Option<&str>,
    model_provider: &str,
    model: &str,
    metadata: Option<&BTreeMap<String, Value>>,
) -> Value {
    let mut params = json!({
        "session_key": session_key,
        "chat_id": session_key,
        "input": input,
        "stream": stream,
        "model_provider": model_provider,
        "model": model,
    });
    if let Some(archive_id) = archive_id.filter(|value| !value.is_empty()) {
        params["archive_id"] = json!(archive_id);
    }
    if let Some(metadata) = metadata.filter(|metadata| !metadata.is_empty()) {
        params["metadata"] = json!(metadata);
    }
    params
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn detect_active_slash_command(
    text: &str,
    cursor_char_index: usize,
) -> Option<ActiveSlashCommand> {
    let cursor_byte_index = char_index_to_byte_index(text, cursor_char_index);
    let start_byte_index = text[..cursor_byte_index]
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let end_byte_index = text[cursor_byte_index..]
        .find(char::is_whitespace)
        .map(|offset| cursor_byte_index + offset)
        .unwrap_or(text.len());
    let token = &text[start_byte_index..end_byte_index];
    if !token.starts_with('/') {
        return None;
    }
    if token.contains('\n') {
        return None;
    }
    Some(ActiveSlashCommand {
        replace_range: start_byte_index..end_byte_index,
        query: token.trim_start_matches('/').to_string(),
    })
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn slash_command_matches(query: &str) -> Vec<SlashCommandCompletion> {
    let normalized_query = query.trim().to_ascii_lowercase().replace('-', "_");
    slash_command_catalog()
        .iter()
        .copied()
        .filter(|completion| {
            if normalized_query.is_empty() {
                return true;
            }
            let normalized_command = completion
                .command
                .trim_start_matches('/')
                .to_ascii_lowercase();
            normalized_command.starts_with(&normalized_query)
                || normalized_command.contains(&normalized_query)
        })
        .collect()
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn apply_slash_completion(
    text: &mut String,
    replace_range: Range<usize>,
    completion: SlashCommandCompletion,
) -> usize {
    text.replace_range(replace_range.clone(), completion.insert_text);
    text[..replace_range.start + completion.insert_text.len()]
        .chars()
        .count()
}

#[cfg(any(test, target_arch = "wasm32"))]
fn char_index_to_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArchiveUploadResponse {
    pub(crate) success: bool,
    pub(crate) record: Option<ArchiveRecord>,
    pub(crate) error: Option<String>,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArchiveRecord {
    pub(crate) id: String,
    pub(crate) original_filename: Option<String>,
    pub(crate) mime_type: Option<String>,
    pub(crate) size_bytes: i64,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageMode {
    ConnectionGuide,
    LoadingWorkspace,
    Workspace,
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn normalize_gateway_token_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn should_prompt_for_gateway_token_before_connect(token: Option<&str>) -> bool {
    token.and_then(normalize_gateway_token_input).is_none()
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn connection_action_label(connection_state: &ConnectionState) -> &'static str {
    match connection_state {
        ConnectionState::Connected => "Reconnect",
        ConnectionState::Connecting => "Connect",
        ConnectionState::Disconnected => "Connect",
        ConnectionState::Error(_) => "Reconnect",
    }
}

#[cfg(test)]
pub(crate) fn session_card_activity_label(_is_active: bool) -> Option<&'static str> {
    None
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn delete_confirmation_body(agent_name: &str) -> String {
    format!("Delete agent '{agent_name}' permanently? This cannot be undone.")
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn resolve_gateway_token(
    query_token: Option<String>,
    persisted_token: Option<String>,
) -> Option<String> {
    query_token
        .as_deref()
        .and_then(normalize_gateway_token_input)
        .or_else(|| {
            persisted_token
                .as_deref()
                .and_then(normalize_gateway_token_input)
        })
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn should_activate_session_window(
    window_contains_pointer: bool,
    primary_pointer_pressed: bool,
) -> bool {
    window_contains_pointer && primary_pointer_pressed
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn derive_page_mode(
    connection_state: &ConnectionState,
    workspace_loaded: bool,
) -> PageMode {
    match connection_state {
        ConnectionState::Connected if workspace_loaded => PageMode::Workspace,
        ConnectionState::Connected => PageMode::LoadingWorkspace,
        _ => PageMode::ConnectionGuide,
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn sort_session_entries_by_created_at_desc(entries: &mut [SessionListEntry]) {
    entries.sort_by(|left, right| {
        right
            .created_at_ms
            .cmp(&left.created_at_ms)
            .then_with(|| right.session_key.cmp(&left.session_key))
    });
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn classify_stream_message_action(
    last_role: Option<MessageRole>,
    active_stream_request_id: Option<&str>,
    request_id: Option<&str>,
    content: &str,
) -> StreamMessageAction {
    if content.is_empty() {
        return StreamMessageAction::IgnoreEmpty;
    }

    if request_id.is_some()
        && request_id == active_stream_request_id
        && last_role == Some(MessageRole::Assistant)
    {
        return StreamMessageAction::ReplaceLastAssistant;
    }

    StreamMessageAction::PushAssistant
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn should_register_non_stream_fade(
    role: MessageRole,
    streamed: bool,
    history_event: bool,
    content: &str,
) -> bool {
    matches!(role, MessageRole::Assistant) && !streamed && !history_event && !content.is_empty()
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn attachment_action_in_progress(selecting_file: bool, uploading_file: bool) -> bool {
    selecting_file || uploading_file
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn can_trigger_file_picker(
    can_send: bool,
    selecting_file: bool,
    uploading_file: bool,
) -> bool {
    can_send && !attachment_action_in_progress(selecting_file, uploading_file)
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn next_selected_archive_id_after_submit(
    selected_archive_id: Option<&str>,
    send_succeeded: bool,
) -> Option<String> {
    if send_succeeded {
        None
    } else {
        selected_archive_id.map(str::to_owned)
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn should_cancel_file_picker_selection(
    picker_took_focus: bool,
    has_focus: bool,
    grace_elapsed: bool,
) -> bool {
    picker_took_focus && has_focus && grace_elapsed
}

#[cfg(test)]
pub(crate) fn classify_message_role(
    pending_local_echoes: &mut VecDeque<String>,
    text: &str,
) -> MessageRole {
    match pending_local_echoes.front() {
        Some(expected) if expected == text => {
            pending_local_echoes.pop_front();
            MessageRole::User
        }
        _ => MessageRole::Assistant,
    }
}

#[cfg(target_arch = "wasm32")]
mod web_chat;

#[cfg(target_arch = "wasm32")]
pub use web_chat::start_chat_ui;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::VecDeque;

    use serde_json::json;

    use super::{
        ArchiveRecord, ArchiveUploadResponse, ConnectionState, ImCardKind, MessageRole, PageMode,
        ProviderCatalog, ProviderCatalogEntry, ResolvedSessionRoute, SessionListEntry,
        StreamMessageAction, ThemeMode, apply_slash_completion, attachment_action_in_progress,
        build_websocket_submit_params, can_trigger_file_picker, classify_message_role,
        classify_stream_message_action, connection_action_label, delete_confirmation_body,
        derive_page_mode, detect_active_slash_command, next_selected_archive_id_after_submit,
        normalize_gateway_token_input, resolve_gateway_token, resolve_im_card,
        resolve_session_route_inputs, session_card_activity_label, should_activate_session_window,
        should_cancel_file_picker_selection, should_prompt_for_gateway_token_before_connect,
        should_register_non_stream_fade, slash_command_matches,
        sort_session_entries_by_created_at_desc,
    };

    #[test]
    fn connected_state_uses_friendly_status_copy() {
        assert_eq!(ConnectionState::Connected.status_text(), "Ready");
    }

    #[test]
    fn connecting_state_reports_connecting_status() {
        assert_eq!(ConnectionState::Connecting.status_text(), "Connecting…");
    }

    #[test]
    fn disconnected_empty_state_invites_reconnect() {
        let copy = ConnectionState::Disconnected.empty_state_copy();
        assert_eq!(copy.title, "Reconnect to Klaw");
        assert!(copy.body.contains("Reconnect"));
    }

    #[test]
    fn error_empty_state_surfaces_context() {
        let copy = ConnectionState::Error("send failed".to_string()).empty_state_copy();
        assert_eq!(copy.title, "Connection error");
        assert!(copy.body.contains("send failed"));
    }

    #[test]
    fn disconnected_state_disables_sending_and_updates_hint() {
        assert!(!ConnectionState::Disconnected.can_send());
        assert_eq!(
            ConnectionState::Disconnected.composer_hint_text(),
            "Reconnect to message Klaw…"
        );
    }

    #[test]
    fn incoming_echo_matching_local_send_is_rendered_as_user_message() {
        let mut pending = VecDeque::from([String::from("hello from browser")]);
        let role = classify_message_role(&mut pending, "hello from browser");
        assert_eq!(role, MessageRole::User);
        assert!(pending.is_empty());
    }

    #[test]
    fn non_matching_incoming_text_is_rendered_as_assistant_message() {
        let mut pending = VecDeque::from([String::from("hello from browser")]);
        let role = classify_message_role(&mut pending, "hello from server");
        assert_eq!(role, MessageRole::Assistant);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn first_stream_snapshot_pushes_assistant_after_user_message() {
        let action = classify_stream_message_action(
            Some(MessageRole::User),
            Some("req-1"),
            Some("req-1"),
            "Hel",
        );
        assert_eq!(action, StreamMessageAction::PushAssistant);
    }

    #[test]
    fn later_stream_snapshot_replaces_existing_assistant_message() {
        let action = classify_stream_message_action(
            Some(MessageRole::Assistant),
            Some("req-1"),
            Some("req-1"),
            "Hello",
        );
        assert_eq!(action, StreamMessageAction::ReplaceLastAssistant);
    }

    #[test]
    fn empty_stream_snapshot_is_ignored() {
        let action = classify_stream_message_action(
            Some(MessageRole::Assistant),
            Some("req-1"),
            Some("req-1"),
            "",
        );
        assert_eq!(action, StreamMessageAction::IgnoreEmpty);
    }

    #[test]
    fn registers_fade_only_for_non_stream_assistant_messages() {
        assert!(should_register_non_stream_fade(
            MessageRole::Assistant,
            false,
            false,
            "final answer",
        ));
        assert!(!should_register_non_stream_fade(
            MessageRole::Assistant,
            true,
            false,
            "partial answer",
        ));
        assert!(!should_register_non_stream_fade(
            MessageRole::Assistant,
            false,
            true,
            "history answer",
        ));
        assert!(!should_register_non_stream_fade(
            MessageRole::User,
            false,
            false,
            "hello",
        ));
        assert!(!should_register_non_stream_fade(
            MessageRole::Assistant,
            false,
            false,
            "",
        ));
    }

    #[test]
    fn upload_response_deserializes_success_payload() {
        let response: ArchiveUploadResponse = serde_json::from_value(json!({
            "success": true,
            "record": {
                "id": "archive-1",
                "original_filename": "notes.txt",
                "mime_type": "text/plain",
                "size_bytes": 42
            },
            "error": null
        }))
        .expect("success payload should deserialize");

        assert!(response.success);
        assert_eq!(
            response.record,
            Some(ArchiveRecord {
                id: "archive-1".to_string(),
                original_filename: Some("notes.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                size_bytes: 42,
            })
        );
        assert_eq!(response.error, None);
    }

    #[test]
    fn upload_response_deserializes_error_payload() {
        let response: ArchiveUploadResponse = serde_json::from_value(json!({
            "success": false,
            "record": null,
            "error": "upload failed"
        }))
        .expect("error payload should deserialize");

        assert!(!response.success);
        assert_eq!(response.record, None);
        assert_eq!(response.error.as_deref(), Some("upload failed"));
    }

    #[test]
    fn system_role_stays_distinct_from_user_messages() {
        assert_ne!(MessageRole::System, MessageRole::User);
    }

    #[test]
    fn blank_gateway_token_is_treated_as_missing() {
        assert_eq!(normalize_gateway_token_input("   "), None);
    }

    #[test]
    fn gateway_token_input_is_trimmed_before_use() {
        assert_eq!(
            normalize_gateway_token_input("  secret-token  "),
            Some("secret-token".to_string())
        );
    }

    #[test]
    fn query_token_overrides_persisted_gateway_token() {
        assert_eq!(
            resolve_gateway_token(
                Some(" query-token ".to_string()),
                Some("persisted-token".to_string())
            ),
            Some("query-token".to_string())
        );
    }

    #[test]
    fn persisted_gateway_token_is_used_when_query_missing() {
        assert_eq!(
            resolve_gateway_token(None, Some(" persisted-token ".to_string())),
            Some("persisted-token".to_string())
        );
    }

    #[test]
    fn pointer_press_inside_window_activates_session() {
        assert!(should_activate_session_window(true, true));
        assert!(!should_activate_session_window(true, false));
        assert!(!should_activate_session_window(false, true));
    }

    #[test]
    fn attachment_action_in_progress_is_true_while_selecting_or_uploading() {
        assert!(attachment_action_in_progress(true, false));
        assert!(attachment_action_in_progress(false, true));
        assert!(!attachment_action_in_progress(false, false));
    }

    #[test]
    fn file_picker_requires_connected_idle_session() {
        assert!(can_trigger_file_picker(true, false, false));
        assert!(!can_trigger_file_picker(false, false, false));
        assert!(!can_trigger_file_picker(true, true, false));
        assert!(!can_trigger_file_picker(true, false, true));
    }

    #[test]
    fn failed_submit_keeps_selected_archive() {
        assert_eq!(
            next_selected_archive_id_after_submit(Some("archive-1"), false),
            Some("archive-1".to_string())
        );
    }

    #[test]
    fn successful_submit_clears_selected_archive() {
        assert_eq!(
            next_selected_archive_id_after_submit(Some("archive-1"), true),
            None
        );
        assert_eq!(next_selected_archive_id_after_submit(None, true), None);
    }

    #[test]
    fn file_picker_does_not_cancel_immediately_when_focus_returns() {
        assert!(!should_cancel_file_picker_selection(true, true, false));
    }

    #[test]
    fn file_picker_cancels_after_focus_returns_and_grace_expires() {
        assert!(should_cancel_file_picker_selection(true, true, true));
    }

    #[test]
    fn active_session_relies_on_selection_styling_not_badge_copy() {
        assert_eq!(session_card_activity_label(true), None);
        assert_eq!(session_card_activity_label(false), None);
    }

    #[test]
    fn theme_mode_labels_match_gui_copy() {
        assert_eq!(ThemeMode::System.label(), "System");
        assert_eq!(ThemeMode::Light.label(), "Light");
        assert_eq!(ThemeMode::Dark.label(), "Dark");
    }

    #[test]
    fn derive_page_mode_hides_workspace_until_bootstrap_is_ready() {
        assert_eq!(
            derive_page_mode(&ConnectionState::Disconnected, false),
            PageMode::ConnectionGuide
        );
        assert_eq!(
            derive_page_mode(&ConnectionState::Connected, false),
            PageMode::LoadingWorkspace
        );
        assert_eq!(
            derive_page_mode(&ConnectionState::Connected, true),
            PageMode::Workspace
        );
    }

    #[test]
    fn sort_sessions_by_created_at_desc_keeps_newest_first() {
        let mut sessions = vec![
            SessionListEntry {
                session_key: "websocket:1".to_string(),
                title: "Agent 1".to_string(),
                created_at_ms: 10,
            },
            SessionListEntry {
                session_key: "websocket:2".to_string(),
                title: "Agent 2".to_string(),
                created_at_ms: 20,
            },
        ];

        sort_session_entries_by_created_at_desc(&mut sessions);

        assert_eq!(sessions[0].session_key, "websocket:2");
        assert_eq!(sessions[1].session_key, "websocket:1");
    }

    #[test]
    fn connect_without_token_should_prompt_for_gateway_token() {
        assert!(should_prompt_for_gateway_token_before_connect(None));
        assert!(should_prompt_for_gateway_token_before_connect(Some("   ")));
        assert!(!should_prompt_for_gateway_token_before_connect(Some(
            "secret-token"
        )));
    }

    #[test]
    fn connection_action_uses_global_connection_wording() {
        assert_eq!(
            connection_action_label(&ConnectionState::Disconnected),
            "Connect"
        );
        assert_eq!(
            connection_action_label(&ConnectionState::Connecting),
            "Connect"
        );
        assert_eq!(
            connection_action_label(&ConnectionState::Connected),
            "Reconnect"
        );
        assert_eq!(
            connection_action_label(&ConnectionState::Error("oops".to_string())),
            "Reconnect"
        );
    }

    #[test]
    fn delete_confirmation_mentions_agent_name() {
        let body = delete_confirmation_body("My Agent");
        assert!(body.contains("My Agent"));
        assert!(body.contains("permanently"));
    }

    #[test]
    fn session_route_inputs_fall_back_to_catalog_defaults() {
        let catalog = ProviderCatalog {
            default_provider: Some("openai".to_string()),
            providers: vec![
                ProviderCatalogEntry {
                    id: "openai".to_string(),
                    default_model: "gpt-4.1-mini".to_string(),
                },
                ProviderCatalogEntry {
                    id: "anthropic".to_string(),
                    default_model: "claude-sonnet-4-5".to_string(),
                },
            ],
        };

        assert_eq!(
            resolve_session_route_inputs(None, None, &catalog),
            ResolvedSessionRoute {
                model_provider: "openai".to_string(),
                model: "gpt-4.1-mini".to_string(),
            }
        );
        assert_eq!(
            resolve_session_route_inputs(Some("anthropic"), None, &catalog),
            ResolvedSessionRoute {
                model_provider: "anthropic".to_string(),
                model: "claude-sonnet-4-5".to_string(),
            }
        );
    }

    #[test]
    fn websocket_submit_params_include_model_route() {
        let params = build_websocket_submit_params(
            "websocket:test",
            "hello",
            true,
            Some("archive-1"),
            "anthropic",
            "claude-sonnet-4-5",
            None,
        );

        assert_eq!(
            params
                .get("session_key")
                .and_then(serde_json::Value::as_str),
            Some("websocket:test")
        );
        assert_eq!(
            params.get("chat_id").and_then(serde_json::Value::as_str),
            Some("websocket:test")
        );
        assert_eq!(
            params.get("input").and_then(serde_json::Value::as_str),
            Some("hello")
        );
        assert_eq!(
            params.get("stream").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            params
                .get("model_provider")
                .and_then(serde_json::Value::as_str),
            Some("anthropic")
        );
        assert_eq!(
            params.get("model").and_then(serde_json::Value::as_str),
            Some("claude-sonnet-4-5")
        );
        assert_eq!(
            params.get("archive_id").and_then(serde_json::Value::as_str),
            Some("archive-1")
        );
    }

    #[test]
    fn websocket_submit_params_include_card_metadata() {
        let metadata = BTreeMap::from([(
            "webui.card.action".to_string(),
            serde_json::Value::Bool(true),
        )]);
        let params = build_websocket_submit_params(
            "websocket:test",
            "/approve approval-1",
            false,
            None,
            "anthropic",
            "claude-sonnet-4-5",
            Some(&metadata),
        );

        assert_eq!(
            params
                .get("metadata")
                .and_then(|value| value.get("webui.card.action"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn resolve_im_card_reads_explicit_question_card_metadata() {
        let metadata = BTreeMap::from([(
            "im.card".to_string(),
            serde_json::json!({
                "kind": "question_single_select",
                "title": "Choose",
                "body": "Pick one provider",
                "actions": [
                    {
                        "kind": "submit_command",
                        "label": "Anthropic",
                        "command": "/card_answer q-1 anthropic"
                    }
                ],
                "metadata": {
                    "question_id": "q-1"
                }
            }),
        )]);

        let card = resolve_im_card("ignored", &metadata).expect("card");
        assert_eq!(card.kind, ImCardKind::QuestionSingleSelect);
        assert_eq!(card.title.as_deref(), Some("Choose"));
        assert_eq!(
            card.metadata
                .get("question_id")
                .and_then(serde_json::Value::as_str),
            Some("q-1")
        );
    }

    #[test]
    fn resolve_im_card_falls_back_to_approval_signal_metadata() {
        let metadata = BTreeMap::from([(
            "approval.signal".to_string(),
            serde_json::json!({
                "approval_id": "approval-2",
                "command_preview": "git push origin HEAD"
            }),
        )]);

        let card = resolve_im_card("Approval required", &metadata).expect("card");
        assert_eq!(card.kind, ImCardKind::Approval);
        assert_eq!(card.approval_id(), Some("approval-2"));
        assert_eq!(card.command_preview(), Some("git push origin HEAD"));
    }

    #[test]
    fn detect_active_slash_command_at_input_start() {
        let detected = detect_active_slash_command("/mo", 3).expect("slash command");
        assert_eq!(detected.replace_range, 0..3);
        assert_eq!(detected.query, "mo");
    }

    #[test]
    fn detect_active_slash_command_inside_multiline_draft() {
        let text = "hello\n/model_provider";
        let cursor = text.chars().count();
        let detected = detect_active_slash_command(text, cursor).expect("slash command");
        assert_eq!(detected.query, "model_provider");
    }

    #[test]
    fn slash_command_matches_filter_known_commands() {
        let matched = slash_command_matches("mod");
        assert!(matched.iter().any(|item| item.command == "/model"));
        assert!(matched.iter().any(|item| item.command == "/model_provider"));
    }

    #[test]
    fn apply_slash_completion_replaces_current_token() {
        let mut draft = "/mod".to_string();
        let completion = slash_command_matches("mod")
            .into_iter()
            .find(|item| item.command == "/model")
            .expect("model command");
        let cursor = apply_slash_completion(&mut draft, 0..4, completion);
        assert_eq!(draft, "/model ");
        assert_eq!(cursor, "/model ".chars().count());
    }
}
