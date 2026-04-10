//! Browser chat UI for Klaw gateway (egui + WebSocket).
//!
//! Refresh embedded assets from the workspace root: `make webui-wasm`

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) use klaw_ui_kit::ThemeMode;

#[cfg(test)]
use std::collections::VecDeque;

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
    pub(crate) fn status_text(&self) -> &'static str {
        match self {
            Self::Disconnected => "Offline",
            Self::Connecting => "Connecting…",
            Self::Connected => "Ready",
            Self::Error(_) => "Connection error",
        }
    }

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
pub(crate) fn toolbar_title() -> &'static str {
    "Klaw Web Chat"
}

#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn normalize_gateway_token_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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
    use std::collections::VecDeque;

    use super::{
        ConnectionState, MessageRole, StreamMessageAction, ThemeMode, classify_message_role,
        classify_stream_message_action, normalize_gateway_token_input, toolbar_title,
    };

    #[test]
    fn toolbar_title_matches_chat_product() {
        assert_eq!(toolbar_title(), "Klaw Web Chat");
    }

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
    fn theme_mode_labels_match_gui_copy() {
        assert_eq!(ThemeMode::System.label(), "System");
        assert_eq!(ThemeMode::Light.label(), "Light");
        assert_eq!(ThemeMode::Dark.label(), "Dark");
    }
}
