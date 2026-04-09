use serde_json::{Value, json};
use uuid::Uuid;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use crate::{ConnectionState, MessageRole};

use super::{
    app::ChatApp,
    protocol::{ServerFrame, send_method},
    session::{ChatMessage, SessionBuffers, current_timestamp_ms},
};

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

impl ChatApp {
    pub(in crate::web_chat) fn close_buffers(buffers: &SessionBuffers) {
        if let Some(ws) = buffers.ws.borrow_mut().take() {
            *buffers.suppress_next_close_notice.borrow_mut() = true;
            let _ = ws.close();
        }
        *buffers.state.borrow_mut() = ConnectionState::Disconnected;
        *buffers.auth_verified.borrow_mut() = false;
    }

    pub(in crate::web_chat) fn reconnect_all_sessions(&mut self) {
        let keys = self
            .sessions
            .iter()
            .map(|session| session.session_key.clone())
            .collect::<Vec<_>>();
        for session_key in keys {
            self.try_connect_session(&session_key);
        }
    }

    pub(in crate::web_chat) fn maybe_auto_connect_prefilled_token(&mut self) {
        if self.did_attempt_prefilled_token {
            return;
        }
        self.did_attempt_prefilled_token = true;
        if self.gateway_token.is_some() {
            self.reconnect_all_sessions();
        }
    }

    pub(in crate::web_chat) fn try_connect_session(&mut self, session_key: &str) {
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
                            if let Some(message) = history.last_mut()
                                && message.role == MessageRole::Assistant
                            {
                                message.text = content;
                                message.timestamp_ms = current_timestamp_ms();
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
                    "session.stream.clear" | "session.stream.done" => {
                        *active_stream_request_id.borrow_mut() = None;
                    }
                    _ => {}
                },
                Ok(ServerFrame::Result { id, result }) => {
                    let _ = id;
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
                Ok(ServerFrame::Error { id, error }) => {
                    let _ = id;
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

    pub(in crate::web_chat) fn send_session_draft(&mut self, session_key: &str) {
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
}
