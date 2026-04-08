use std::collections::VecDeque;

pub(crate) const CHAT_COLUMN_MAX_WIDTH: f32 = 760.0;
pub(crate) const COMPOSER_MAX_WIDTH: f32 = 760.0;
pub(crate) const BUBBLE_MAX_WIDTH: f32 = 520.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionViewState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageRole {
    System,
    Assistant,
    User,
}

pub(crate) struct EmptyStateCopy {
    pub(crate) title: String,
    pub(crate) body: String,
}

pub(crate) fn toolbar_title() -> &'static str {
    "Klaw Web Chat"
}

pub(crate) fn status_text(state: &ConnectionViewState) -> String {
    match state {
        ConnectionViewState::Disconnected => "Offline".to_string(),
        ConnectionViewState::Connecting => "Connecting…".to_string(),
        ConnectionViewState::Connected => "Ready".to_string(),
        ConnectionViewState::Error(_) => "Connection error".to_string(),
    }
}

pub(crate) fn empty_state_copy(state: &ConnectionViewState) -> EmptyStateCopy {
    match state {
        ConnectionViewState::Connected => EmptyStateCopy {
            title: "Start a conversation with Klaw".to_string(),
            body: "Send a message below to begin this chat.".to_string(),
        },
        ConnectionViewState::Connecting => EmptyStateCopy {
            title: "Connecting to Klaw".to_string(),
            body: "Waiting for the chat room to come online.".to_string(),
        },
        ConnectionViewState::Disconnected => EmptyStateCopy {
            title: "Reconnect to Klaw".to_string(),
            body: "Reconnect from the toolbar, then send your next message.".to_string(),
        },
        ConnectionViewState::Error(error) => EmptyStateCopy {
            title: "Connection error".to_string(),
            body: format!("Klaw could not keep the chat connection alive: {error}"),
        },
    }
}

pub(crate) fn composer_hint_text(state: &ConnectionViewState) -> &'static str {
    match state {
        ConnectionViewState::Connected => "Message Klaw…",
        ConnectionViewState::Connecting => "Connecting to Klaw…",
        ConnectionViewState::Disconnected => "Reconnect to message Klaw…",
        ConnectionViewState::Error(_) => "Fix the connection to keep chatting…",
    }
}

pub(crate) fn can_send(state: &ConnectionViewState) -> bool {
    matches!(state, ConnectionViewState::Connected)
}

pub(crate) fn classify_incoming_text(
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{
        ConnectionViewState, MessageRole, can_send, classify_incoming_text, composer_hint_text,
        empty_state_copy, status_text, toolbar_title,
    };

    #[test]
    fn toolbar_title_matches_chat_product() {
        assert_eq!(toolbar_title(), "Klaw Web Chat");
    }

    #[test]
    fn connected_state_uses_friendly_status_copy() {
        assert_eq!(status_text(&ConnectionViewState::Connected), "Ready");
    }

    #[test]
    fn disconnected_empty_state_invites_reconnect() {
        let copy = empty_state_copy(&ConnectionViewState::Disconnected);
        assert_eq!(copy.title, "Reconnect to Klaw");
        assert!(copy.body.contains("Reconnect"));
    }

    #[test]
    fn error_empty_state_surfaces_context() {
        let copy = empty_state_copy(&ConnectionViewState::Error("send failed".to_string()));
        assert_eq!(copy.title, "Connection error");
        assert!(copy.body.contains("send failed"));
    }

    #[test]
    fn disconnected_state_disables_sending_and_updates_hint() {
        assert!(!can_send(&ConnectionViewState::Disconnected));
        assert_eq!(
            composer_hint_text(&ConnectionViewState::Disconnected),
            "Reconnect to message Klaw…"
        );
    }

    #[test]
    fn incoming_echo_matching_local_send_is_rendered_as_user_message() {
        let mut pending = VecDeque::from([String::from("hello from browser")]);
        let role = classify_incoming_text(&mut pending, "hello from browser");
        assert_eq!(role, MessageRole::User);
        assert!(pending.is_empty());
    }

    #[test]
    fn non_matching_incoming_text_is_rendered_as_assistant_message() {
        let mut pending = VecDeque::from([String::from("hello from browser")]);
        let role = classify_incoming_text(&mut pending, "hello from server");
        assert_eq!(role, MessageRole::Assistant);
        assert_eq!(pending.len(), 1);
    }
}
