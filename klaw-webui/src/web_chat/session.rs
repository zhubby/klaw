use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{MessageRole, SessionListEntry};
use eframe::epaint::{Color32, FontFamily, FontId};
use js_sys::Date;
use klaw_ui_kit::text_animator::{AnimationType, TextAnimator};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{markdown::MarkdownCache, storage::PersistedSession};

pub(super) const BUBBLE_MAX_WIDTH: f32 = 420.0;
pub(super) const SESSION_LIST_WIDTH: f32 = 240.0;
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
    pub(in crate::web_chat) id: String,
    pub(in crate::web_chat) text: String,
    pub(in crate::web_chat) role: MessageRole,
    pub(in crate::web_chat) timestamp_ms: i64,
}

impl ChatMessage {
    pub(super) fn new(text: String, role: MessageRole, timestamp_ms: i64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            text,
            role,
            timestamp_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WindowAnchor {
    pub(in crate::web_chat) x: i32,
    pub(in crate::web_chat) y: i32,
}

impl WindowAnchor {
    pub(super) fn to_pos2(self) -> egui::Pos2 {
        egui::pos2(self.x as f32, self.y as f32)
    }
}

pub(super) fn session_window_id(session_key: &str) -> egui::Id {
    egui::Id::new(("session-window", session_key))
}

#[derive(Clone)]
pub(super) struct SessionBuffers {
    pub(in crate::web_chat) messages: Rc<RefCell<Vec<ChatMessage>>>,
    pub(in crate::web_chat) active_stream_request_id: Rc<RefCell<Option<String>>>,
    pub(in crate::web_chat) history_loaded: Rc<RefCell<bool>>,
}

impl Default for SessionBuffers {
    fn default() -> Self {
        Self {
            messages: Rc::new(RefCell::new(Vec::new())),
            active_stream_request_id: Rc::new(RefCell::new(None)),
            history_loaded: Rc::new(RefCell::new(false)),
        }
    }
}

pub(super) struct SessionWindow {
    pub(in crate::web_chat) session_key: String,
    pub(in crate::web_chat) title: String,
    pub(in crate::web_chat) created_at_ms: i64,
    pub(in crate::web_chat) draft: String,
    pub(in crate::web_chat) open: bool,
    pub(in crate::web_chat) window_anchor: WindowAnchor,
    pub(in crate::web_chat) buffers: SessionBuffers,
    pub(in crate::web_chat) markdown_cache: MarkdownCache,
    pub(in crate::web_chat) fade_in_messages: HashMap<String, TextAnimator>,
}

impl SessionWindow {
    pub(super) fn new(metadata: SessionListEntry, open: bool) -> Self {
        Self {
            session_key: metadata.session_key,
            title: metadata.title,
            created_at_ms: metadata.created_at_ms,
            draft: String::new(),
            open,
            window_anchor: window_anchor_for_slot(0),
            buffers: SessionBuffers::default(),
            markdown_cache: MarkdownCache::default(),
            fade_in_messages: HashMap::new(),
        }
    }

    pub(super) fn metadata(&self) -> PersistedSession {
        PersistedSession {
            session_key: self.session_key.clone(),
            open: self.open,
        }
    }

    pub(super) fn register_fade_in_message(&mut self, message: &ChatMessage) {
        self.fade_in_messages
            .entry(message.id.clone())
            .or_insert_with(|| {
                TextAnimator::new(
                    &message.text,
                    FontId::new(14.0, FontFamily::Proportional),
                    Color32::WHITE,
                    0.6,
                    AnimationType::FadeIn,
                )
            });
    }

    pub(super) fn prune_finished_animations(&mut self) {
        self.fade_in_messages
            .retain(|_, animator| !animator.is_animation_finished());
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

pub(super) fn current_timestamp_ms() -> i64 {
    Date::now().round() as i64
}

pub(super) fn format_message_timestamp(timestamp_ms: i64) -> String {
    let date = Date::new(&wasm_bindgen::JsValue::from_f64(timestamp_ms as f64));
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
}

pub(super) fn format_datetime(timestamp_ms: i64) -> String {
    let date = Date::new(&wasm_bindgen::JsValue::from_f64(timestamp_ms as f64));
    format!(
        "{}/{:02}/{:02} {:02}:{:02}:{:02}",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date(),
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds(),
    )
}

pub(super) fn format_relative_time(created_at_ms: i64, now_ms: i64) -> String {
    let elapsed_ms = (now_ms - created_at_ms).max(0);
    let elapsed_secs = elapsed_ms / 1000;
    if elapsed_secs < 60 {
        "just now".to_string()
    } else if elapsed_secs < 3600 {
        let mins = elapsed_secs / 60;
        format!("{mins}m ago")
    } else if elapsed_secs < 86400 {
        let hours = elapsed_secs / 3600;
        format!("{hours}h ago")
    } else if elapsed_secs < 604800 {
        let days = elapsed_secs / 86400;
        format!("{days}d ago")
    } else {
        let weeks = elapsed_secs / 604800;
        format!("{weeks}w ago")
    }
}
