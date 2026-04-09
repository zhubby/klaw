use std::{cell::RefCell, rc::Rc};

use crate::{ConnectionState, MessageRole};
use js_sys::Date;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use web_sys::WebSocket;

use super::storage::PersistedSession;

pub(super) const BUBBLE_MAX_WIDTH: f32 = 420.0;
pub(super) const SESSION_LIST_WIDTH: f32 = 220.0;
pub(super) const SESSION_WINDOW_DEFAULT_WIDTH: f32 = 560.0;
pub(super) const SESSION_WINDOW_DEFAULT_HEIGHT: f32 = 620.0;
pub(super) const SESSION_WINDOW_MIN_WIDTH: f32 = 360.0;
pub(super) const SESSION_WINDOW_MIN_HEIGHT: f32 = 420.0;
pub(super) const INPUT_PANEL_HEIGHT: f32 = 124.0;

const WINDOW_START_X: f32 = SESSION_LIST_WIDTH + 28.0;
const WINDOW_START_Y: f32 = 72.0;
const WINDOW_OFFSET_X: f32 = 40.0;
const WINDOW_OFFSET_Y: f32 = 32.0;
const WINDOW_STAGGER_COLUMNS: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChatMessage {
    pub(in crate::web_chat) text: String,
    pub(in crate::web_chat) role: MessageRole,
    pub(in crate::web_chat) timestamp_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WindowAnchor {
    pub(in crate::web_chat) x: i32,
    pub(in crate::web_chat) y: i32,
}

impl WindowAnchor {
    pub(super) fn from_pos2(pos: egui::Pos2) -> Self {
        Self {
            x: pos.x.round() as i32,
            y: pos.y.round() as i32,
        }
    }

    pub(super) fn to_pos2(self) -> egui::Pos2 {
        egui::pos2(self.x as f32, self.y as f32)
    }
}

#[derive(Clone)]
pub(super) struct SessionBuffers {
    pub(in crate::web_chat) messages: Rc<RefCell<Vec<ChatMessage>>>,
    pub(in crate::web_chat) state: Rc<RefCell<ConnectionState>>,
    pub(in crate::web_chat) ws: Rc<RefCell<Option<WebSocket>>>,
    pub(in crate::web_chat) auth_verified: Rc<RefCell<bool>>,
    pub(in crate::web_chat) suppress_next_close_notice: Rc<RefCell<bool>>,
    pub(in crate::web_chat) active_stream_request_id: Rc<RefCell<Option<String>>>,
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

pub(super) struct SessionWindow {
    pub(in crate::web_chat) session_key: String,
    pub(in crate::web_chat) title: String,
    pub(in crate::web_chat) draft: String,
    pub(in crate::web_chat) open: bool,
    pub(in crate::web_chat) window_anchor: WindowAnchor,
    pub(in crate::web_chat) buffers: SessionBuffers,
}

impl SessionWindow {
    pub(super) fn new(metadata: PersistedSession) -> Self {
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

    pub(super) fn metadata(&self) -> PersistedSession {
        PersistedSession {
            session_key: self.session_key.clone(),
            title: self.title.clone(),
            open: self.open,
            window_anchor: Some(self.window_anchor),
        }
    }

    pub(super) fn connection_state(&self) -> ConnectionState {
        self.buffers.state.borrow().clone()
    }
}

pub(super) fn window_anchor_for_slot(slot: u32) -> WindowAnchor {
    let column = slot % WINDOW_STAGGER_COLUMNS;
    let row = slot / WINDOW_STAGGER_COLUMNS;
    WindowAnchor {
        x: (WINDOW_START_X + column as f32 * WINDOW_OFFSET_X).round() as i32,
        y: (WINDOW_START_Y + row as f32 * WINDOW_OFFSET_Y).round() as i32,
    }
}

pub(super) fn anchor_rect(anchor: WindowAnchor) -> egui::Rect {
    egui::Rect::from_min_size(
        anchor.to_pos2(),
        egui::vec2(SESSION_WINDOW_DEFAULT_WIDTH, SESSION_WINDOW_DEFAULT_HEIGHT),
    )
}

pub(super) fn current_timestamp_ms() -> i64 {
    Date::now().round() as i64
}

pub(super) fn format_message_timestamp(timestamp_ms: i64) -> String {
    let date = Date::new(&wasm_bindgen::JsValue::from_f64(timestamp_ms as f64));
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
}

pub(super) fn generate_session_key() -> String {
    format!("web:{}", Uuid::new_v4())
}

pub(super) fn is_valid_session_key(session_key: &str) -> bool {
    let Some(rest) = session_key.strip_prefix("web:") else {
        return false;
    };
    Uuid::parse_str(rest).is_ok()
}

pub(super) fn session_title(number: u32) -> String {
    format!("Session {number}")
}
