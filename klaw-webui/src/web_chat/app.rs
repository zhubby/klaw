use std::{cell::RefCell, rc::Rc};

use eframe::egui::{self, Context};
use klaw_ui_kit::{NotificationCenter, theme_preference};

use crate::resolve_gateway_token;

use super::{
    session::{
        SessionWindow, generate_session_key, session_title, window_anchor_for_slot,
    },
    storage::{
        PersistedSession, PersistedWorkspaceState, load_workspace_state, save_workspace_state,
    },
};

pub(super) struct ChatApp {
    pub(in crate::web_chat) ctx: Context,
    pub(in crate::web_chat) gateway_token: Option<String>,
    pub(in crate::web_chat) gateway_token_input: String,
    pub(in crate::web_chat) sessions: Vec<SessionWindow>,
    pub(in crate::web_chat) active_session_key: Option<String>,
    pub(in crate::web_chat) next_session_number: u32,
    pub(in crate::web_chat) toasts: Rc<RefCell<NotificationCenter>>,
    pub(in crate::web_chat) show_gateway_dialog: bool,
    pub(in crate::web_chat) rename_session_key: Option<String>,
    pub(in crate::web_chat) rename_session_input: String,
    pub(in crate::web_chat) did_attempt_prefilled_token: bool,
}

impl ChatApp {
    pub(super) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let persisted = load_workspace_state();
        let gateway_token = resolve_gateway_token(gateway_token_from_page(), persisted.gateway_token);
        let gateway_token_input = gateway_token.clone().unwrap_or_default();
        let sessions = persisted
            .sessions
            .into_iter()
            .map(SessionWindow::new)
            .collect::<Vec<_>>();
        let mut sessions = sessions;
        for (index, session) in sessions.iter_mut().enumerate() {
            session.window_anchor = window_anchor_for_slot(index as u32);
        }
        let active_session_key = persisted
            .active_session_key
            .or_else(|| sessions.first().map(|session| session.session_key.clone()));

        let app = Self {
            ctx: cc.egui_ctx.clone(),
            gateway_token,
            gateway_token_input,
            sessions,
            active_session_key,
            next_session_number: persisted.next_session_number,
            toasts: Rc::new(RefCell::new(NotificationCenter::default())),
            show_gateway_dialog: false,
            rename_session_key: None,
            rename_session_input: String::new(),
            did_attempt_prefilled_token: false,
        };
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
            next_session_number: self.next_session_number,
            gateway_token: self.gateway_token.clone(),
        });
    }

    pub(in crate::web_chat) fn session_index(&self, session_key: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.session_key == session_key)
    }

    pub(in crate::web_chat) fn bring_session_to_front(&mut self, session_key: &str) -> bool {
        let Some(index) = self.session_index(session_key) else {
            return false;
        };
        if index + 1 == self.sessions.len() {
            return false;
        }
        let session = self.sessions.remove(index);
        self.sessions.push(session);
        true
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
        if self.bring_session_to_front(session_key) {
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
        let session = self.sessions.remove(index);
        Self::close_buffers(&session.buffers);

        if self.active_session_key.as_deref() == Some(session_key) {
            self.active_session_key = self
                .sessions
                .last()
                .map(|session| session.session_key.clone());
        }
        self.persist_workspace_state();
    }

    pub(in crate::web_chat) fn choose_next_window_anchor(&self) -> super::session::WindowAnchor {
        window_anchor_for_slot(self.sessions.iter().filter(|session| session.open).count() as u32)
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

    pub(in crate::web_chat) fn create_session(&mut self) {
        let window_anchor = self.choose_next_window_anchor();
        let session = PersistedSession {
            session_key: generate_session_key(),
            title: session_title(self.next_session_number),
            open: true,
        };
        self.next_session_number += 1;
        let session_key = session.session_key.clone();
        let mut session = SessionWindow::new(session);
        session.window_anchor = window_anchor;
        self.sessions.push(session);
        self.active_session_key = Some(session_key.clone());
        self.persist_workspace_state();

        if self.gateway_token.is_some() {
            self.try_connect_session(&session_key);
        }
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
