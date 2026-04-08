//! WASM-only egui chat client for `/ws/chat`.

use std::{cell::RefCell, rc::Rc};

use eframe::egui::{self, ScrollArea, TextEdit};
use uuid::Uuid;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, WebSocket};

const SESSION_STORAGE_KEY: &str = "klaw_webui_session_key";

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConnState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// Start the chat UI on the given canvas (install from `index.html` via wasm-bindgen).
#[wasm_bindgen]
pub fn start_chat_ui(canvas: web_sys::HtmlCanvasElement) {
    console_error_panic_hook::set_once();
    let web_options = eframe::WebOptions::default();
    let runner = eframe::WebRunner::new();
    wasm_bindgen_futures::spawn_local(async move {
        runner
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(ChatApp::new(cc)))),
            )
            .await
            .expect("eframe web start failed");
    });
}

fn load_or_create_session_key() -> String {
    let Some(window) = web_sys::window() else {
        return format!("web:{}", Uuid::new_v4());
    };
    let Some(storage) = window.local_storage().ok().flatten() else {
        return format!("web:{}", Uuid::new_v4());
    };
    if let Ok(Some(existing)) = storage.get_item(SESSION_STORAGE_KEY) {
        if let Some(rest) = existing.strip_prefix("web:") {
            if Uuid::parse_str(rest).is_ok() {
                return existing;
            }
        }
    }
    let key = format!("web:{}", Uuid::new_v4());
    let _ = storage.set_item(SESSION_STORAGE_KEY, &key);
    key
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

fn ws_chat_url(session_key: &str, token: Option<&str>) -> Result<String, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let loc = window.location();
    let protocol = loc
        .protocol()
        .map_err(|_| "location.protocol unavailable".to_string())?;
    let ws_scheme = if protocol == "https:" { "wss" } else { "ws" };
    let host = loc
        .host()
        .map_err(|_| "location.host unavailable".to_string())?;
    let mut url = format!(
        "{}://{}/ws/chat?session_key={}",
        ws_scheme,
        host,
        urlencoding::encode(session_key)
    );
    if let Some(t) = token {
        url.push_str("&token=");
        url.push_str(&urlencoding::encode(t));
    }
    Ok(url)
}

pub struct ChatApp {
    session_key: String,
    gateway_token: Option<String>,
    ctx: egui::Context,
    lines: Rc<RefCell<Vec<String>>>,
    state: Rc<RefCell<ConnState>>,
    ws: Rc<RefCell<Option<WebSocket>>>,
    draft: String,
    did_request_initial_connect: bool,
}

impl ChatApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            session_key: load_or_create_session_key(),
            gateway_token: gateway_token_from_page(),
            ctx: cc.egui_ctx.clone(),
            lines: Rc::new(RefCell::new(Vec::new())),
            state: Rc::new(RefCell::new(ConnState::Disconnected)),
            ws: Rc::new(RefCell::new(None)),
            draft: String::new(),
            did_request_initial_connect: false,
        }
    }

    fn close_socket(&mut self) {
        if let Some(ws) = self.ws.borrow_mut().take() {
            let _ = ws.close();
        }
        *self.state.borrow_mut() = ConnState::Disconnected;
    }

    fn try_connect(&mut self) {
        self.close_socket();
        let url = match ws_chat_url(&self.session_key, self.gateway_token.as_deref()) {
            Ok(u) => u,
            Err(e) => {
                *self.state.borrow_mut() = ConnState::Error(e);
                return;
            }
        };
        *self.state.borrow_mut() = ConnState::Connecting;
        let ws = match WebSocket::new(&url.as_str()) {
            Ok(w) => w,
            Err(e) => {
                *self.state.borrow_mut() = ConnState::Error(format!("WebSocket::new: {e:?}"));
                return;
            }
        };

        let lines = self.lines.clone();
        let ctx = self.ctx.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            let text = if let Ok(s) = e.data().dyn_into::<js_sys::JsString>() {
                String::from(s)
            } else if let Some(s) = e.data().as_string() {
                s
            } else {
                "[non-text message]".to_string()
            };
            lines.borrow_mut().push(text);
            ctx.request_repaint();
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        let state_open = self.state.clone();
        let lines_open = self.lines.clone();
        let ctx_open = self.ctx.clone();
        let onopen = Closure::wrap(Box::new(move |_e: JsValue| {
            *state_open.borrow_mut() = ConnState::Connected;
            lines_open
                .borrow_mut()
                .push("[connected to gateway room]".to_string());
            ctx_open.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let state_err = self.state.clone();
        let ctx_err = self.ctx.clone();
        let onerror = Closure::wrap(Box::new(move |_e: JsValue| {
            if *state_err.borrow() == ConnState::Connecting {
                *state_err.borrow_mut() =
                    ConnState::Error("WebSocket error before open".to_string());
            }
            ctx_err.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let state_close = self.state.clone();
        let ctx_close = self.ctx.clone();
        let ws_cell = self.ws.clone();
        let onclose = Closure::wrap(Box::new(move |_e: JsValue| {
            ws_cell.borrow_mut().take();
            *state_close.borrow_mut() = ConnState::Disconnected;
            ctx_close.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        *self.ws.borrow_mut() = Some(ws);
    }

    fn send_draft(&mut self) {
        let text = self.draft.trim().to_string();
        if text.is_empty() {
            return;
        }
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        if ws.send_with_str(&text).is_err() {
            *self.state.borrow_mut() = ConnState::Error("send failed".to_string());
            return;
        }
        self.draft.clear();
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.did_request_initial_connect {
            self.did_request_initial_connect = true;
            self.try_connect();
        }

        let status_text = match self.state.borrow().clone() {
            ConnState::Disconnected => "Disconnected".to_string(),
            ConnState::Connecting => "Connecting…".to_string(),
            ConnState::Connected => "Connected".to_string(),
            ConnState::Error(ref e) => format!("Error: {e}"),
        };

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Session: {}", self.session_key));
                ui.separator();
                ui.label(&status_text);
                if ui.button("Reconnect").clicked() {
                    self.try_connect();
                }
                if ui.button("Disconnect").clicked() {
                    self.close_socket();
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Messages broadcast within this session room (plain text).");
            ui.separator();

            let lines_len = self.lines.borrow().len();
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    let lines = self.lines.borrow();
                    for line in lines.iter() {
                        ui.label(egui::RichText::new(line).monospace());
                    }
                    drop(lines);
                    if lines_len == 0 {
                        ui.label("(no messages yet)");
                    }
                });

            ui.separator();
            ui.horizontal(|ui| {
                let re = TextEdit::singleline(&mut self.draft)
                    .desired_width(f32::INFINITY)
                    .hint_text("Type a message…");
                ui.add(re);
                if ui.button("Send").clicked() {
                    self.send_draft();
                }
            });
        });
    }
}
