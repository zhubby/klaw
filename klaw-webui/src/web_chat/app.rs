use std::{cell::RefCell, rc::Rc};

use eframe::egui::{self, Context};
use klaw_ui_kit::{NotificationCenter, theme_preference};
use web_sys::WebSocket;

use crate::{
    ConnectionState, SessionListEntry, resolve_gateway_token,
    sort_session_entries_by_created_at_desc,
};

use super::{
    protocol::ServerFrame,
    session::{SessionWindow, window_anchor_for_slot},
    storage::{PersistedWorkspaceState, load_workspace_state, save_workspace_state},
};

pub(super) struct ChatApp {
    pub(in crate::web_chat) ctx: Context,
    pub(in crate::web_chat) gateway_token: Option<String>,
    pub(in crate::web_chat) gateway_token_input: String,
    pub(in crate::web_chat) ws: Rc<RefCell<Option<WebSocket>>>,
    pub(in crate::web_chat) connection_state: Rc<RefCell<ConnectionState>>,
    pub(in crate::web_chat) pending_frames: Rc<RefCell<Vec<ServerFrame>>>,
    pub(in crate::web_chat) sessions: Vec<SessionWindow>,
    pub(in crate::web_chat) active_session_key: Option<String>,
    pub(in crate::web_chat) workspace_loaded: bool,
    pub(in crate::web_chat) toasts: Rc<RefCell<NotificationCenter>>,
    pub(in crate::web_chat) show_gateway_dialog: bool,
    pub(in crate::web_chat) rename_session_key: Option<String>,
    pub(in crate::web_chat) rename_session_input: String,
    pub(in crate::web_chat) did_attempt_prefilled_token: bool,
}

impl ChatApp {
    pub(super) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let persisted = load_workspace_state();
        let gateway_token =
            resolve_gateway_token(gateway_token_from_page(), persisted.gateway_token);
        let gateway_token_input = gateway_token.clone().unwrap_or_default();
        let persisted_active_session_key = persisted.active_session_key;
        let persisted_sessions = persisted.sessions;

        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            gateway_token,
            gateway_token_input,
            ws: Rc::new(RefCell::new(None)),
            connection_state: Rc::new(RefCell::new(ConnectionState::Disconnected)),
            pending_frames: Rc::new(RefCell::new(Vec::new())),
            sessions: Vec::new(),
            active_session_key: persisted_active_session_key,
            workspace_loaded: false,
            toasts: Rc::new(RefCell::new(NotificationCenter::default())),
            show_gateway_dialog: false,
            rename_session_key: None,
            rename_session_input: String::new(),
            did_attempt_prefilled_token: false,
        };
        app.restore_window_state(persisted_sessions);
        if let Some(legacy_theme_mode) = persisted.legacy_theme_mode {
            app.ctx.set_theme(theme_preference(legacy_theme_mode));
        }
        app.apply_theme();
        app
    }

    pub(in crate::web_chat) fn apply_theme(&self) {
        self.ctx
            .set_visuals_of(egui::Theme::Light, egui::Visuals::light());
        self.ctx
            .set_visuals_of(egui::Theme::Dark, egui::Visuals::dark());
    }

    pub(in crate::web_chat) fn persist_workspace_state(&self) {
        save_workspace_state(&PersistedWorkspaceState {
            legacy_theme_mode: None,
            sessions: self.sessions.iter().map(SessionWindow::metadata).collect(),
            active_session_key: self.active_session_key.clone(),
            gateway_token: self.gateway_token.clone(),
        });
    }

    pub(in crate::web_chat) fn session_index(&self, session_key: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.session_key == session_key)
    }

    pub(in crate::web_chat) fn bring_session_to_front(&mut self, session_key: &str) -> bool {
        let _ = session_key;
        false
    }

    pub(in crate::web_chat) fn focus_session(&mut self, session_key: &str) {
        let mut changed = false;
        if let Some(index) = self.session_index(session_key) {
            let session = &mut self.sessions[index];
            if !session.open {
                session.open = true;
                changed = true;
            }
        }
        if self.active_session_key.as_deref() != Some(session_key) {
            self.active_session_key = Some(session_key.to_string());
            changed = true;
        }
        if changed {
            self.persist_workspace_state();
        }
    }

    pub(in crate::web_chat) fn set_theme_mode(&mut self, theme_mode: egui::ThemePreference) {
        if self.ctx.options(|opt| opt.theme_preference) == theme_mode {
            return;
        }
        self.ctx.set_theme(theme_mode);
        self.persist_workspace_state();
    }

    pub(in crate::web_chat) fn remove_session(&mut self, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };
        self.sessions.remove(index);

        if self.active_session_key.as_deref() == Some(session_key) {
            self.active_session_key = self
                .sessions
                .first()
                .map(|session| session.session_key.clone());
        }
        self.persist_workspace_state();
    }

    pub(in crate::web_chat) fn tile_open_sessions(&mut self) {
        let mut slot = 0;
        let mut changed = false;
        for session in &mut self.sessions {
            if !session.open {
                continue;
            }
            let next_anchor = window_anchor_for_slot(slot);
            slot += 1;
            if session.window_anchor != next_anchor {
                session.window_anchor = next_anchor;
                changed = true;
            }
        }
        if changed {
            self.ctx.memory_mut(|memory| memory.reset_areas());
            self.ctx.request_repaint();
        }
    }

    pub(in crate::web_chat) fn reset_window_layout(&mut self) {
        let mut changed = false;
        for (index, session) in self.sessions.iter_mut().enumerate() {
            let next_anchor = window_anchor_for_slot(index as u32);
            if session.window_anchor != next_anchor {
                session.window_anchor = next_anchor;
                changed = true;
            }
        }
        if changed {
            self.ctx.memory_mut(|memory| memory.reset_areas());
            self.ctx.request_repaint();
        }
    }

    pub(in crate::web_chat) fn restore_window_state(
        &mut self,
        persisted_sessions: Vec<super::storage::PersistedSession>,
    ) {
        for (index, session) in persisted_sessions.into_iter().enumerate() {
            let mut restored = SessionWindow::new(
                SessionListEntry {
                    session_key: session.session_key,
                    title: String::new(),
                    created_at_ms: 0,
                },
                session.open,
            );
            restored.window_anchor = window_anchor_for_slot(index as u32);
            self.sessions.push(restored);
        }
    }

    pub(in crate::web_chat) fn is_workspace_ready(&self) -> bool {
        matches!(*self.connection_state.borrow(), ConnectionState::Connected)
            && self.workspace_loaded
    }

    pub(in crate::web_chat) fn sync_sessions_from_workspace(
        &mut self,
        mut entries: Vec<SessionListEntry>,
        active_session_key: Option<String>,
    ) {
        sort_session_entries_by_created_at_desc(&mut entries);
        let persisted = self
            .sessions
            .iter()
            .map(|session| (session.session_key.clone(), session.open))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut sessions = entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let open = persisted.get(&entry.session_key).copied().unwrap_or(true);
                let mut session = SessionWindow::new(entry, open);
                session.window_anchor = window_anchor_for_slot(index as u32);
                session
            })
            .collect::<Vec<_>>();
        for (index, session) in sessions.iter_mut().enumerate() {
            session.window_anchor = window_anchor_for_slot(index as u32);
        }
        self.sessions = sessions;
        self.workspace_loaded = true;
        self.active_session_key = active_session_key
            .filter(|key| {
                self.sessions
                    .iter()
                    .any(|session| &session.session_key == key)
            })
            .or_else(|| {
                self.sessions
                    .first()
                    .map(|session| session.session_key.clone())
            });
        self.persist_workspace_state();
    }
}

fn gateway_token_from_page() -> Option<String> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    parse_query_param(&search, "gateway_token").or_else(|| parse_query_param(&search, "token"))
}

fn parse_query_param(search: &str, key: &str) -> Option<String> {
    let q = search.trim_start_matches('?');
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k != key {
            continue;
        }
        return Some(urlencoding::decode(v).ok()?.into_owned());
    }
    None
}
