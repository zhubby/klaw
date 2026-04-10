use crate::{ThemeMode, normalize_gateway_token_input};
use serde::{Deserialize, Serialize};
use web_sys::Storage;

use super::session::{
    generate_session_key, is_valid_session_key, migrate_legacy_session_title, session_title,
};

const APP_STATE_STORAGE_KEY: &str = "klaw_webui_workspace_state";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedSession {
    pub(in crate::web_chat) session_key: String,
    pub(in crate::web_chat) title: String,
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
    #[serde(default = "default_next_session_number")]
    pub(in crate::web_chat) next_session_number: u32,
    #[serde(default)]
    pub(in crate::web_chat) gateway_token: Option<String>,
}

const fn default_next_session_number() -> u32 {
    2
}

const fn default_session_open() -> bool {
    true
}

fn default_workspace_state() -> PersistedWorkspaceState {
    PersistedWorkspaceState {
        legacy_theme_mode: None,
        sessions: vec![PersistedSession {
            session_key: generate_session_key(),
            title: session_title(1),
            open: true,
        }],
        active_session_key: None,
        next_session_number: default_next_session_number(),
        gateway_token: None,
    }
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

    state.sessions.retain(|session| {
        is_valid_session_key(&session.session_key) && !session.title.trim().is_empty()
    });

    if state.sessions.is_empty() {
        return default_workspace_state();
    }

    for session in state.sessions.iter_mut() {
        if let Some(updated_title) = migrate_legacy_session_title(&session.title) {
            session.title = updated_title;
        }
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

    state.next_session_number = state
        .next_session_number
        .max(state.sessions.len() as u32 + 1);
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
