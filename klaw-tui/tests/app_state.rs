use klaw_tui::{AppMessage, AppState, MessageRole, TuiMeta};

fn sample_meta() -> TuiMeta {
    TuiMeta {
        version: "0.10.3".to_string(),
        session_key: "terminal:test".to_string(),
        channel: "terminal".to_string(),
        provider: "openai".to_string(),
        model: "gpt-5".to_string(),
        skills: "git-commit".to_string(),
        tools: "shell, web_fetch".to_string(),
        mcp: "ready".to_string(),
        show_reasoning: false,
    }
}

#[test]
fn app_state_starts_with_terminal_session_metadata() {
    let state = AppState::new(sample_meta());
    assert_eq!(state.meta().channel, "terminal");
    assert_eq!(state.meta().session_key, "terminal:test");
    assert!(state.messages().is_empty());
}

#[test]
fn submit_ignores_blank_input() {
    let mut state = AppState::new(sample_meta());
    state.set_input("   \n".to_string());
    assert!(state.take_submit_input().is_none());
    assert!(state.messages().is_empty());
}

#[test]
fn submit_turns_input_into_user_message_and_clears_editor() {
    let mut state = AppState::new(sample_meta());
    state.set_input("hello".to_string());
    let submitted = state
        .take_submit_input()
        .expect("non-empty input should submit");
    assert_eq!(submitted, "hello");
    assert_eq!(state.input(), "");
    assert_eq!(
        state.messages(),
        &[AppMessage::new(MessageRole::User, "hello".to_string())]
    );
}

#[test]
fn snapshot_replaces_pending_agent_message() {
    let mut state = AppState::new(sample_meta());
    state.begin_agent_response();
    state.apply_agent_snapshot("first".to_string());
    state.apply_agent_snapshot("final".to_string());
    assert_eq!(
        state.messages().last(),
        Some(&AppMessage::new(MessageRole::Agent, "final".to_string()))
    );
}
