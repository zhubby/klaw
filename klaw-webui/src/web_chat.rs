//! WASM-only egui chat client for `/ws/chat`.

use std::{cell::RefCell, rc::Rc};

use crate::{
    ConnectionState, MessageRole, ThemeMode, normalize_gateway_token_input, toolbar_title,
};
use eframe::egui::{
    self, Align, Align2, Button, ComboBox, Context, Frame, Id, Key, Layout, RichText, ScrollArea,
    TextEdit, TopBottomPanel, vec2,
};
use egui_notify::{Anchor, Toasts};
use egui_phosphor::regular;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, MessageEvent, Storage, WebSocket};

const APP_STATE_STORAGE_KEY: &str = "klaw_webui_workspace_state";
const BUBBLE_MAX_WIDTH: f32 = 420.0;
const SESSION_LIST_WIDTH: f32 = 220.0;
const SESSION_WINDOW_DEFAULT_WIDTH: f32 = 560.0;
const SESSION_WINDOW_DEFAULT_HEIGHT: f32 = 620.0;
const SESSION_WINDOW_MIN_WIDTH: f32 = 360.0;
const SESSION_WINDOW_MIN_HEIGHT: f32 = 420.0;
const WINDOW_START_X: f32 = SESSION_LIST_WIDTH + 28.0;
const WINDOW_START_Y: f32 = 72.0;
const WINDOW_OFFSET_X: f32 = 40.0;
const WINDOW_OFFSET_Y: f32 = 32.0;
const WINDOW_STAGGER_COLUMNS: u32 = 4;
const INPUT_PANEL_HEIGHT: f32 = 124.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChatMessage {
    text: String,
    role: MessageRole,
    timestamp_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedSession {
    session_key: String,
    title: String,
    #[serde(default = "default_session_open")]
    open: bool,
    #[serde(default)]
    window_anchor: Option<WindowAnchor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WindowAnchor {
    x: i32,
    y: i32,
}

impl WindowAnchor {
    fn from_pos2(pos: egui::Pos2) -> Self {
        Self {
            x: pos.x.round() as i32,
            y: pos.y.round() as i32,
        }
    }

    fn to_pos2(self) -> egui::Pos2 {
        egui::pos2(self.x as f32, self.y as f32)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedWorkspaceState {
    #[serde(default)]
    theme_mode: ThemeMode,
    #[serde(default)]
    sessions: Vec<PersistedSession>,
    #[serde(default)]
    active_session_key: Option<String>,
    #[serde(default = "default_next_session_number")]
    next_session_number: u32,
}

#[derive(Clone)]
struct SessionBuffers {
    messages: Rc<RefCell<Vec<ChatMessage>>>,
    state: Rc<RefCell<ConnectionState>>,
    ws: Rc<RefCell<Option<WebSocket>>>,
    auth_verified: Rc<RefCell<bool>>,
    suppress_next_close_notice: Rc<RefCell<bool>>,
    active_stream_request_id: Rc<RefCell<Option<String>>>,
}

impl Default for SessionBuffers {
    fn default() -> Self {
        Self {
            messages: Rc::new(RefCell::new(Vec::new())),
            state: Rc::new(RefCell::new(ConnectionState::Disconnected)),
            ws: Rc::new(RefCell::new(None)),
            auth_verified: Rc::new(RefCell::new(false)),
            suppress_next_close_notice: Rc::new(RefCell::new(false)),
            active_stream_request_id: Rc::new(RefCell::new(None)),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientFrame<'a> {
    Method {
        id: &'a str,
        method: &'a str,
        #[serde(default)]
        params: Value,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerFrame {
    Event {
        event: String,
        #[serde(default)]
        payload: Value,
    },
    Result {
        id: String,
        #[serde(default)]
        result: Value,
    },
    Error {
        id: Option<String>,
        error: ServerErrorFrame,
    },
}

#[derive(Debug, Deserialize)]
struct ServerErrorFrame {
    code: String,
    message: String,
}

struct SessionWindow {
    session_key: String,
    title: String,
    draft: String,
    open: bool,
    window_anchor: WindowAnchor,
    buffers: SessionBuffers,
}

/// Start the chat UI on the given canvas (install from `index.html` via wasm-bindgen).
#[wasm_bindgen]
pub fn start_chat_ui(canvas: web_sys::HtmlCanvasElement) {
    console_error_panic_hook::set_once();
    let web_options = eframe::WebOptions::default();
    let runner = eframe::WebRunner::new();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = runner
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(ChatApp::new(cc)))),
            )
            .await;
    });
}

fn default_next_session_number() -> u32 {
    2
}

fn default_session_open() -> bool {
    true
}

fn window_anchor_for_slot(slot: u32) -> WindowAnchor {
    let column = slot % WINDOW_STAGGER_COLUMNS;
    let row = slot / WINDOW_STAGGER_COLUMNS;
    WindowAnchor {
        x: (WINDOW_START_X + column as f32 * WINDOW_OFFSET_X).round() as i32,
        y: (WINDOW_START_Y + row as f32 * WINDOW_OFFSET_Y).round() as i32,
    }
}

fn anchor_rect(anchor: WindowAnchor) -> egui::Rect {
    egui::Rect::from_min_size(
        anchor.to_pos2(),
        egui::vec2(SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_DEFAULT_HEIGHT),
    )
}

fn current_timestamp_ms() -> i64 {
    js_sys::Date::now().round() as i64
}

fn format_message_timestamp(timestamp_ms: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(timestamp_ms as f64));
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
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

fn generate_session_key() -> String {
    format!("web:{}", Uuid::new_v4())
}

fn is_valid_session_key(session_key: &str) -> bool {
    let Some(rest) = session_key.strip_prefix("web:") else {
        return false;
    };
    Uuid::parse_str(rest).is_ok()
}

fn session_title(number: u32) -> String {
    format!("Session {number}")
}

fn load_workspace_state() -> PersistedWorkspaceState {
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

fn ws_chat_url(token: Option<&str>) -> Result<String, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let loc = window.location();
    let protocol = loc
        .protocol()
        .map_err(|_| "location.protocol unavailable".to_string())?;
    let ws_scheme = if protocol == "https:" { "wss" } else { "ws" };
    let host = loc
        .host()
        .map_err(|_| "location.host unavailable".to_string())?;
    let mut url = format!("{ws_scheme}://{host}/ws/chat");
    if let Some(token) = token {
        url.push_str("?token=");
        url.push_str(&urlencoding::encode(token));
    }
    Ok(url)
}

fn send_method(ws: &WebSocket, id: &str, method: &str, params: Value) -> Result<(), String> {
    let frame = ClientFrame::Method { id, method, params };
    let payload = serde_json::to_string(&frame)
        .map_err(|err| format!("serialize websocket method: {err}"))?;
    ws.send_with_str(&payload)
        .map_err(|_| format!("websocket send failed for method '{method}'"))
}

impl SessionWindow {
    fn new(metadata: PersistedSession) -> Self {
        Self {
            session_key: metadata.session_key,
            title: metadata.title,
            draft: String::new(),
            open: metadata.open,
            window_anchor: metadata
                .window_anchor
                .unwrap_or_else(|| window_anchor_for_slot(0)),
            buffers: SessionBuffers::default(),
        }
    }

    fn metadata(&self) -> PersistedSession {
        PersistedSession {
            session_key: self.session_key.clone(),
            title: self.title.clone(),
            open: self.open,
            window_anchor: Some(self.window_anchor),
        }
    }

    fn connection_state(&self) -> ConnectionState {
        self.buffers.state.borrow().clone()
    }
}

pub struct ChatApp {
    ctx: Context,
    theme_mode: ThemeMode,
    gateway_token: Option<String>,
    gateway_token_input: String,
    sessions: Vec<SessionWindow>,
    active_session_key: Option<String>,
    next_session_number: u32,
    toasts: Rc<RefCell<Toasts>>,
    show_gateway_dialog: bool,
    rename_session_key: Option<String>,
    rename_session_input: String,
    did_attempt_prefilled_token: bool,
}

impl ChatApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let persisted = load_workspace_state();
        let gateway_token = gateway_token_from_page();
        let gateway_token_input = gateway_token.clone().unwrap_or_default();
        let toasts = Toasts::new()
            .with_anchor(Anchor::BottomRight)
            .with_margin(vec2(16.0, 16.0));
        let sessions = persisted
            .sessions
            .into_iter()
            .map(SessionWindow::new)
            .collect::<Vec<_>>();
        let active_session_key = persisted
            .active_session_key
            .or_else(|| sessions.first().map(|session| session.session_key.clone()));

        let app = Self {
            ctx: cc.egui_ctx.clone(),
            theme_mode: persisted.theme_mode,
            gateway_token,
            gateway_token_input,
            sessions,
            active_session_key,
            next_session_number: persisted.next_session_number,
            toasts: Rc::new(RefCell::new(toasts)),
            show_gateway_dialog: false,
            rename_session_key: None,
            rename_session_input: String::new(),
            did_attempt_prefilled_token: false,
        };
        app.apply_theme();
        app
    }

    fn apply_theme(&self) {
        let preference = match self.theme_mode {
            ThemeMode::System => egui::ThemePreference::System,
            ThemeMode::Light => egui::ThemePreference::Light,
            ThemeMode::Dark => egui::ThemePreference::Dark,
        };
        self.ctx.set_theme(preference);
        self.ctx
            .set_visuals_of(egui::Theme::Light, egui::Visuals::light());
        self.ctx
            .set_visuals_of(egui::Theme::Dark, egui::Visuals::dark());
    }

    fn persist_workspace_state(&self) {
        let Some(storage) = storage() else {
            return;
        };
        let state = PersistedWorkspaceState {
            theme_mode: self.theme_mode,
            sessions: self.sessions.iter().map(SessionWindow::metadata).collect(),
            active_session_key: self.active_session_key.clone(),
            next_session_number: self.next_session_number,
        };
        let Ok(encoded) = serde_json::to_string(&state) else {
            return;
        };
        let _ = storage.set_item(APP_STATE_STORAGE_KEY, &encoded);
    }

    fn session_index(&self, session_key: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.session_key == session_key)
    }

    fn bring_session_to_front(&mut self, session_key: &str) -> bool {
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

    fn focus_session(&mut self, session_key: &str) {
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

    fn set_theme_mode(&mut self, theme_mode: ThemeMode) {
        if self.theme_mode == theme_mode {
            return;
        }
        self.theme_mode = theme_mode;
        self.apply_theme();
        self.persist_workspace_state();
    }

    fn close_buffers(buffers: &SessionBuffers) {
        if let Some(ws) = buffers.ws.borrow_mut().take() {
            *buffers.suppress_next_close_notice.borrow_mut() = true;
            let _ = ws.close();
        }
        *buffers.state.borrow_mut() = ConnectionState::Disconnected;
        *buffers.auth_verified.borrow_mut() = false;
    }

    fn remove_session(&mut self, session_key: &str) {
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

    fn choose_next_window_anchor(&self) -> WindowAnchor {
        let occupied_rects = self
            .sessions
            .iter()
            .filter(|session| session.open)
            .map(|session| anchor_rect(session.window_anchor))
            .collect::<Vec<_>>();

        for slot in 0..128 {
            let anchor = window_anchor_for_slot(slot);
            let rect = anchor_rect(anchor);
            let overlaps_existing = occupied_rects
                .iter()
                .any(|occupied| occupied.intersects(rect));
            if !overlaps_existing {
                return anchor;
            }
        }

        window_anchor_for_slot(self.sessions.len() as u32)
    }

    fn tile_open_sessions(&mut self) {
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
            self.persist_workspace_state();
            self.ctx.request_repaint();
        }
    }

    fn reset_window_layout(&mut self) {
        let mut changed = false;
        for (index, session) in self.sessions.iter_mut().enumerate() {
            let next_anchor = window_anchor_for_slot(index as u32);
            if session.window_anchor != next_anchor {
                session.window_anchor = next_anchor;
                changed = true;
            }
        }
        if changed {
            self.persist_workspace_state();
            self.ctx.request_repaint();
        }
    }

    fn create_session(&mut self) {
        let window_anchor = self.choose_next_window_anchor();
        let session = PersistedSession {
            session_key: generate_session_key(),
            title: session_title(self.next_session_number),
            open: true,
            window_anchor: Some(window_anchor),
        };
        self.next_session_number += 1;
        let session_key = session.session_key.clone();
        self.sessions.push(SessionWindow::new(session));
        self.active_session_key = Some(session_key.clone());
        self.persist_workspace_state();

        if self.gateway_token.is_some() {
            self.try_connect_session(&session_key);
        }
    }

    fn reconnect_all_sessions(&mut self) {
        let keys = self
            .sessions
            .iter()
            .map(|session| session.session_key.clone())
            .collect::<Vec<_>>();
        for session_key in keys {
            self.try_connect_session(&session_key);
        }
    }

    fn maybe_auto_connect_prefilled_token(&mut self) {
        if self.did_attempt_prefilled_token {
            return;
        }
        self.did_attempt_prefilled_token = true;
        if self.gateway_token.is_some() {
            self.reconnect_all_sessions();
        }
    }

    fn try_connect_session(&mut self, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };

        let token = self.gateway_token.clone();
        let buffers = self.sessions[index].buffers.clone();
        Self::close_buffers(&buffers);

        let url = match ws_chat_url(token.as_deref()) {
            Ok(url) => url,
            Err(err) => {
                *buffers.state.borrow_mut() = ConnectionState::Error(err.clone());
                self.toasts.borrow_mut().error(err);
                return;
            }
        };

        *buffers.state.borrow_mut() = ConnectionState::Connecting;
        let ws = match WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(err) => {
                let message = format!("WebSocket::new: {err:?}");
                *buffers.state.borrow_mut() = ConnectionState::Error(message.clone());
                self.toasts.borrow_mut().error(message);
                return;
            }
        };

        let messages = buffers.messages.clone();
        let active_stream_request_id = buffers.active_stream_request_id.clone();
        let state_message = buffers.state.clone();
        let toasts_message = self.toasts.clone();
        let ctx = self.ctx.clone();
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            let text = if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                String::from(text)
            } else if let Some(text) = event.data().as_string() {
                text
            } else {
                "[non-text message]".to_string()
            };
            match serde_json::from_str::<ServerFrame>(&text) {
                Ok(ServerFrame::Event { event, payload }) => match event.as_str() {
                    "session.connected" => {
                        *state_message.borrow_mut() = ConnectionState::Connected;
                        messages.borrow_mut().push(ChatMessage {
                            text: "Connected to the Klaw gateway.".to_string(),
                            role: MessageRole::System,
                            timestamp_ms: current_timestamp_ms(),
                        });
                    }
                    "session.subscribed" => {
                        let session_key = payload
                            .get("session_key")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        messages.borrow_mut().push(ChatMessage {
                            text: format!("Subscribed to session `{session_key}`."),
                            role: MessageRole::System,
                            timestamp_ms: current_timestamp_ms(),
                        });
                    }
                    "session.unsubscribed" => {
                        messages.borrow_mut().push(ChatMessage {
                            text: "Session subscription cleared.".to_string(),
                            role: MessageRole::System,
                            timestamp_ms: current_timestamp_ms(),
                        });
                    }
                    "session.message" => {
                        let request_id = payload
                            .get("request_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string);
                        let content = payload
                            .get("response")
                            .and_then(|response| response.get("content"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if content.is_empty() {
                            ctx.request_repaint();
                            return;
                        }
                        let mut history = messages.borrow_mut();
                        let should_replace = request_id.as_ref().is_some_and(|request_id| {
                            active_stream_request_id.borrow().as_deref()
                                == Some(request_id.as_str())
                        });
                        if should_replace {
                            if let Some(message) = history.last_mut() {
                                if message.role == MessageRole::Assistant {
                                    message.text = content;
                                    message.timestamp_ms = current_timestamp_ms();
                                }
                            }
                        } else {
                            history.push(ChatMessage {
                                text: content,
                                role: MessageRole::Assistant,
                                timestamp_ms: current_timestamp_ms(),
                            });
                            *active_stream_request_id.borrow_mut() = request_id;
                        }
                    }
                    "session.stream.clear" => {
                        *active_stream_request_id.borrow_mut() = None;
                    }
                    "session.stream.done" => {
                        *active_stream_request_id.borrow_mut() = None;
                    }
                    _ => {}
                },
                Ok(ServerFrame::Result { id: _, result }) => {
                    let Some(response) = result.get("response") else {
                        ctx.request_repaint();
                        return;
                    };
                    let streamed = result
                        .get("stream")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let content = response
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if !streamed && !content.is_empty() {
                        messages.borrow_mut().push(ChatMessage {
                            text: content,
                            role: MessageRole::Assistant,
                            timestamp_ms: current_timestamp_ms(),
                        });
                    }
                    *active_stream_request_id.borrow_mut() = None;
                }
                Ok(ServerFrame::Error { id: _, error }) => {
                    *active_stream_request_id.borrow_mut() = None;
                    let message = format!("{}: {}", error.code, error.message);
                    *state_message.borrow_mut() = ConnectionState::Error(message.clone());
                    toasts_message.borrow_mut().error(message.clone());
                    messages.borrow_mut().push(ChatMessage {
                        text: message,
                        role: MessageRole::System,
                        timestamp_ms: current_timestamp_ms(),
                    });
                }
                Err(_) => {
                    messages.borrow_mut().push(ChatMessage {
                        text,
                        role: MessageRole::System,
                        timestamp_ms: current_timestamp_ms(),
                    });
                }
            }
            ctx.request_repaint();
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        let state_open = buffers.state.clone();
        let messages_open = buffers.messages.clone();
        let auth_verified_open = buffers.auth_verified.clone();
        let session_key_open = session_key.to_string();
        let ws_open = ws.clone();
        let toasts_open = self.toasts.clone();
        let ctx_open = self.ctx.clone();
        let onopen = Closure::wrap(Box::new(move |_event: JsValue| {
            *auth_verified_open.borrow_mut() = true;
            *state_open.borrow_mut() = ConnectionState::Connected;
            let subscribe_id = Uuid::new_v4().to_string();
            if let Err(err) = send_method(
                &ws_open,
                &subscribe_id,
                "session.subscribe",
                json!({ "session_key": session_key_open }),
            ) {
                *state_open.borrow_mut() = ConnectionState::Error(err.clone());
                toasts_open.borrow_mut().error(err.clone());
                messages_open.borrow_mut().push(ChatMessage {
                    text: err,
                    role: MessageRole::System,
                    timestamp_ms: current_timestamp_ms(),
                });
            }
            ctx_open.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let state_error = buffers.state.clone();
        let auth_verified_error = buffers.auth_verified.clone();
        let ctx_error = self.ctx.clone();
        let onerror = Closure::wrap(Box::new(move |_event: JsValue| {
            let next_state = if *auth_verified_error.borrow() {
                ConnectionState::Error("WebSocket error before open".to_string())
            } else {
                ConnectionState::Error(
                    "Token validation failed. Check the gateway token and try again.".to_string(),
                )
            };
            *state_error.borrow_mut() = next_state;
            ctx_error.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let state_close = buffers.state.clone();
        let messages_close = buffers.messages.clone();
        let ws_cell = buffers.ws.clone();
        let auth_verified_close = buffers.auth_verified.clone();
        let suppress_close_notice = buffers.suppress_next_close_notice.clone();
        let toasts_close = self.toasts.clone();
        let ctx_close = self.ctx.clone();
        let onclose = Closure::wrap(Box::new(move |event: CloseEvent| {
            ws_cell.borrow_mut().take();
            if *suppress_close_notice.borrow() {
                *suppress_close_notice.borrow_mut() = false;
                ctx_close.request_repaint();
                return;
            }

            if *auth_verified_close.borrow() {
                *state_close.borrow_mut() = ConnectionState::Disconnected;
                messages_close.borrow_mut().push(ChatMessage {
                    text: "Connection closed.".to_string(),
                    role: MessageRole::System,
                    timestamp_ms: current_timestamp_ms(),
                });
            } else {
                let message = match &*state_close.borrow() {
                    ConnectionState::Error(message) => message.clone(),
                    _ if !event.reason().is_empty() => {
                        format!("Gateway rejected websocket connection: {}", event.reason())
                    }
                    _ => "Token validation failed. Check the gateway token and try again."
                        .to_string(),
                };
                *state_close.borrow_mut() = ConnectionState::Disconnected;
                toasts_close.borrow_mut().error(message);
            }
            ctx_close.request_repaint();
        }) as Box<dyn FnMut(CloseEvent)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        *buffers.ws.borrow_mut() = Some(ws);
    }

    fn send_session_draft(&mut self, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };
        let session = &mut self.sessions[index];
        let text = session.draft.trim().to_string();
        if text.is_empty() {
            return;
        }

        let Some(ws) = session.buffers.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }

        let request_id = Uuid::new_v4().to_string();
        session.buffers.messages.borrow_mut().push(ChatMessage {
            text: text.clone(),
            role: MessageRole::User,
            timestamp_ms: current_timestamp_ms(),
        });
        *session.buffers.active_stream_request_id.borrow_mut() = Some(request_id.clone());
        if let Err(err) = send_method(
            &ws,
            &request_id,
            "session.submit",
            json!({
                "session_key": session_key,
                "chat_id": session_key,
                "input": text,
                "stream": true,
            }),
        ) {
            *session.buffers.state.borrow_mut() = ConnectionState::Error(err);
            return;
        }
        session.draft.clear();
    }

    fn render_top_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("klaw-webui-toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("New Session").clicked() {
                    self.create_session();
                }
                if ui.button("Tile Windows").clicked() {
                    self.tile_open_sessions();
                }
                if ui.button("Reset Layout").clicked() {
                    self.reset_window_layout();
                }
                if ui.button("Gateway Token").clicked() {
                    self.show_gateway_dialog = true;
                }
                if ui.button("Reconnect All").clicked() {
                    self.reconnect_all_sessions();
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(toolbar_title()).strong());
                });
            });
        });
    }

    fn render_status_bar(&mut self, ctx: &Context) {
        let mut requested_theme = None;
        TopBottomPanel::bottom("klaw-webui-status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Theme Mode:");
                ComboBox::from_id_salt("klaw-webui-theme-mode")
                    .width(110.0)
                    .selected_text(self.theme_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
                            if ui
                                .selectable_label(self.theme_mode == mode, mode.label())
                                .clicked()
                            {
                                requested_theme = Some(mode);
                                ui.close();
                            }
                        }
                    });
                ui.separator();
                ui.label(format!("Sessions: {}", self.sessions.len()));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(active_session_key) = self.active_session_key.as_deref() {
                        if let Some(session) = self
                            .sessions
                            .iter()
                            .find(|session| session.session_key == active_session_key)
                        {
                            ui.label(session.connection_state().status_text());
                            ui.separator();
                            ui.label(&session.title);
                        }
                    } else {
                        ui.label("No active session");
                    }
                });
            });
        });

        if let Some(theme_mode) = requested_theme {
            self.set_theme_mode(theme_mode);
        }
    }

    fn session_list_order(&self) -> Vec<String> {
        let active = self.active_session_key.as_deref();
        let mut visible = Vec::new();
        let mut hidden = Vec::new();

        for session in &self.sessions {
            if active == Some(session.session_key.as_str()) {
                continue;
            }
            if session.open {
                visible.push(session.session_key.clone());
            } else {
                hidden.push(session.session_key.clone());
            }
        }

        let mut ordered = Vec::with_capacity(self.sessions.len());
        if let Some(active_session) = self
            .sessions
            .iter()
            .find(|session| active == Some(session.session_key.as_str()))
        {
            ordered.push(active_session.session_key.clone());
        }
        ordered.extend(visible);
        ordered.extend(hidden);
        ordered
    }

    fn render_session_list(&mut self, ctx: &Context) {
        let mut remove_session_key = None;
        let mut focus_session_key = None;
        let mut rename_session_key = None;

        egui::SidePanel::left("klaw-webui-sessions")
            .resizable(true)
            .default_width(SESSION_LIST_WIDTH)
            .show(ctx, |ui| {
                ui.heading("Sessions");
                ui.separator();

                if self.sessions.is_empty() {
                    ui.label("No sessions yet.");
                    return;
                }

                ScrollArea::vertical().show(ui, |ui| {
                    for session_key in self.session_list_order() {
                        let Some(index) = self.session_index(&session_key) else {
                            continue;
                        };
                        let session = &self.sessions[index];
                        let is_active = self.active_session_key.as_deref()
                            == Some(session.session_key.as_str());
                        let card = Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(regular::APP_WINDOW);
                                ui.label(
                                    RichText::new(&session.title).strong().size(if is_active {
                                        15.0
                                    } else {
                                        14.0
                                    }),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if is_active {
                                        ui.label(RichText::new("Active").small().strong());
                                    }
                                });
                            });
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(session.connection_state().status_text()).small(),
                                );
                                ui.separator();
                                ui.label(
                                    RichText::new(if session.open { "Visible" } else { "Hidden" })
                                        .small(),
                                );
                            });
                            ui.add_space(2.0);
                            ui.label(RichText::new(&session.session_key).small().weak());
                        });
                        if card.response.clicked() {
                            focus_session_key = Some(session.session_key.clone());
                        }
                        card.response.context_menu(|ui| {
                            if ui
                                .button(format!("{} Rename", regular::PENCIL_SIMPLE))
                                .clicked()
                            {
                                rename_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                            if ui.button(format!("{} Delete", regular::TRASH)).clicked() {
                                remove_session_key = Some(session.session_key.clone());
                                ui.close();
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
            });

        if let Some(session_key) = focus_session_key {
            self.focus_session(&session_key);
        }
        if let Some(session_key) = rename_session_key
            && let Some(index) = self.session_index(&session_key)
        {
            self.rename_session_key = Some(session_key);
            self.rename_session_input = self.sessions[index].title.clone();
        }
        if let Some(session_key) = remove_session_key {
            self.remove_session(&session_key);
        }
    }

    fn render_rename_dialog(&mut self, ctx: &Context) {
        let Some(session_key) = self.rename_session_key.clone() else {
            return;
        };

        let mut open = true;
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Rename Session")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let response = ui.add(
                    TextEdit::singleline(&mut self.rename_session_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("Session name"),
                );
                let submit_with_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() || submit_with_enter {
                        submit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if submit {
            let trimmed = self.rename_session_input.trim();
            if !trimmed.is_empty()
                && let Some(index) = self.session_index(&session_key)
            {
                self.sessions[index].title = trimmed.to_string();
                self.persist_workspace_state();
            }
            self.rename_session_key = None;
            self.rename_session_input.clear();
            return;
        }

        if cancel || !open {
            self.rename_session_key = None;
            self.rename_session_input.clear();
        }
    }

    fn render_gateway_dialog(&mut self, ctx: &Context) {
        if !self.show_gateway_dialog {
            return;
        }

        let mut open = self.show_gateway_dialog;
        let mut reconnect_all = false;

        egui::Window::new("Gateway Token")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label("If gateway auth is enabled, enter the token here.");
                ui.label(
                    RichText::new("Leave it blank when auth is disabled.")
                        .small()
                        .weak(),
                );
                ui.add_space(8.0);

                let response = ui.add(
                    TextEdit::singleline(&mut self.gateway_token_input)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text("Gateway token"),
                );
                let submit_with_enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save & Reconnect").clicked() || submit_with_enter {
                        reconnect_all = true;
                    }
                    if ui.button("Clear").clicked() {
                        self.gateway_token_input.clear();
                        self.gateway_token = None;
                    }
                });
            });

        self.show_gateway_dialog = open;

        if reconnect_all {
            self.gateway_token = normalize_gateway_token_input(&self.gateway_token_input);
            self.reconnect_all_sessions();
            self.show_gateway_dialog = false;
        }
    }

    fn render_empty_state(ui: &mut egui::Ui, state: &ConnectionState) {
        let copy = state.empty_state_copy();
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(copy.title).heading().strong());
            ui.add_space(4.0);
            ui.label(RichText::new(copy.body).weak());
        });
    }

    fn render_message(ui: &mut egui::Ui, message: &ChatMessage) {
        let time_label = format_message_timestamp(message.timestamp_ms);
        match message.role {
            MessageRole::System => {
                ui.vertical_centered(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("System").small().strong().weak());
                        ui.label(RichText::new(time_label).small().weak());
                    });
                    ui.label(RichText::new(&message.text).small().weak());
                });
            }
            MessageRole::Assistant | MessageRole::User => {
                let role_label = match message.role {
                    MessageRole::Assistant => "Klaw",
                    MessageRole::User => "You",
                    MessageRole::System => "System",
                };
                let layout = if matches!(message.role, MessageRole::User) {
                    Layout::right_to_left(Align::TOP)
                } else {
                    Layout::left_to_right(Align::TOP)
                };
                ui.with_layout(layout, |ui| {
                    Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_max_width(BUBBLE_MAX_WIDTH);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(role_label).strong());
                            ui.label(RichText::new(time_label).small().weak());
                        });
                        ui.add_space(4.0);
                        ui.label(&message.text);
                    });
                });
            }
        }
    }

    fn render_session_window(&mut self, ctx: &Context, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };

        let mut trigger_send = false;
        let mut trigger_connect = false;
        let mut trigger_disconnect = false;
        let mut set_active = false;
        let mut persist_after_render = false;
        {
            let session = &mut self.sessions[index];
            let state = session.connection_state();
            let messages = session.buffers.messages.borrow().clone();
            let error_text = match &state {
                ConnectionState::Error(message) => Some(message.clone()),
                _ => None,
            };
            let mut open = session.open;

            let window = egui::Window::new(&session.title)
                .id(Id::new(("session-window", &session.session_key)))
                .default_pos(session.window_anchor.to_pos2())
                .default_size([SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_DEFAULT_HEIGHT])
                .min_width(SESSION_WINDOW_MIN_WIDTH)
                .min_height(SESSION_WINDOW_MIN_HEIGHT)
                .open(&mut open);

            if let Some(inner) = window.show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&session.session_key).small().weak());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let button_label = if matches!(state, ConnectionState::Connected) {
                            "Disconnect"
                        } else if matches!(state, ConnectionState::Connecting) {
                            "Connecting…"
                        } else {
                            "Connect"
                        };
                        let button = ui.add_enabled(
                            !matches!(state, ConnectionState::Connecting),
                            Button::new(button_label),
                        );
                        if button.clicked() {
                            if matches!(state, ConnectionState::Connected) {
                                trigger_disconnect = true;
                            } else {
                                trigger_connect = true;
                            }
                        }
                        ui.label(state.status_text());
                    });
                });
                if let Some(message) = error_text {
                    ui.label(RichText::new(message).small().weak());
                }
                ui.separator();

                let messages_height = (ui.available_height() - INPUT_PANEL_HEIGHT).max(140.0);
                ui.allocate_ui(vec2(ui.available_width(), messages_height), |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            if messages.is_empty() {
                                Self::render_empty_state(ui, &state);
                                return;
                            }

                            for message in &messages {
                                Self::render_message(ui, message);
                                ui.add_space(8.0);
                            }
                        });
                });

                ui.separator();
                ui.vertical(|ui| {
                    let input = TextEdit::multiline(&mut session.draft)
                        .desired_rows(3)
                        .hint_text(state.composer_hint_text())
                        .interactive(state.can_send());
                    let response = ui.add_sized([ui.available_width(), 72.0], input);
                    let shortcut = response.has_focus()
                        && ui.input(|input| {
                            input.key_pressed(Key::Enter)
                                && (input.modifiers.command || input.modifiers.ctrl)
                        });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let helper_text = if state.can_send() {
                            "Cmd/Ctrl+Enter to send"
                        } else {
                            state.composer_hint_text()
                        };
                        ui.label(RichText::new(helper_text).small().weak());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let send_button = ui.add_enabled(state.can_send(), Button::new("Send"));
                            if send_button.clicked() || shortcut {
                                trigger_send = true;
                            }
                        });
                    });
                });
            }) {
                set_active = inner.response.clicked();
                let next_anchor = WindowAnchor::from_pos2(inner.response.rect.min);
                if session.window_anchor != next_anchor {
                    session.window_anchor = next_anchor;
                    persist_after_render = true;
                }
            }

            session.open = open;
            let should_remove = !open;
            if should_remove {
                self.persist_workspace_state();
                return;
            }
        }

        let became_active = set_active && self.active_session_key.as_deref() != Some(session_key);
        let moved_to_front = if set_active {
            self.bring_session_to_front(session_key)
        } else {
            false
        };
        if became_active {
            self.active_session_key = Some(session_key.to_string());
        }
        if became_active || moved_to_front {
            self.persist_workspace_state();
        }
        if trigger_disconnect {
            if let Some(index) = self.session_index(session_key) {
                Self::close_buffers(&self.sessions[index].buffers);
            }
        }
        if trigger_connect {
            self.try_connect_session(session_key);
        }
        if trigger_send {
            self.send_session_draft(session_key);
        }
        if persist_after_render {
            self.persist_workspace_state();
        }
    }

    fn session_render_order(&self) -> Vec<String> {
        let active = self.active_session_key.as_deref();
        let mut ordered = self
            .sessions
            .iter()
            .filter(|session| active != Some(session.session_key.as_str()))
            .map(|session| session.session_key.clone())
            .collect::<Vec<_>>();
        if let Some(active_session) = self
            .sessions
            .iter()
            .find(|session| active == Some(session.session_key.as_str()))
        {
            ordered.push(active_session.session_key.clone());
        }
        ordered
    }

    fn render_workbench(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sessions.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("No sessions open. Click New Session to start.");
                });
                return;
            }

            ui.label(RichText::new("Workbench").strong());
            ui.label(
                RichText::new("Each session opens as its own egui window.")
                    .small()
                    .weak(),
            );
        });

        for session_key in self.session_render_order() {
            if self
                .session_index(&session_key)
                .and_then(|index| self.sessions.get(index))
                .is_some_and(|session| session.open)
            {
                self.render_session_window(ctx, &session_key);
            }
        }
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.apply_theme();
        self.maybe_auto_connect_prefilled_token();
        self.render_top_bar(ctx);
        self.render_status_bar(ctx);
        self.render_session_list(ctx);
        self.render_workbench(ctx);
        self.render_gateway_dialog(ctx);
        self.render_rename_dialog(ctx);
        self.toasts.borrow_mut().show(ctx);
    }
}
