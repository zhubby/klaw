use crate::ThemeMode;
use serde::{Deserialize, Serialize};
use web_sys::Storage;

use super::session::{
    WindowAnchor, generate_session_key, is_valid_session_key, session_title, window_anchor_for_slot,
};

const APP_STATE_STORAGE_KEY: &str = "klaw_webui_workspace_state";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedSession {
    pub(in crate::web_chat) session_key: String,
    pub(in crate::web_chat) title: String,
    #[serde(default = "default_session_open")]
    pub(in crate::web_chat) open: bool,
    #[serde(default)]
    pub(in crate::web_chat) window_anchor: Option<WindowAnchor>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct PersistedWorkspaceState {
    pub(in crate::web_chat) theme_mode: ThemeMode,
    #[serde(default)]
    pub(in crate::web_chat) sessions: Vec<PersistedSession>,
    #[serde(default)]
    pub(in crate::web_chat) active_session_key: Option<String>,
    #[serde(default = "default_next_session_number")]
    pub(in crate::web_chat) next_session_number: u32,
}

const fn default_next_session_number() -> u32 {
    2
}

const fn default_session_open() -> bool {
    true
}

fn default_workspace_state() -> PersistedWorkspaceState {
    PersistedWorkspaceState {
        theme_mode: ThemeMode::System,
        sessions: vec![PersistedSession {
            session_key: generate_session_key(),
            title: session_title(1),
            open: true,
            window_anchor: Some(window_anchor_for_slot(0)),
        }],
        active_session_key: None,
        next_session_number: default_next_session_number(),
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

    state.sessions.retain(|session| {
        is_valid_session_key(&session.session_key) && !session.title.trim().is_empty()
    });

    if state.sessions.is_empty() {
        return default_workspace_state();
    }

    for (index, session) in state.sessions.iter_mut().enumerate() {
        if session.window_anchor.is_none() {
            session.window_anchor = Some(window_anchor_for_slot(index as u32));
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
