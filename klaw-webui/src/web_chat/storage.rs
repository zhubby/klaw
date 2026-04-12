use crate::{ThemeMode, normalize_gateway_token_input};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use web_sys::Storage;

const APP_STATE_STORAGE_KEY: &str = "klaw_webui_workspace_state";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedSession {
    pub(in crate::web_chat) session_key: String,
    #[serde(default = "default_session_open")]
    pub(in crate::web_chat) open: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedWorkspaceState {
    #[serde(default)]
    pub(in crate::web_chat) legacy_theme_mode: Option<ThemeMode>,
    #[serde(default)]
    pub(in crate::web_chat) sessions: Vec<PersistedSession>,
    #[serde(default)]
    pub(in crate::web_chat) active_session_key: Option<String>,
    #[serde(default)]
    pub(in crate::web_chat) gateway_token: Option<String>,
    #[serde(default = "default_stream_enabled")]
    pub(in crate::web_chat) stream_enabled: bool,
}

const fn default_session_open() -> bool {
    false
}

fn default_workspace_state() -> PersistedWorkspaceState {
    PersistedWorkspaceState {
        legacy_theme_mode: None,
        sessions: Vec::new(),
        active_session_key: None,
        gateway_token: None,
        stream_enabled: default_stream_enabled(),
    }
}

const fn default_stream_enabled() -> bool {
    true
}

fn storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub(super) fn load_workspace_state() -> PersistedWorkspaceState {
    let mut state = storage()
        .and_then(|storage| storage.get_item(APP_STATE_STORAGE_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str::<PersistedWorkspaceState>(&raw).ok())
        .unwrap_or_else(default_workspace_state);

    if state.legacy_theme_mode.is_none() {
        #[derive(Deserialize)]
        struct LegacyWorkspaceTheme {
            theme_mode: Option<ThemeMode>,
        }

        state.legacy_theme_mode = storage()
            .and_then(|storage| storage.get_item(APP_STATE_STORAGE_KEY).ok().flatten())
            .and_then(|raw| serde_json::from_str::<LegacyWorkspaceTheme>(&raw).ok())
            .and_then(|legacy| legacy.theme_mode);
    }

    state
        .sessions
        .retain(|session| is_valid_session_key(&session.session_key));

    if state.sessions.is_empty() {
        state.active_session_key = None;
    }

    if state.active_session_key.as_ref().is_none_or(|active| {
        !state
            .sessions
            .iter()
            .any(|session| &session.session_key == active)
    }) {
        state.active_session_key = state
            .sessions
            .first()
            .map(|session| session.session_key.clone());
    }

    state.gateway_token = state
        .gateway_token
        .as_deref()
        .and_then(normalize_gateway_token_input);
    state
}

pub(super) fn save_workspace_state(state: &PersistedWorkspaceState) {
    let Some(storage) = storage() else {
        return;
    };
    let Ok(encoded) = serde_json::to_string(state) else {
        return;
    };
    let _ = storage.set_item(APP_STATE_STORAGE_KEY, &encoded);
}

fn is_valid_session_key(session_key: &str) -> bool {
    let Some(rest) = session_key.strip_prefix("web:") else {
        return false;
    };
    Uuid::parse_str(rest).is_ok()
}

#[cfg(test)]
mod tests {
    use super::{default_stream_enabled, default_workspace_state};

    #[test]
    fn persisted_workspace_state_defaults_without_local_sessions() {
        let state = default_workspace_state();
        assert!(state.active_session_key.is_none());
        assert!(state.gateway_token.is_none());
        assert!(state.sessions.is_empty());
        assert!(state.stream_enabled);
    }

    #[test]
    fn webui_stream_toggle_defaults_to_enabled() {
        assert!(default_stream_enabled());
    }
}
