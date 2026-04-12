use serde_json::{Value, json};
use uuid::Uuid;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use crate::{
    ConnectionState, MessageRole, SessionListEntry, classify_stream_message_action,
    next_selected_archive_id_after_submit, should_register_non_stream_fade,
};

use super::{
    app::ChatApp,
    protocol::{ServerFrame, send_method},
    session::ChatMessage,
    session::current_timestamp_ms,
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
    pub(in crate::web_chat) fn close_connection(&mut self) {
        if let Some(ws) = self.ws.borrow_mut().take() {
            let _ = ws.close();
        }
        *self.connection_state.borrow_mut() = ConnectionState::Disconnected;
        self.workspace_loaded = false;
    }

    pub(in crate::web_chat) fn reconnect_all_sessions(&mut self) {
        self.connect_workspace();
    }

    pub(in crate::web_chat) fn maybe_auto_connect_prefilled_token(&mut self) {
        if self.did_attempt_prefilled_token {
            return;
        }
        self.did_attempt_prefilled_token = true;
        if self.gateway_token.is_some() {
            self.connect_workspace();
        }
    }

    pub(in crate::web_chat) fn connect_workspace(&mut self) {
        if matches!(*self.connection_state.borrow(), ConnectionState::Connecting) {
            return;
        }

        self.sync_gateway_token_from_input_and_persist();
        let token = self.gateway_token.clone();
        self.close_connection();
        let url = match ws_chat_url(token.as_deref()) {
            Ok(url) => url,
            Err(err) => {
                *self.connection_state.borrow_mut() = ConnectionState::Error(err.clone());
                self.toasts.borrow_mut().error(err);
                return;
            }
        };

        *self.connection_state.borrow_mut() = ConnectionState::Connecting;
        let ws = match WebSocket::new(&url) {
            Ok(ws) => ws,
            Err(err) => {
                let message = format!("WebSocket::new: {err:?}");
                *self.connection_state.borrow_mut() = ConnectionState::Error(message.clone());
                self.toasts.borrow_mut().error(message);
                return;
            }
        };

        let pending_frames = self.pending_frames.clone();
        let ctx = self.ctx.clone();
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            let text = if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                String::from(text)
            } else if let Some(text) = event.data().as_string() {
                text
            } else {
                "[non-text message]".to_string()
            };
            let frame =
                serde_json::from_str::<ServerFrame>(&text).unwrap_or_else(|_| ServerFrame::Event {
                    event: "system.raw".to_string(),
                    payload: json!({ "text": text }),
                });
            pending_frames.borrow_mut().push(frame);
            ctx.request_repaint();
        }) as Box<dyn FnMut(MessageEvent)>);
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        let state_open = self.connection_state.clone();
        let ws_open = ws.clone();
        let ctx_open = self.ctx.clone();
        let onopen = Closure::wrap(Box::new(move |_event: JsValue| {
            *state_open.borrow_mut() = ConnectionState::Connected;
            let bootstrap_id = Uuid::new_v4().to_string();
            if let Err(err) = send_method(&ws_open, &bootstrap_id, "workspace.bootstrap", json!({}))
            {
                *state_open.borrow_mut() = ConnectionState::Error(err);
            }
            ctx_open.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let state_error = self.connection_state.clone();
        let ctx_error = self.ctx.clone();
        let onerror = Closure::wrap(Box::new(move |_event: JsValue| {
            *state_error.borrow_mut() =
                ConnectionState::Error("WebSocket error before open".to_string());
            ctx_error.request_repaint();
        }) as Box<dyn FnMut(JsValue)>);
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let state_close = self.connection_state.clone();
        let ws_cell = self.ws.clone();
        let ctx_close = self.ctx.clone();
        let onclose = Closure::wrap(Box::new(move |_event: CloseEvent| {
            ws_cell.borrow_mut().take();
            *state_close.borrow_mut() = ConnectionState::Disconnected;
            ctx_close.request_repaint();
        }) as Box<dyn FnMut(CloseEvent)>);
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        *self.ws.borrow_mut() = Some(ws);
    }

    pub(in crate::web_chat) fn create_session(&mut self) {
        if !self.is_workspace_ready() {
            return;
        }
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        let request_id = Uuid::new_v4().to_string();
        let _ = send_method(&ws, &request_id, "session.create", json!({}));
    }

    pub(in crate::web_chat) fn subscribe_session(&mut self, session_key: &str) {
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        let request_id = Uuid::new_v4().to_string();
        let _ = send_method(
            &ws,
            &request_id,
            "session.subscribe",
            json!({ "session_key": session_key }),
        );
    }

    pub(in crate::web_chat) fn rename_session(&mut self, session_key: &str, title: &str) {
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        let request_id = Uuid::new_v4().to_string();
        let _ = send_method(
            &ws,
            &request_id,
            "session.update",
            json!({
                "session_key": session_key,
                "title": title,
            }),
        );
    }

    pub(in crate::web_chat) fn delete_session(&mut self, session_key: &str) {
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        let request_id = Uuid::new_v4().to_string();
        let _ = send_method(
            &ws,
            &request_id,
            "session.delete",
            json!({
                "session_key": session_key,
            }),
        );
    }

    pub(in crate::web_chat) fn process_pending_frames(&mut self) {
        let frames = self
            .pending_frames
            .borrow_mut()
            .drain(..)
            .collect::<Vec<_>>();
        for frame in frames {
            self.process_frame(frame);
        }
    }

    pub(in crate::web_chat) fn process_frame(&mut self, frame: ServerFrame) {
        match frame {
            ServerFrame::Event { event, payload } => self.process_event_frame(&event, &payload),
            ServerFrame::Result { id: _, result } => self.process_result_frame(&result),
            ServerFrame::Error { id: _, error } => {
                let message = format!("{}: {}", error.code, error.message);
                *self.connection_state.borrow_mut() = ConnectionState::Error(message.clone());
                self.workspace_loaded = false;
                self.toasts.borrow_mut().error(message);
            }
        }
    }

    fn process_result_frame(&mut self, result: &Value) {
        if result.get("updated").and_then(Value::as_bool) == Some(true) {
            let Some(session_key) = result.get("session_key").and_then(Value::as_str) else {
                return;
            };
            let Some(title) = result.get("title").and_then(Value::as_str) else {
                return;
            };
            if let Some(index) = self.session_index(session_key) {
                self.sessions[index].title = title.to_string();
                self.persist_workspace_state();
            }
            return;
        }

        if result.get("deleted").and_then(Value::as_bool) == Some(true) {
            let Some(session_key) = result.get("session_key").and_then(Value::as_str) else {
                return;
            };
            self.remove_session(session_key);
            return;
        }

        if let Some(session_key) = result.get("session_key").and_then(Value::as_str)
            && result.get("title").is_none()
            && result.get("created_at_ms").is_none()
            && result.get("response").is_none()
        {
            if let Some(index) = self.session_index(session_key) {
                *self.sessions[index].buffers.history_loaded.borrow_mut() = true;
            }
            return;
        }

        if let Some(sessions_value) = result.get("sessions").cloned() {
            let entries =
                serde_json::from_value::<Vec<SessionListEntry>>(sessions_value).unwrap_or_default();
            let active_session_key = result
                .get("active_session_key")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            self.sync_sessions_from_workspace(entries, active_session_key);
            self.subscribe_open_sessions_needing_history();
            return;
        }

        if let (Some(session_key), Some(title), Some(created_at_ms)) = (
            result.get("session_key").and_then(Value::as_str),
            result.get("title").and_then(Value::as_str),
            result.get("created_at_ms").and_then(Value::as_i64),
        ) {
            let mut entries = self
                .sessions
                .iter()
                .map(|session| SessionListEntry {
                    session_key: session.session_key.clone(),
                    title: session.title.clone(),
                    created_at_ms: session.created_at_ms,
                })
                .collect::<Vec<_>>();
            entries.push(SessionListEntry {
                session_key: session_key.to_string(),
                title: title.to_string(),
                created_at_ms,
            });
            self.sync_sessions_from_workspace(entries, Some(session_key.to_string()));
            if let Some(index) = self.session_index(session_key) {
                self.sessions[index].open = true;
            }
            self.persist_workspace_state();
            self.subscribe_open_sessions_needing_history();
            return;
        }

        let Some(response) = result.get("response") else {
            return;
        };
        let streamed = result
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(session_key) = result.get("session_key").and_then(Value::as_str) else {
            return;
        };
        let Some(index) = self.session_index(session_key) else {
            return;
        };
        let content = response
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !streamed && !content.is_empty() {
            let message = ChatMessage::new(content, MessageRole::Assistant, current_timestamp_ms());
            if should_register_non_stream_fade(message.role, streamed, false, &message.text) {
                self.sessions[index].register_fade_in_message(&message);
            }
            self.sessions[index]
                .buffers
                .messages
                .borrow_mut()
                .push(message);
        }
        *self.sessions[index]
            .buffers
            .active_stream_request_id
            .borrow_mut() = None;
    }

    fn process_event_frame(&mut self, event: &str, payload: &Value) {
        match event {
            "session.connected" => {
                *self.connection_state.borrow_mut() = ConnectionState::Connected;
            }
            "session.message" => {
                let Some(session_key) = payload.get("session_key").and_then(Value::as_str) else {
                    return;
                };
                let Some(index) = self.session_index(session_key) else {
                    return;
                };
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
                let role = match payload.get("role").and_then(Value::as_str) {
                    Some("user") => MessageRole::User,
                    Some("system") => MessageRole::System,
                    _ => MessageRole::Assistant,
                };
                let timestamp_ms = payload
                    .get("timestamp_ms")
                    .and_then(Value::as_i64)
                    .unwrap_or_else(current_timestamp_ms);
                let history_event = payload
                    .get("history")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut history = self.sessions[index].buffers.messages.borrow_mut();
                if history_event || !matches!(role, MessageRole::Assistant) {
                    history.push(ChatMessage::new(content, role, timestamp_ms));
                    return;
                }
                let action = classify_stream_message_action(
                    history.last().map(|message| message.role),
                    self.sessions[index]
                        .buffers
                        .active_stream_request_id
                        .borrow()
                        .as_deref(),
                    request_id.as_deref(),
                    &content,
                );
                match action {
                    crate::StreamMessageAction::IgnoreEmpty => {}
                    crate::StreamMessageAction::ReplaceLastAssistant => {
                        if let Some(message) = history.last_mut() {
                            message.text = content;
                            message.timestamp_ms = current_timestamp_ms();
                        }
                    }
                    crate::StreamMessageAction::PushAssistant => {
                        history.push(ChatMessage::new(
                            content,
                            MessageRole::Assistant,
                            timestamp_ms,
                        ));
                        *self.sessions[index]
                            .buffers
                            .active_stream_request_id
                            .borrow_mut() = request_id;
                    }
                }
            }
            "session.stream.clear" | "session.stream.done" => {
                let Some(session_key) = payload.get("session_key").and_then(Value::as_str) else {
                    return;
                };
                let Some(index) = self.session_index(session_key) else {
                    return;
                };
                *self.sessions[index]
                    .buffers
                    .active_stream_request_id
                    .borrow_mut() = None;
            }
            "system.raw" => {
                let text = payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.toasts.borrow_mut().info(text);
            }
            _ => {}
        }
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

        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        let request_id = Uuid::new_v4().to_string();
        session.buffers.messages.borrow_mut().push(ChatMessage::new(
            text.clone(),
            MessageRole::User,
            current_timestamp_ms(),
        ));
        *session.buffers.active_stream_request_id.borrow_mut() =
            self.stream_enabled.then_some(request_id.clone());

        let archive_id = session.selected_archive_id.borrow().clone();
        let mut params = json!({
            "session_key": session_key,
            "chat_id": session_key,
            "input": text,
            "stream": self.stream_enabled,
        });

        if let Some(archive_id) = archive_id.as_ref() {
            params["archive_id"] = json!(archive_id);
        }

        let send_result = send_method(&ws, &request_id, "session.submit", params);
        *session.selected_archive_id.borrow_mut() =
            next_selected_archive_id_after_submit(archive_id.as_deref(), send_result.is_ok());

        if let Err(err) = send_result {
            *self.connection_state.borrow_mut() = ConnectionState::Error(err);
            return;
        }
        session.draft.clear();
    }
}
