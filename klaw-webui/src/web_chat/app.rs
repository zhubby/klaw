use std::{cell::RefCell, rc::Rc};

use eframe::egui::{self, Context};
use klaw_ui_kit::{NotificationCenter, theme_preference};
use wasm_bindgen::JsCast;
use web_sys::Notification;
use web_sys::WebSocket;

use crate::{
    ConnectionState, SessionListEntry, attachment_action_in_progress,
    normalize_gateway_token_input, resolve_gateway_token,
    should_prompt_for_gateway_token_before_connect, sort_session_entries_by_created_at_desc,
};

use super::{
    protocol::ServerFrame,
    session::{SessionWindow, session_window_id, window_anchor_for_slot},
    storage::{PersistedWorkspaceState, load_workspace_state, save_workspace_state},
};

pub(super) struct ChatApp {
    pub(in crate::web_chat) ctx: Context,
    pub(in crate::web_chat) gateway_origin: Option<String>,
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
    pub(in crate::web_chat) delete_session_key: Option<String>,
    pub(in crate::web_chat) did_attempt_prefilled_token: bool,
    pub(in crate::web_chat) stream_enabled: bool,
}

impl ChatApp {
    pub(super) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let _ = Notification::request_permission();
        let persisted = load_workspace_state();
        let gateway_token =
            resolve_gateway_token(gateway_token_from_page(), persisted.gateway_token);
        let gateway_token_input = gateway_token.clone().unwrap_or_default();
        let persisted_active_session_key = persisted.active_session_key;
        let persisted_sessions = persisted.sessions;
        let stream_enabled = persisted.stream_enabled;

        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            gateway_origin: gateway_origin_from_page(),
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
            delete_session_key: None,
            did_attempt_prefilled_token: false,
            stream_enabled,
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
            stream_enabled: self.stream_enabled,
        });
    }

    /// Keep in-memory token aligned with the text field and write `localStorage` so reconnects
    /// survive reloads (toolbar Connect does not require opening the Save dialog).
    pub(in crate::web_chat) fn sync_gateway_token_from_input_and_persist(&mut self) {
        self.gateway_token = normalize_gateway_token_input(&self.gateway_token_input);
        self.persist_workspace_state();
    }

    pub(in crate::web_chat) fn disconnect_and_clear_token(&mut self) {
        self.close_connection();
        self.gateway_token = None;
        self.gateway_token_input.clear();
        self.persist_workspace_state();
    }

    /// Subscribe every visible agent window that has not received history yet.
    pub(in crate::web_chat) fn subscribe_open_sessions_needing_history(&mut self) {
        if !self.is_workspace_ready() {
            return;
        }
        let keys: Vec<String> = self
            .sessions
            .iter()
            .filter(|session| session.open && !*session.buffers.history_loaded.borrow())
            .map(|session| session.session_key.clone())
            .collect();
        for session_key in keys {
            self.subscribe_session(&session_key);
        }
    }

    pub(in crate::web_chat) fn session_index(&self, session_key: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.session_key == session_key)
    }

    pub(in crate::web_chat) fn bring_session_to_front(&mut self, session_key: &str) -> bool {
        if self.session_index(session_key).is_none() {
            return false;
        }

        self.ctx.move_to_top(egui::LayerId::new(
            egui::Order::Middle,
            session_window_id(session_key),
        ));
        self.ctx.request_repaint();
        true
    }

    pub(in crate::web_chat) fn focus_session(&mut self, session_key: &str) {
        let mut changed = false;
        let mut should_subscribe_history = false;
        let workspace_ready = self.is_workspace_ready();
        if let Some(index) = self.session_index(session_key) {
            let session = &mut self.sessions[index];
            if !session.open {
                session.open = true;
                changed = true;
            }
            should_subscribe_history = workspace_ready && !*session.buffers.history_loaded.borrow();
        }
        if self.active_session_key.as_deref() != Some(session_key) {
            self.active_session_key = Some(session_key.to_string());
            changed = true;
        }
        let moved_to_front = self.bring_session_to_front(session_key);
        if should_subscribe_history {
            self.subscribe_session(session_key);
        }
        if changed || moved_to_front {
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

    pub(in crate::web_chat) fn request_workspace_connection(&mut self) {
        let token_for_gate = normalize_gateway_token_input(&self.gateway_token_input)
            .or_else(|| self.gateway_token.clone());
        if should_prompt_for_gateway_token_before_connect(token_for_gate.as_deref()) {
            self.show_gateway_dialog = true;
            return;
        }
        self.reconnect_all_sessions();
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
                let open = persisted.get(&entry.session_key).copied().unwrap_or(false);
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

fn gateway_origin_from_page() -> Option<String> {
    web_sys::window()?.location().origin().ok()
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

impl ChatApp {
    pub(in crate::web_chat) fn trigger_file_picker(&mut self, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };

        let Some(gateway_origin) = self.gateway_origin.clone() else {
            self.toasts
                .borrow_mut()
                .error("Gateway origin not available");
            return;
        };

        let gateway_token = self.gateway_token.clone();
        let session_key_owned = session_key.to_string();
        let toasts = self.toasts.clone();
        let ctx = self.ctx.clone();

        let selected_archive_id = self.sessions[index].selected_archive_id.clone();
        let selecting_flag = self.sessions[index].selecting_file.clone();
        let uploading_flag = self.sessions[index].uploading_file.clone();

        if attachment_action_in_progress(*selecting_flag.borrow(), *uploading_flag.borrow()) {
            return;
        }

        *selecting_flag.borrow_mut() = true;
        self.ctx.request_repaint();

        wasm_bindgen_futures::spawn_local(async move {
            let window = match web_sys::window() {
                Some(w) => w,
                None => {
                    toasts.borrow_mut().error("Window not available");
                    set_attachment_flag(&selecting_flag, false, &ctx);
                    return;
                }
            };

            let document = match window.document() {
                Some(d) => d,
                None => {
                    toasts.borrow_mut().error("Document not available");
                    set_attachment_flag(&selecting_flag, false, &ctx);
                    return;
                }
            };

            let Some(body) = document.body() else {
                toasts.borrow_mut().error("Document body not available");
                set_attachment_flag(&selecting_flag, false, &ctx);
                return;
            };

            let input = match document.create_element("input") {
                Ok(el) => el,
                Err(_) => {
                    toasts.borrow_mut().error("Failed to create input element");
                    set_attachment_flag(&selecting_flag, false, &ctx);
                    return;
                }
            };

            if input.set_attribute("type", "file").is_err() {
                toasts
                    .borrow_mut()
                    .error("Failed to set input type attribute");
                set_attachment_flag(&selecting_flag, false, &ctx);
                return;
            }

            let _ = input.set_attribute("style", "display:none");

            let input_element: web_sys::HtmlInputElement = match input.dyn_into() {
                Ok(el) => el,
                Err(_) => {
                    toasts
                        .borrow_mut()
                        .error("Failed to cast to HtmlInputElement");
                    set_attachment_flag(&selecting_flag, false, &ctx);
                    return;
                }
            };

            if body.append_child(&input_element).is_err() {
                toasts.borrow_mut().error("Failed to attach file input");
                set_attachment_flag(&selecting_flag, false, &ctx);
                return;
            }

            input_element.click();
            let picked_file = match wait_for_selected_file(&window, &document, &input_element).await
            {
                Ok(file) => file,
                Err(err) => {
                    remove_file_input(&input_element);
                    set_attachment_flag(&selecting_flag, false, &ctx);
                    toasts
                        .borrow_mut()
                        .error(format!("Failed to read selected file: {err}"));
                    return;
                }
            };

            remove_file_input(&input_element);
            set_attachment_flag(&selecting_flag, false, &ctx);

            let Some(file) = picked_file else {
                return;
            };

            set_attachment_flag(&uploading_flag, true, &ctx);

            match super::upload::upload_file_to_archive(
                &gateway_origin,
                gateway_token.as_deref(),
                file,
                &session_key_owned,
            )
            .await
            {
                Ok(record) => {
                    toasts.borrow_mut().success(format!(
                        "File uploaded: {}",
                        record.original_filename.as_deref().unwrap_or("unknown")
                    ));
                    *selected_archive_id.borrow_mut() = Some(record.id);
                    set_attachment_flag(&uploading_flag, false, &ctx);
                }
                Err(err) => {
                    toasts.borrow_mut().error(format!("Upload failed: {err}"));
                    set_attachment_flag(&uploading_flag, false, &ctx);
                }
            }
        });
    }
}

fn set_attachment_flag(flag: &Rc<RefCell<bool>>, value: bool, ctx: &Context) {
    *flag.borrow_mut() = value;
    ctx.request_repaint();
}

fn remove_file_input(input_element: &web_sys::HtmlInputElement) {
    input_element.remove();
}

async fn wait_for_selected_file(
    window: &web_sys::Window,
    document: &web_sys::Document,
    input_element: &web_sys::HtmlInputElement,
) -> Result<Option<web_sys::File>, String> {
    const FILE_PICKER_TIMEOUT_MS: f64 = 120_000.0;

    let mut picker_took_focus = false;
    let started_at = js_sys::Date::now();

    loop {
        if let Some(file) = input_element.files().and_then(|files| files.get(0)) {
            return Ok(Some(file));
        }

        let has_focus = document
            .has_focus()
            .map_err(|_| "Failed to inspect picker focus state".to_string())?;
        if !has_focus {
            picker_took_focus = true;
        } else if picker_took_focus {
            return Ok(None);
        }
        if js_sys::Date::now() - started_at >= FILE_PICKER_TIMEOUT_MS {
            return Ok(None);
        }

        sleep_ms(window, 50).await;
    }
}

async fn sleep_ms(window: &web_sys::Window, ms: i32) {
    use wasm_bindgen::{JsValue, closure::Closure};
    use wasm_bindgen_futures::JsFuture;

    let window = window.clone();
    let promise = js_sys::Promise::new(&mut move |resolve, _reject| {
        let resolve_callback = resolve.clone();
        let callback = Closure::once_into_js(move || {
            let _ = resolve_callback.call0(&JsValue::NULL);
        });

        if window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.unchecked_ref(),
                ms,
            )
            .is_err()
        {
            let _ = resolve.call0(&JsValue::NULL);
            return;
        }
    });

    let _ = JsFuture::from(promise).await;
}
