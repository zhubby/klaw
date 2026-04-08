use uuid::Uuid;

pub const TERMINAL_CHANNEL_NAME: &str = "terminal";

pub fn resolve_session_key(session_key: Option<String>) -> String {
    session_key.unwrap_or_else(|| format!("{TERMINAL_CHANNEL_NAME}:{}", Uuid::new_v4()))
}

pub fn resolve_chat_id(session_key: &str) -> String {
    session_key.split(':').nth(1).unwrap_or("chat").to_string()
}

#[cfg(test)]
mod tests {
    use super::{TERMINAL_CHANNEL_NAME, resolve_chat_id, resolve_session_key};
    use crate::{
        ChannelResponse,
        render::{OutputRenderStyle, render_agent_output},
    };

    #[test]
    fn keeps_explicit_session_key() {
        let session_key = resolve_session_key(Some("terminal:explicit".to_string()));
        assert_eq!(session_key, "terminal:explicit");
    }

    #[test]
    fn defaults_session_key_to_terminal_prefix() {
        let session_key = resolve_session_key(None);
        assert!(session_key.starts_with("terminal:"));
    }

    #[test]
    fn reports_terminal_channel_name() {
        assert_eq!(TERMINAL_CHANNEL_NAME, "terminal");
    }

    #[test]
    fn derives_chat_id_from_terminal_session_key() {
        assert_eq!(resolve_chat_id("terminal:chat-42"), "chat-42");
    }

    #[test]
    fn hides_reasoning_when_flag_disabled() {
        let view = render_agent_output(
            &ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1\nstep2".to_string()),
                metadata: std::collections::BTreeMap::new(),
                attachments: Vec::new(),
            },
            false,
            OutputRenderStyle::Terminal,
        );
        assert!(view.contains("[answer]"));
        assert!(!view.contains("[reasoning]"));
    }

    #[test]
    fn renders_reasoning_block_when_enabled() {
        let view = render_agent_output(
            &ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1\nstep2".to_string()),
                metadata: std::collections::BTreeMap::new(),
                attachments: Vec::new(),
            },
            true,
            OutputRenderStyle::Terminal,
        );
        assert!(view.contains("[reasoning]"));
        assert!(view.contains("> step1"));
        assert!(view.contains("> step2"));
    }
}
