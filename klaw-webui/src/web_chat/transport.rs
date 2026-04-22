use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use crate::{
    ConnectionState, MessageRole, ProviderCatalog, WebArchiveAttachment, WorkspaceSessionEntry,
    build_websocket_submit_params, classify_stream_message_action,
    next_pending_attachments_after_submit, should_hide_heartbeat_operational_message,
    should_hide_heartbeat_silent_ack, should_register_non_stream_fade,
};

use super::{
    app::ChatApp,
    protocol::{ServerFrame, send_method},
    session::ChatMessage,
    session::HistoryRequestCursor,
    session::PendingHistoryScrollRestore,
    session::SessionWindow,
    session::current_timestamp_ms,
    session::window_anchor_for_slot,
    ui::sync_card_state_overrides,
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
        for session in &mut self.sessions {
            session.reset_connection_state();
        }
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
                ctx_open.request_repaint();
                return;
            }
            let providers_id = Uuid::new_v4().to_string();
            if let Err(err) = send_method(&ws_open, &providers_id, "provider.list", json!({})) {
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

    pub(in crate::web_chat) fn ensure_session_ready(&mut self, session_key: &str) {
        self.subscribe_session(session_key);
        self.load_history_page(session_key, None);
    }

    pub(in crate::web_chat) fn subscribe_session(&mut self, session_key: &str) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };
        if self.sessions[index].subscribed {
            return;
        }
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        let request_id = Uuid::new_v4().to_string();
        if let Err(err) = send_method(
            &ws,
            &request_id,
            "session.subscribe",
            json!({ "session_key": session_key }),
        ) {
            self.toasts.borrow_mut().error(err);
            return;
        }
        self.sessions[index].subscribed = true;
    }

    pub(in crate::web_chat) fn load_history_page(
        &mut self,
        session_key: &str,
        scroll_restore: Option<PendingHistoryScrollRestore>,
    ) {
        let Some(index) = self.session_index(session_key) else {
            return;
        };
        if *self.sessions[index].buffers.history_loading.borrow()
            || !self.sessions[index].history_has_more
        {
            return;
        }
        let request_cursor =
            history_request_cursor(self.sessions[index].oldest_loaded_message_id.clone());
        if !self.sessions[index].buffers.messages.borrow().is_empty()
            && self.sessions[index].last_requested_history_cursor.as_ref() == Some(&request_cursor)
        {
            self.sessions[index].history_has_more = false;
            return;
        }
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return;
        }
        let before_message_id = self.sessions[index].oldest_loaded_message_id.clone();
        let request_id = Uuid::new_v4().to_string();
        *self.sessions[index].buffers.history_loading.borrow_mut() = true;
        self.sessions[index].last_requested_history_cursor = Some(request_cursor);
        self.sessions[index].pending_history_scroll_restore = scroll_restore;
        if let Err(err) = send_method(
            &ws,
            &request_id,
            "session.history.load",
            json!({
                "session_key": session_key,
                "before_message_id": before_message_id,
                "limit": 30,
            }),
        ) {
            *self.sessions[index].buffers.history_loading.borrow_mut() = false;
            self.sessions[index].pending_history_scroll_restore = None;
            self.toasts.borrow_mut().error(err);
        }
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
                for session in &mut self.sessions {
                    *session.buffers.history_loading.borrow_mut() = false;
                }
                self.toasts.borrow_mut().error(message);
            }
        }
    }

    fn process_result_frame(&mut self, result: &Value) {
        if let Some(messages_value) = result.get("messages").cloned() {
            let Some(session_key) = result.get("session_key").and_then(Value::as_str) else {
                return;
            };
            let Some(index) = self.session_index(session_key) else {
                return;
            };
            let page_messages = serde_json::from_value::<Vec<HistoryPageMessage>>(messages_value)
                .unwrap_or_default();
            let mut history = self.sessions[index].buffers.messages.borrow_mut();
            prepend_history_page(&mut history, page_messages);
            let messages = history.clone();
            drop(history);
            sync_card_state_overrides(&messages, &mut self.sessions[index].card_state_overrides);
            *self.sessions[index].buffers.history_loading.borrow_mut() = false;
            let has_more = result
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let next_oldest_loaded_message_id = result
                .get("oldest_loaded_message_id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            self.sessions[index].history_has_more = history_cursor_can_advance(
                self.sessions[index].last_requested_history_cursor.as_ref(),
                next_oldest_loaded_message_id.as_deref(),
                has_more,
            );
            self.sessions[index].oldest_loaded_message_id = next_oldest_loaded_message_id;
            return;
        }

        if let Some(providers_value) = result.get("providers").cloned() {
            let providers =
                serde_json::from_value::<Vec<crate::ProviderCatalogEntry>>(providers_value)
                    .unwrap_or_default();
            let provider_catalog = ProviderCatalog {
                default_provider: result
                    .get("default_provider")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                providers,
            };
            self.apply_provider_catalog(provider_catalog);
            return;
        }

        if result.get("updated").and_then(Value::as_bool) == Some(true) {
            let Some(session_key) = result.get("session_key").and_then(Value::as_str) else {
                return;
            };
            let Some(title) = result.get("title").and_then(Value::as_str) else {
                return;
            };
            if let Some(index) = self.session_index(session_key) {
                self.sessions[index].title = title.to_string();
                self.sessions[index].sync_route_from_workspace(
                    result
                        .get("model_provider")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    result
                        .get("model")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    &self.provider_catalog,
                );
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

        if let Some(sessions_value) = result.get("sessions").cloned() {
            let entries = serde_json::from_value::<Vec<WorkspaceSessionEntry>>(sessions_value)
                .unwrap_or_default();
            let active_session_key = result
                .get("active_session_key")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            self.sync_sessions_from_workspace(entries, active_session_key);
            self.subscribe_sessions_needing_history();
            return;
        }

        if let (Some(session_key), Some(title), Some(created_at_ms)) = (
            result.get("session_key").and_then(Value::as_str),
            result.get("title").and_then(Value::as_str),
            result.get("created_at_ms").and_then(Value::as_i64),
        ) {
            let entry = WorkspaceSessionEntry {
                session_key: session_key.to_string(),
                title: title.to_string(),
                created_at_ms,
                model_provider: result
                    .get("model_provider")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                model: result
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            };
            let session = SessionWindow::new(entry, true, &self.provider_catalog);
            self.sessions.push(session);
            self.sessions
                .sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
            for (index, s) in self.sessions.iter_mut().enumerate() {
                s.window_anchor = window_anchor_for_slot(index as u32);
            }
            self.active_session_key = Some(session_key.to_string());
            self.persist_workspace_state();
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
            let response_metadata = response
                .get("metadata")
                .cloned()
                .and_then(|value| serde_json::from_value::<BTreeMap<String, Value>>(value).ok())
                .unwrap_or_default();
            if should_hide_heartbeat_silent_ack(&content, &response_metadata) {
                *self.sessions[index]
                    .buffers
                    .active_stream_request_id
                    .borrow_mut() = None;
                return;
            }
            let message = ChatMessage::new_with_metadata(
                content,
                MessageRole::Assistant,
                current_timestamp_ms(),
                result
                    .get("message_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                response_metadata,
            );
            if message_id_exists(
                &self.sessions[index].buffers.messages.borrow(),
                message.message_id.as_deref(),
            ) {
                *self.sessions[index]
                    .buffers
                    .active_stream_request_id
                    .borrow_mut() = None;
                return;
            }
            if should_register_non_stream_fade(message.role, streamed, false, &message.text) {
                self.sessions[index].register_fade_in_message(&message);
            }
            self.sessions[index]
                .buffers
                .messages
                .borrow_mut()
                .push(message);
            let messages = self.sessions[index].buffers.messages.borrow().clone();
            sync_card_state_overrides(&messages, &mut self.sessions[index].card_state_overrides);
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
                let response_metadata = payload
                    .get("response")
                    .and_then(|response| response.get("metadata"))
                    .cloned()
                    .and_then(|value| serde_json::from_value::<BTreeMap<String, Value>>(value).ok())
                    .unwrap_or_default();
                let mut history = self.sessions[index].buffers.messages.borrow_mut();
                if history_event || !matches!(role, MessageRole::Assistant) {
                    if message_id_exists(
                        &history,
                        payload.get("message_id").and_then(Value::as_str),
                    ) {
                        return;
                    }
                    if matches!(role, MessageRole::Assistant)
                        && should_hide_heartbeat_silent_ack(&content, &response_metadata)
                    {
                        return;
                    }
                    let message = ChatMessage::new_with_metadata(
                        content,
                        role,
                        timestamp_ms,
                        payload
                            .get("message_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        response_metadata,
                    );
                    history.push(message);
                    let messages = history.clone();
                    drop(history);
                    sync_card_state_overrides(
                        &messages,
                        &mut self.sessions[index].card_state_overrides,
                    );
                    return;
                }
                if should_hide_heartbeat_silent_ack(&content, &response_metadata) {
                    *self.sessions[index]
                        .buffers
                        .active_stream_request_id
                        .borrow_mut() = request_id;
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
                            message.metadata = response_metadata.clone();
                            message.card = crate::resolve_im_card(&message.text, &message.metadata);
                        }
                        let messages = history.clone();
                        drop(history);
                        sync_card_state_overrides(
                            &messages,
                            &mut self.sessions[index].card_state_overrides,
                        );
                    }
                    crate::StreamMessageAction::PushAssistant => {
                        history.push(ChatMessage::new_with_metadata(
                            content.clone(),
                            MessageRole::Assistant,
                            timestamp_ms,
                            payload
                                .get("message_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            response_metadata,
                        ));
                        let messages = history.clone();
                        *self.sessions[index]
                            .buffers
                            .active_stream_request_id
                            .borrow_mut() = request_id;
                        drop(history);
                        sync_card_state_overrides(
                            &messages,
                            &mut self.sessions[index].card_state_overrides,
                        );
                        self.notify_new_assistant_reply(session_key, &content);
                    }
                }
            }
            "session.subscribed" => {
                let Some(session_key) = payload.get("session_key").and_then(Value::as_str) else {
                    return;
                };
                let Some(index) = self.session_index(session_key) else {
                    return;
                };
                self.sessions[index].subscribed = true;
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
        let text = self.sessions[index].draft.trim().to_string();
        let attachments = self.sessions[index].pending_attachments.borrow().clone();
        if text.is_empty() && attachments.is_empty() {
            return;
        }

        let model_provider = self.sessions[index].selected_route.model_provider.clone();
        let model = self.sessions[index].selected_route.model.clone();
        let sent = self.send_session_input(
            session_key,
            &text,
            self.stream_enabled,
            &attachments,
            &model_provider,
            &model,
            None,
            true,
        );
        *self.sessions[index].pending_attachments.borrow_mut() =
            next_pending_attachments_after_submit(&attachments, sent);
        if sent {
            self.sessions[index].draft.clear();
        }
    }

    pub(in crate::web_chat) fn send_card_action(
        &mut self,
        session_key: &str,
        command: &str,
        metadata: BTreeMap<String, Value>,
    ) -> bool {
        let Some(index) = self.session_index(session_key) else {
            return false;
        };
        let model_provider = self.sessions[index].selected_route.model_provider.clone();
        let model = self.sessions[index].selected_route.model.clone();
        self.send_session_input(
            session_key,
            command,
            false,
            &[],
            &model_provider,
            &model,
            Some(&metadata),
            true,
        )
    }

    fn send_session_input(
        &mut self,
        session_key: &str,
        text: &str,
        stream: bool,
        attachments: &[WebArchiveAttachment],
        model_provider: &str,
        model: &str,
        metadata: Option<&BTreeMap<String, Value>>,
        local_echo: bool,
    ) -> bool {
        let Some(ws) = self.ws.borrow().as_ref().cloned() else {
            return false;
        };
        if ws.ready_state() != WebSocket::OPEN {
            return false;
        }
        let request_id = Uuid::new_v4().to_string();
        if let Some(index) = self.session_index(session_key)
            && local_echo
        {
            let mut local_metadata = metadata.cloned().unwrap_or_default();
            if !attachments.is_empty() {
                local_metadata.insert("attachments".to_string(), json!(attachments));
            }
            self.sessions[index].buffers.messages.borrow_mut().push(
                ChatMessage::new_with_metadata(
                    text.to_string(),
                    MessageRole::User,
                    current_timestamp_ms(),
                    None,
                    local_metadata,
                ),
            );
            *self.sessions[index]
                .buffers
                .active_stream_request_id
                .borrow_mut() = stream.then_some(request_id.clone());
        }
        let params = build_submit_params(
            session_key,
            text,
            stream,
            attachments,
            model_provider,
            model,
            metadata,
        );
        let send_result = send_method(&ws, &request_id, "session.submit", params);
        if let Err(err) = send_result {
            *self.connection_state.borrow_mut() = ConnectionState::Error(err);
            return false;
        }
        true
    }
}

fn build_submit_params(
    session_key: &str,
    input: &str,
    stream: bool,
    attachments: &[WebArchiveAttachment],
    model_provider: &str,
    model: &str,
    metadata: Option<&BTreeMap<String, Value>>,
) -> Value {
    build_websocket_submit_params(
        session_key,
        input,
        stream,
        attachments,
        model_provider,
        model,
        metadata,
    )
}

#[derive(serde::Deserialize)]
struct HistoryPageMessage {
    role: String,
    content: String,
    timestamp_ms: i64,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
    #[serde(default)]
    message_id: Option<String>,
}

fn prepend_history_page(history: &mut Vec<ChatMessage>, page_messages: Vec<HistoryPageMessage>) {
    let existing_message_ids = history
        .iter()
        .filter_map(|message| message.message_id.clone())
        .collect::<BTreeSet<_>>();
    let mut prepended = page_messages
        .into_iter()
        .filter(|message| {
            !should_hide_heartbeat_operational_message(
                &message.role,
                &message.content,
                &message.metadata,
            )
        })
        .filter(|message| {
            message
                .message_id
                .as_ref()
                .is_none_or(|message_id| !existing_message_ids.contains(message_id))
        })
        .map(|message| {
            ChatMessage::new_with_metadata(
                message.content,
                parse_message_role(&message.role),
                message.timestamp_ms,
                message.message_id,
                message.metadata,
            )
        })
        .collect::<Vec<_>>();
    prepended.append(history);
    *history = prepended;
}

fn parse_message_role(role: &str) -> MessageRole {
    match role {
        "user" => MessageRole::User,
        "system" => MessageRole::System,
        _ => MessageRole::Assistant,
    }
}

fn message_id_exists(messages: &[ChatMessage], message_id: Option<&str>) -> bool {
    let Some(message_id) = message_id else {
        return false;
    };
    messages
        .iter()
        .any(|message| message.message_id.as_deref() == Some(message_id))
}

fn history_request_cursor(oldest_loaded_message_id: Option<String>) -> HistoryRequestCursor {
    match oldest_loaded_message_id {
        Some(message_id) => HistoryRequestCursor::BeforeMessage(message_id),
        None => HistoryRequestCursor::InitialPage,
    }
}

fn history_cursor_can_advance(
    last_requested_cursor: Option<&HistoryRequestCursor>,
    next_oldest_loaded_message_id: Option<&str>,
    has_more: bool,
) -> bool {
    if !has_more {
        return false;
    }

    match (last_requested_cursor, next_oldest_loaded_message_id) {
        (_, None) => false,
        (Some(HistoryRequestCursor::InitialPage), Some(_)) => true,
        (Some(HistoryRequestCursor::BeforeMessage(previous)), Some(next)) => previous != next,
        (None, Some(_)) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HistoryPageMessage, build_submit_params, history_cursor_can_advance,
        history_request_cursor, prepend_history_page,
    };
    use crate::{MessageRole, WebArchiveAttachment};
    use crate::{should_hide_heartbeat_operational_message, should_hide_heartbeat_silent_ack};
    use std::collections::BTreeMap;

    #[test]
    fn submit_params_include_model_route_and_archive() {
        let params = build_submit_params(
            "websocket:test",
            "hello",
            true,
            &[WebArchiveAttachment {
                archive_id: "archive-1".to_string(),
                filename: Some("report.pdf".to_string()),
                mime_type: Some("application/pdf".to_string()),
                size_bytes: 42,
            }],
            "anthropic",
            "claude-sonnet-4-5",
            None,
        );

        assert_eq!(
            params
                .get("session_key")
                .and_then(serde_json::Value::as_str),
            Some("websocket:test")
        );
        assert_eq!(
            params.get("input").and_then(serde_json::Value::as_str),
            Some("hello")
        );
        assert_eq!(
            params.get("stream").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            params
                .get("model_provider")
                .and_then(serde_json::Value::as_str),
            Some("anthropic")
        );
        assert_eq!(
            params.get("model").and_then(serde_json::Value::as_str),
            Some("claude-sonnet-4-5")
        );
        assert_eq!(
            params.get("archive_id").and_then(serde_json::Value::as_str),
            Some("archive-1")
        );
        assert_eq!(params["attachments"][0]["archive_id"], "archive-1");
    }

    #[test]
    fn submit_params_omit_archive_when_unset() {
        let params = build_submit_params(
            "websocket:test",
            "hello",
            false,
            &[],
            "openai",
            "gpt-4.1-mini",
            None,
        );

        assert!(params.get("archive_id").is_none());
        assert_eq!(
            params
                .get("model_provider")
                .and_then(serde_json::Value::as_str),
            Some("openai")
        );
        assert_eq!(
            params.get("model").and_then(serde_json::Value::as_str),
            Some("gpt-4.1-mini")
        );
    }

    #[test]
    fn prepend_history_page_deduplicates_existing_message_ids() {
        let mut history = vec![crate::web_chat::session::ChatMessage::new_with_metadata(
            "current".to_string(),
            MessageRole::Assistant,
            2,
            Some("msg-2".to_string()),
            BTreeMap::new(),
        )];

        prepend_history_page(
            &mut history,
            vec![
                HistoryPageMessage {
                    role: "user".to_string(),
                    content: "older".to_string(),
                    timestamp_ms: 1,
                    metadata: BTreeMap::new(),
                    message_id: Some("msg-1".to_string()),
                },
                HistoryPageMessage {
                    role: "assistant".to_string(),
                    content: "duplicate".to_string(),
                    timestamp_ms: 2,
                    metadata: BTreeMap::new(),
                    message_id: Some("msg-2".to_string()),
                },
            ],
        );

        let summary = history
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(summary, vec!["older", "current"]);
    }

    #[test]
    fn repeated_history_cursor_is_not_treated_as_progress() {
        let cursor = history_request_cursor(Some("msg-10".to_string()));
        assert!(!history_cursor_can_advance(
            Some(&cursor),
            Some("msg-10"),
            true,
        ));
    }

    #[test]
    fn history_pagination_stops_when_server_has_more_but_no_cursor() {
        let cursor = history_request_cursor(None);
        assert!(!history_cursor_can_advance(Some(&cursor), None, true));
    }

    #[test]
    fn prepend_history_page_drops_heartbeat_silent_ack_messages() {
        let mut history = Vec::new();

        prepend_history_page(
            &mut history,
            vec![HistoryPageMessage {
                role: "assistant".to_string(),
                content: "HEARTBEAT_OK".to_string(),
                timestamp_ms: 1,
                metadata: BTreeMap::from([
                    ("trigger.kind".to_string(), json!("heartbeat")),
                    (
                        "heartbeat.silent_ack_token".to_string(),
                        json!("HEARTBEAT_OK"),
                    ),
                ]),
                message_id: Some("msg-hb".to_string()),
            }],
        );

        assert!(history.is_empty());
    }

    #[test]
    fn heartbeat_silent_ack_detection_requires_matching_metadata() {
        assert!(should_hide_heartbeat_silent_ack(
            " HEARTBEAT_OK ",
            &BTreeMap::from([
                ("trigger.kind".to_string(), json!("heartbeat")),
                (
                    "heartbeat.silent_ack_token".to_string(),
                    json!("HEARTBEAT_OK"),
                ),
            ])
        ));
        assert!(!should_hide_heartbeat_silent_ack(
            "HEARTBEAT_OK",
            &BTreeMap::new()
        ));
    }

    #[test]
    fn prepend_history_page_drops_heartbeat_operational_prompts() {
        let mut history = Vec::new();

        prepend_history_page(
            &mut history,
            vec![HistoryPageMessage {
                role: "user".to_string(),
                content: "heartbeat prompt".to_string(),
                timestamp_ms: 1,
                metadata: BTreeMap::from([("trigger.kind".to_string(), json!("heartbeat"))]),
                message_id: Some("msg-hb-user".to_string()),
            }],
        );

        assert!(history.is_empty());
    }

    #[test]
    fn heartbeat_operational_message_filter_preserves_visible_assistant_output() {
        assert!(!should_hide_heartbeat_operational_message(
            "assistant",
            "Please follow up with the user.",
            &BTreeMap::from([("trigger.kind".to_string(), json!("heartbeat"))]),
        ));
    }
}
