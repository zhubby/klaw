//! WASM-only egui chat client for `/ws/chat`.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::{
    ConnectionState, MessageRole, classify_message_role, normalize_gateway_token_input,
    toolbar_title,
};
use eframe::egui::{
    self, Align, Align2, Color32, Frame, Key, RichText, ScrollArea, Stroke, TextEdit, vec2,
};
use egui_notify::{Anchor, Toasts};
use uuid::Uuid;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

const CHAT_COLUMN_MAX_WIDTH: f32 = 760.0;
const COMPOSER_MAX_WIDTH: f32 = 760.0;
const BUBBLE_MAX_WIDTH: f32 = 520.0;
const SESSION_STORAGE_KEY: &str = "klaw_webui_session_key";

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChatMessage {
    text: String,
    role: MessageRole,
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
    gateway_token_input: String,
    ctx: egui::Context,
    messages: Rc<RefCell<Vec<ChatMessage>>>,
    pending_local_echoes: Rc<RefCell<VecDeque<String>>>,
    state: Rc<RefCell<ConnectionState>>,
    ws: Rc<RefCell<Option<WebSocket>>>,
    auth_verified: Rc<RefCell<bool>>,
    suppress_next_close_notice: Rc<RefCell<bool>>,
    toasts: Rc<RefCell<Toasts>>,
    draft: String,
    did_attempt_prefilled_token: bool,
}

impl ChatApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let gateway_token = gateway_token_from_page();
        let gateway_token_input = gateway_token.clone().unwrap_or_default();
        let toasts = Toasts::new()
            .with_anchor(Anchor::BottomRight)
            .with_margin(vec2(16.0, 16.0));
        Self {
            session_key: load_or_create_session_key(),
            gateway_token,
            gateway_token_input,
            ctx: cc.egui_ctx.clone(),
            messages: Rc::new(RefCell::new(Vec::new())),
            pending_local_echoes: Rc::new(RefCell::new(VecDeque::new())),
            state: Rc::new(RefCell::new(ConnectionState::Disconnected)),
            ws: Rc::new(RefCell::new(None)),
            auth_verified: Rc::new(RefCell::new(false)),
            suppress_next_close_notice: Rc::new(RefCell::new(false)),
            toasts: Rc::new(RefCell::new(toasts)),
            draft: String::new(),
            did_attempt_prefilled_token: false,
        }
    }

    fn connection_state(&self) -> ConnectionState {
        self.state.borrow().clone()
    }

    fn is_authenticated(&self) -> bool {
        *self.auth_verified.borrow()
    }

    fn close_socket(&mut self) {
        if let Some(ws) = self.ws.borrow_mut().take() {
            *self.suppress_next_close_notice.borrow_mut() = true;
            let _ = ws.close();
        }
        *self.state.borrow_mut() = ConnectionState::Disconnected;
    }

    fn maybe_auto_validate_prefilled_token(&mut self) {
        if self.did_attempt_prefilled_token {
            return;
        }
        self.did_attempt_prefilled_token = true;
        if self.gateway_token.is_some() {
            self.try_connect();
        }
    }

    fn begin_gateway_auth(&mut self) {
        self.gateway_token = normalize_gateway_token_input(&self.gateway_token_input);
        self.try_connect();
    }

    fn try_connect(&mut self) {
        self.close_socket();
        let url = match ws_chat_url(&self.session_key, self.gateway_token.as_deref()) {
            Ok(u) => u,
            Err(e) => {
                *self.state.borrow_mut() = ConnectionState::Error(e.clone());
                if !self.is_authenticated() {
                    self.toasts.borrow_mut().error(e);
                }
                return;
            }
        };
        *self.state.borrow_mut() = ConnectionState::Connecting;
        let ws = match WebSocket::new(&url.as_str()) {
            Ok(w) => w,
            Err(e) => {
                let message = format!("WebSocket::new: {e:?}");
                *self.state.borrow_mut() = ConnectionState::Error(message.clone());
                if !self.is_authenticated() {
                    self.toasts.borrow_mut().error(message);
                }
                return;
            }
        };

        let messages = self.messages.clone();
        let pending_local_echoes = self.pending_local_echoes.clone();
        let ctx = self.ctx.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            let text = if let Ok(s) = e.data().dyn_into::<js_sys::JsString>() {
                String::from(s)
            } else if let Some(s) = e.data().as_string() {
                s
            } else {
                "[non-text message]".to_string()
            };
            let role = classify_message_role(&mut pending_local_echoes.borrow_mut(), text.as_str());
            messages.borrow_mut().push(ChatMessage { text, role });
            ctx.request_repaint();
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        let state_open = self.state.clone();
        let messages_open = self.messages.clone();
        let ctx_open = self.ctx.clone();
        let auth_verified_open = self.auth_verified.clone();
        let onopen = Closure::wrap(Box::new(move |_e: JsValue| {
            *auth_verified_open.borrow_mut() = true;
            *state_open.borrow_mut() = ConnectionState::Connected;
            messages_open.borrow_mut().push(ChatMessage {
                text: "Connected to the Klaw room.".to_string(),
                role: MessageRole::System,
            });
            ctx_open.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let state_err = self.state.clone();
        let ctx_err = self.ctx.clone();
        let auth_verified_err = self.auth_verified.clone();
        let onerror = Closure::wrap(Box::new(move |_e: JsValue| {
            if *auth_verified_err.borrow() {
                if *state_err.borrow() == ConnectionState::Connecting {
                    *state_err.borrow_mut() =
                        ConnectionState::Error("WebSocket error before open".to_string());
                }
            } else {
                *state_err.borrow_mut() = ConnectionState::Error(
                    "Token validation failed. Check the gateway token and try again.".to_string(),
                );
            }
            ctx_err.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let state_close = self.state.clone();
        let messages_close = self.messages.clone();
        let ctx_close = self.ctx.clone();
        let ws_cell = self.ws.clone();
        let auth_verified_close = self.auth_verified.clone();
        let suppress_close_notice = self.suppress_next_close_notice.clone();
        let toasts_close = self.toasts.clone();
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
        self.pending_local_echoes
            .borrow_mut()
            .push_back(text.clone());
        if ws.send_with_str(&text).is_err() {
            *self.state.borrow_mut() = ConnectionState::Error("send failed".to_string());
            return;
        }
        self.draft.clear();
    }

    fn render_auth_gate(&mut self, ctx: &egui::Context) {
        let is_connecting = matches!(self.connection_state(), ConnectionState::Connecting);
        let action_label = if is_connecting {
            "Validating…"
        } else {
            "Connect"
        };

        egui::CentralPanel::default()
            .frame(Frame::new().fill(Color32::from_rgb(14, 17, 24)))
            .show(ctx, |_ui| {});

        egui::Window::new("Gateway Token")
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .title_bar(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Enter the gateway token before opening chat.")
                            .strong()
                            .size(18.0)
                            .color(Color32::from_rgb(236, 240, 248)),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "If gateway auth is disabled, you can leave this field blank and connect directly.",
                        )
                        .color(Color32::from_rgb(152, 160, 178)),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!("Session {}", self.session_key))
                            .size(11.0)
                            .color(Color32::from_rgb(132, 140, 158)),
                    );
                    ui.add_space(10.0);

                    let response = ui.add_enabled(
                        !is_connecting,
                        TextEdit::singleline(&mut self.gateway_token_input)
                            .password(true)
                            .desired_width(f32::INFINITY)
                            .hint_text("Gateway token"),
                    );
                    let submit_with_enter =
                        response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(self.connection_state().status_text())
                                .color(Color32::from_rgb(240, 190, 110))
                                .strong(),
                        );
                        let connect_clicked = ui
                            .add_enabled(!is_connecting, egui::Button::new(action_label))
                            .clicked();
                        if connect_clicked || submit_with_enter {
                            self.begin_gateway_auth();
                        }
                    });
                });
            });
    }

    fn render_top_bar(&mut self, ctx: &egui::Context, state: &ConnectionState) {
        let status_color = match state {
            ConnectionState::Connected => Color32::from_rgb(123, 216, 157),
            ConnectionState::Connecting => Color32::from_rgb(240, 190, 110),
            ConnectionState::Disconnected => Color32::from_rgb(164, 169, 188),
            ConnectionState::Error(_) => Color32::from_rgb(255, 130, 130),
        };

        egui::TopBottomPanel::top("toolbar")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(20, 24, 31))
                    .inner_margin(12.0)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(37, 45, 57))),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(toolbar_title())
                                .strong()
                                .size(16.0)
                                .color(Color32::from_rgb(236, 240, 248)),
                        );
                        ui.label(
                            RichText::new(format!("Session {}", self.session_key))
                                .size(11.0)
                                .color(Color32::from_rgb(132, 140, 158)),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if !matches!(state, ConnectionState::Connected)
                            && ui.small_button("Reconnect").clicked()
                        {
                            self.try_connect();
                        }
                        ui.label(
                            RichText::new(state.status_text())
                                .color(status_color)
                                .strong(),
                        );
                    });
                });
            });
    }

    fn render_empty_state(&self, ui: &mut egui::Ui, state: &ConnectionState) {
        let copy = state.empty_state_copy();
        ui.add_space(72.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(copy.title)
                    .size(28.0)
                    .strong()
                    .color(Color32::from_rgb(238, 242, 248)),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(copy.body)
                    .size(14.0)
                    .color(Color32::from_rgb(152, 160, 178)),
            );
        });
    }

    fn render_message(&self, ui: &mut egui::Ui, message: &ChatMessage) {
        match message.role {
            MessageRole::System => {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(&message.text)
                            .size(11.0)
                            .color(Color32::from_rgb(130, 138, 154)),
                    );
                });
            }
            MessageRole::Assistant | MessageRole::User => {
                let fill = match message.role {
                    MessageRole::Assistant => Color32::from_rgb(46, 61, 84),
                    MessageRole::User => Color32::from_rgb(228, 233, 241),
                    MessageRole::System => Color32::TRANSPARENT,
                };
                let text_color = match message.role {
                    MessageRole::User => Color32::from_rgb(20, 24, 31),
                    _ => Color32::from_rgb(241, 244, 248),
                };
                let align = if matches!(message.role, MessageRole::User) {
                    egui::Layout::right_to_left(Align::TOP)
                } else {
                    egui::Layout::left_to_right(Align::TOP)
                };

                ui.with_layout(align, |ui| {
                    Frame::group(ui.style())
                        .fill(fill)
                        .stroke(Stroke::new(1.0, Color32::from_rgb(60, 70, 88)))
                        .corner_radius(18.0)
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.set_max_width(BUBBLE_MAX_WIDTH);
                            ui.label(RichText::new(&message.text).size(14.0).color(text_color));
                        });
                });
            }
        }
    }

    fn render_chat_surface(&self, ui: &mut egui::Ui, state: &ConnectionState) {
        let messages = self.messages.borrow();
        ui.vertical_centered(|ui| {
            ui.set_max_width(CHAT_COLUMN_MAX_WIDTH);
            ui.set_width(ui.available_width().min(CHAT_COLUMN_MAX_WIDTH));

            if messages.is_empty() {
                self.render_empty_state(ui, state);
                return;
            }

            ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add_space(12.0);
                    for message in messages.iter() {
                        self.render_message(ui, message);
                        ui.add_space(10.0);
                    }
                    ui.add_space(8.0);
                });
        });
    }

    fn render_composer(&mut self, ctx: &egui::Context, state: &ConnectionState) {
        let can_send = state.can_send();
        let hint = state.composer_hint_text();

        egui::TopBottomPanel::bottom("composer")
            .frame(
                Frame::new()
                    .fill(Color32::from_rgb(16, 20, 28))
                    .inner_margin(16.0)
                    .stroke(Stroke::new(1.0, Color32::from_rgb(36, 42, 54))),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_max_width(COMPOSER_MAX_WIDTH);
                    ui.set_width(ui.available_width().min(COMPOSER_MAX_WIDTH));

                    Frame::group(ui.style())
                        .fill(Color32::from_rgb(27, 33, 44))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(52, 60, 74)))
                        .corner_radius(22.0)
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                let input = TextEdit::multiline(&mut self.draft)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(3)
                                    .hint_text(hint)
                                    .interactive(can_send);
                                ui.add_sized([ui.available_width() - 84.0, 72.0], input);

                                let send_button =
                                    ui.add_enabled(can_send, egui::Button::new("Send"));
                                if send_button.clicked() {
                                    self.send_draft();
                                }
                            });
                        });
                });
            });
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.style_mut(|style| {
            style.visuals.panel_fill = Color32::from_rgb(14, 17, 24);
            style.visuals.override_text_color = Some(Color32::from_rgb(235, 239, 245));
        });

        self.maybe_auto_validate_prefilled_token();
        if !self.is_authenticated() {
            self.render_auth_gate(ctx);
            self.toasts.borrow_mut().show(ctx);
            return;
        }

        let state = self.connection_state();
        self.render_top_bar(ctx, &state);

        egui::CentralPanel::default()
            .frame(Frame::new().fill(Color32::from_rgb(14, 17, 24)))
            .show(ctx, |ui| {
                ui.add_space(18.0);
                self.render_chat_surface(ui, &state);
            });

        self.render_composer(ctx, &state);
        self.toasts.borrow_mut().show(ctx);
    }
}
