use crate::{
    RuntimeBundle, submit_channel_request, submit_channel_request_streaming_with_callback,
};
use async_trait::async_trait;
use klaw_channel::{ChannelResponse, websocket::WebsocketSubmitEnvelope};
use klaw_config::{AppConfig, WebsocketConfig};
use klaw_core::{MediaReference, MediaSourceKind};
use klaw_gateway::{
    GatewayProtocolMethod, GatewayProviderCatalog, GatewayProviderEntry, GatewayRpcMessage,
    GatewaySessionHistoryMessage, GatewaySessionHistoryPage, GatewayThreadItem,
    GatewayThreadItemStatus, GatewayThreadItemType, GatewayTurnStatus,
    GatewayWebsocketAttachmentRef, GatewayWebsocketHandler, GatewayWebsocketHandlerError,
    GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest, GatewayWorkspaceBootstrap,
    GatewayWorkspaceSession, META_WEBSOCKET_V1_THREAD_ID, META_WEBSOCKET_V1_TURN_ID, OutboundEvent,
};
use klaw_heartbeat::{HeartbeatManager, should_exclude_chat_record_from_context};
use klaw_session::{SessionHistoryPage, SessionListQuery, SessionManager};
use klaw_storage::{ChatRecord, StorageError};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::mpsc;
use uuid::Uuid;

pub fn build_gateway_websocket_handler(
    runtime: Arc<RuntimeBundle>,
    config: &AppConfig,
) -> Arc<dyn GatewayWebsocketHandler> {
    Arc::new(RuntimeWebsocketHandler::from_app_config(runtime, config))
}

struct RuntimeWebsocketHandler {
    runtime: Arc<RuntimeBundle>,
    configs: BTreeMap<String, WebsocketConfig>,
}

impl RuntimeWebsocketHandler {
    fn from_app_config(runtime: Arc<RuntimeBundle>, config: &AppConfig) -> Self {
        let configs = config
            .channels
            .websocket
            .iter()
            .cloned()
            .map(|entry| (entry.id.clone(), entry))
            .collect();
        Self { runtime, configs }
    }

    fn resolve_config(
        &self,
        channel_id: &str,
    ) -> Result<WebsocketConfig, GatewayWebsocketHandlerError> {
        if let Some(config) = self.configs.get(channel_id) {
            return Ok(config.clone());
        }
        if self.configs.is_empty() && channel_id == "default" {
            return Ok(WebsocketConfig::default());
        }
        Err(GatewayWebsocketHandlerError::invalid_request(format!(
            "unknown websocket channel_id '{channel_id}'"
        )))
    }

    async fn load_web_workspace(
        &self,
    ) -> Result<GatewayWorkspaceBootstrap, GatewayWebsocketHandlerError> {
        let sessions =
            klaw_session::SqliteSessionManager::from_store(self.runtime.session_store.clone())
                .list_sessions(SessionListQuery {
                    limit: None,
                    channel: Some("websocket".to_string()),
                    session_key_prefix: Some("websocket:".to_string()),
                    sort_order: klaw_storage::SessionSortOrder::CreatedAtDesc,
                    ..SessionListQuery::default()
                })
                .await
                .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        Ok(build_web_workspace_bootstrap(sessions))
    }
}

fn resolved_history_session_key(session: &klaw_storage::SessionIndex) -> &str {
    session
        .active_session_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .unwrap_or(&session.session_key)
}

fn parse_chat_record_metadata(raw: Option<&str>) -> BTreeMap<String, Value> {
    raw.and_then(|value| serde_json::from_str::<BTreeMap<String, Value>>(value).ok())
        .unwrap_or_default()
}

fn history_record_is_visible(record: &ChatRecord) -> bool {
    !should_exclude_chat_record_from_context(record)
}

fn history_message_from_record(record: ChatRecord) -> GatewaySessionHistoryMessage {
    GatewaySessionHistoryMessage {
        role: record.role,
        content: record.content,
        timestamp_ms: record.ts_ms,
        metadata: parse_chat_record_metadata(record.metadata_json.as_deref()),
        message_id: record.message_id,
    }
}

fn prepend_visible_history_records(
    visible_records: &mut Vec<ChatRecord>,
    page_records: Vec<ChatRecord>,
) {
    let mut page_visible = page_records
        .into_iter()
        .filter(history_record_is_visible)
        .collect::<Vec<_>>();
    page_visible.append(visible_records);
    *visible_records = page_visible;
}

fn finalize_visible_history_page(
    mut visible_records: Vec<ChatRecord>,
    limit: usize,
    raw_has_more: bool,
) -> GatewaySessionHistoryPage {
    let mut has_more = raw_has_more;
    if limit > 0 && visible_records.len() > limit {
        let keep_from = visible_records.len() - limit;
        visible_records = visible_records.split_off(keep_from);
        has_more = true;
    }
    let messages = visible_records
        .into_iter()
        .map(history_message_from_record)
        .collect::<Vec<_>>();
    let oldest_loaded_message_id = messages
        .first()
        .and_then(|message| message.message_id.clone());
    GatewaySessionHistoryPage {
        messages,
        has_more,
        oldest_loaded_message_id,
    }
}

fn build_websocket_media_references(
    attachments: &[GatewayWebsocketAttachmentRef],
) -> Vec<MediaReference> {
    attachments
        .iter()
        .map(|attachment| {
            let mut metadata = BTreeMap::new();
            metadata.insert(
                "archive.id".to_string(),
                Value::String(attachment.archive_id.clone()),
            );
            if let Some(mime_type) = attachment.mime_type.clone() {
                metadata.insert("archive.mime_type".to_string(), Value::String(mime_type));
            }
            if attachment.size_bytes > 0 {
                metadata.insert(
                    "archive.size_bytes".to_string(),
                    Value::from(attachment.size_bytes),
                );
            }
            MediaReference {
                source_kind: MediaSourceKind::UserUpload,
                filename: attachment.filename.clone(),
                mime_type: attachment.mime_type.clone(),
                remote_url: None,
                bytes: None,
                message_id: None,
                metadata,
            }
        })
        .collect()
}

#[async_trait]
impl GatewayWebsocketHandler for RuntimeWebsocketHandler {
    async fn bootstrap(&self) -> Result<GatewayWorkspaceBootstrap, GatewayWebsocketHandlerError> {
        self.load_web_workspace().await
    }

    async fn list_providers(&self) -> Result<GatewayProviderCatalog, GatewayWebsocketHandlerError> {
        let snapshot = self.runtime.runtime.provider_runtime_snapshot();
        let providers = snapshot
            .provider_default_models
            .into_iter()
            .map(|(id, default_model)| GatewayProviderEntry { id, default_model })
            .collect::<Vec<_>>();
        Ok(GatewayProviderCatalog {
            default_provider: snapshot.default_provider_id,
            providers,
        })
    }

    async fn create_session(
        &self,
    ) -> Result<GatewayWorkspaceSession, GatewayWebsocketHandlerError> {
        let manager =
            klaw_session::SqliteSessionManager::from_store(self.runtime.session_store.clone());
        let session_key = format!("websocket:{}", Uuid::new_v4());
        manager
            .touch_session(&session_key, &session_key, "websocket")
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        HeartbeatManager::new(Arc::new(self.runtime.session_store.clone()))
            .sync_job_to_session(&session_key, "websocket", &session_key)
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        let workspace = self.load_web_workspace().await?;
        workspace
            .sessions
            .into_iter()
            .find(|session| session.session_key == session_key)
            .ok_or_else(|| {
                GatewayWebsocketHandlerError::internal(format!(
                    "created session `{session_key}` missing from workspace bootstrap"
                ))
            })
    }

    async fn update_session(
        &self,
        session_key: &str,
        title: String,
    ) -> Result<GatewayWorkspaceSession, GatewayWebsocketHandlerError> {
        let manager =
            klaw_session::SqliteSessionManager::from_store(self.runtime.session_store.clone());
        let session = manager
            .set_session_title(session_key, Some(&title))
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        Ok(GatewayWorkspaceSession {
            session_key: session.session_key,
            title,
            created_at_ms: session.created_at_ms,
            model_provider: session.model_provider,
            model: session.model,
        })
    }

    async fn delete_session(
        &self,
        session_key: &str,
    ) -> Result<bool, GatewayWebsocketHandlerError> {
        let manager =
            klaw_session::SqliteSessionManager::from_store(self.runtime.session_store.clone());
        manager
            .delete_session(session_key)
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))
    }

    async fn load_session_history(
        &self,
        session_key: &str,
        before_message_id: Option<&str>,
        limit: usize,
    ) -> Result<GatewaySessionHistoryPage, GatewayWebsocketHandlerError> {
        let manager =
            klaw_session::SqliteSessionManager::from_store(self.runtime.session_store.clone());
        let session = manager
            .get_session(session_key)
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        let target_limit = limit.max(1);
        let mut cursor = before_message_id.map(ToOwned::to_owned);
        let mut visible_records = Vec::new();
        let raw_has_more = loop {
            let page = manager
                .read_chat_records_page(
                    resolved_history_session_key(&session),
                    cursor.as_deref(),
                    target_limit,
                )
                .await
                .map_err(|err| match err {
                    klaw_session::SessionError::Storage(StorageError::InvalidHistoryCursor(_)) => {
                        GatewayWebsocketHandlerError::invalid_request(err.to_string())
                    }
                    _ => GatewayWebsocketHandlerError::internal(err.to_string()),
                })?;

            let SessionHistoryPage {
                records,
                has_more,
                oldest_message_id,
            } = page;

            prepend_visible_history_records(&mut visible_records, records);

            if visible_records.len() > target_limit || !has_more {
                break has_more;
            }

            let Some(next_cursor) = oldest_message_id else {
                break false;
            };
            cursor = Some(next_cursor);
        };

        Ok(finalize_visible_history_page(
            visible_records,
            target_limit,
            raw_has_more,
        ))
    }

    async fn submit(
        &self,
        request: GatewayWebsocketSubmitRequest,
        frame_tx: mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    ) -> Result<(), GatewayWebsocketHandlerError> {
        let config = self.resolve_config(&request.channel_id)?;
        let stream_output = request.stream.unwrap_or(config.stream_output);
        let request_id = request.request_id.clone();
        let session_key = request.session_key.clone();
        let v1_context = GatewayV1StreamContext::from_metadata(
            &request.metadata,
            request.session_key.clone(),
            request.request_id.clone(),
        );
        let channel_request = WebsocketSubmitEnvelope {
            channel_id: request.channel_id.clone(),
            connection_id: request.connection_id,
            request_id: request.request_id.clone(),
            session_key: request.session_key.clone(),
            chat_id: request.chat_id.clone(),
            input: request.input,
            media_references: build_websocket_media_references(&request.attachments),
            metadata: request.metadata,
        }
        .into_channel_request();

        if stream_output {
            let mut stream_state = GatewayStreamState::new(v1_context.clone());
            let response = submit_channel_request_streaming_with_callback(
                self.runtime.as_ref(),
                channel_request,
                |event| {
                    stream_state.push_event(
                        &frame_tx,
                        &request_id,
                        &session_key,
                        config.show_reasoning,
                        event,
                    )
                },
            )
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
            if let Some(context) = v1_context.as_ref() {
                send_v1_item_completed(
                    &frame_tx,
                    context,
                    response.as_ref(),
                    config.show_reasoning,
                )?;
                send_v1_turn_finished(
                    &frame_tx,
                    context,
                    GatewayProtocolMethod::TurnCompleted,
                    GatewayTurnStatus::Completed,
                    response
                        .as_ref()
                        .map(|response| serialize_response(response, config.show_reasoning)),
                )?;
                return Ok(());
            }
            send_frame(
                &frame_tx,
                GatewayWebsocketServerFrame::Event {
                    event: OutboundEvent::SessionStreamDone,
                    payload: json!({
                        "request_id": request_id.clone(),
                        "response": response.as_ref().map(|response| serialize_response(response, config.show_reasoning)),
                        "session_key": session_key.clone(),
                    }),
                },
            )?;
            send_frame(
                &frame_tx,
                GatewayWebsocketServerFrame::Result {
                    id: request_id,
                    result: json!({
                        "response": response.as_ref().map(|response| serialize_response(response, config.show_reasoning)),
                        "session_key": session_key,
                        "stream": true,
                    }),
                },
            )?;
            return Ok(());
        }

        let response = submit_channel_request(self.runtime.as_ref(), channel_request)
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        if let Some(context) = v1_context.as_ref() {
            send_v1_item_completed(&frame_tx, context, response.as_ref(), config.show_reasoning)?;
            send_v1_turn_finished(
                &frame_tx,
                context,
                GatewayProtocolMethod::TurnCompleted,
                GatewayTurnStatus::Completed,
                response
                    .as_ref()
                    .map(|response| serialize_response(response, config.show_reasoning)),
            )?;
            return Ok(());
        }
        send_frame(
            &frame_tx,
            GatewayWebsocketServerFrame::Result {
                id: request_id,
                result: json!({
                    "response": response.as_ref().map(|response| serialize_response(response, config.show_reasoning)),
                    "session_key": session_key,
                    "stream": false,
                }),
            },
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GatewayV1StreamContext {
    session_id: String,
    thread_id: String,
    turn_id: String,
    request_id: String,
    agent_message_item_id: String,
}

impl GatewayV1StreamContext {
    fn from_metadata(
        metadata: &BTreeMap<String, Value>,
        session_id: String,
        request_id: String,
    ) -> Option<Self> {
        let thread_id = metadata
            .get(META_WEBSOCKET_V1_THREAD_ID)
            .and_then(Value::as_str)?
            .to_string();
        let turn_id = metadata
            .get(META_WEBSOCKET_V1_TURN_ID)
            .and_then(Value::as_str)?
            .to_string();
        Some(Self {
            session_id,
            thread_id,
            turn_id: turn_id.clone(),
            request_id: request_id.clone(),
            agent_message_item_id: format!("item_agent_{turn_id}"),
        })
    }
}

struct GatewayStreamState {
    last_snapshot: Option<String>,
    v1_context: Option<GatewayV1StreamContext>,
    v1_agent_message_started: bool,
}

impl GatewayStreamState {
    fn new(v1_context: Option<GatewayV1StreamContext>) -> Self {
        Self {
            last_snapshot: None,
            v1_context,
            v1_agent_message_started: false,
        }
    }
}

impl GatewayStreamState {
    fn push_event(
        &mut self,
        frame_tx: &mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
        request_id: &str,
        session_key: &str,
        show_reasoning: bool,
        event: klaw_channel::ChannelStreamEvent,
    ) -> klaw_channel::ChannelResult<()> {
        match event {
            klaw_channel::ChannelStreamEvent::Snapshot(response) => {
                let delta = match self.last_snapshot.as_deref() {
                    Some(previous) if response.content.starts_with(previous) => {
                        response.content[previous.len()..].to_string()
                    }
                    _ => response.content.clone(),
                };
                self.last_snapshot = Some(response.content.clone());
                if let Some(context) = self.v1_context.clone() {
                    if !self.v1_agent_message_started {
                        send_v1_item_started(frame_tx, &context, &response, show_reasoning)
                            .map_err(|err| std::io::Error::other(err.message.clone()))?;
                        self.v1_agent_message_started = true;
                    }
                    if !delta.is_empty() {
                        send_v1_agent_delta(frame_tx, &context, &delta)
                            .map_err(|err| std::io::Error::other(err.message.clone()))?;
                    }
                    return Ok(());
                }
                send_frame(
                    frame_tx,
                    GatewayWebsocketServerFrame::Event {
                        event: OutboundEvent::SessionMessage,
                        payload: json!({
                            "request_id": request_id,
                            "session_key": session_key,
                            "response": serialize_response(&response, show_reasoning),
                        }),
                    },
                )
                .map_err(|err| std::io::Error::other(err.message))?;
                if !delta.is_empty() {
                    send_frame(
                        frame_tx,
                        GatewayWebsocketServerFrame::Event {
                            event: OutboundEvent::SessionStreamDelta,
                            payload: json!({
                                "request_id": request_id,
                                "session_key": session_key,
                                "delta": delta,
                            }),
                        },
                    )
                    .map_err(|err| std::io::Error::other(err.message))?;
                }
            }
            klaw_channel::ChannelStreamEvent::Clear => {
                self.last_snapshot = None;
                if self.v1_context.is_some() {
                    return Ok(());
                }
                send_frame(
                    frame_tx,
                    GatewayWebsocketServerFrame::Event {
                        event: OutboundEvent::SessionStreamClear,
                        payload: json!({
                            "request_id": request_id,
                            "session_key": session_key,
                        }),
                    },
                )
                .map_err(|err| std::io::Error::other(err.message))?;
            }
        }
        Ok(())
    }
}

fn send_v1_item_started(
    frame_tx: &mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    context: &GatewayV1StreamContext,
    response: &ChannelResponse,
    show_reasoning: bool,
) -> Result<(), GatewayWebsocketHandlerError> {
    let item = GatewayThreadItem {
        item_id: context.agent_message_item_id.clone(),
        turn_id: context.turn_id.clone(),
        item_type: GatewayThreadItemType::AgentMessage,
        status: GatewayThreadItemStatus::InProgress,
        payload: json!({
            "session_id": context.session_id,
            "thread_id": context.thread_id,
            "response": serialize_response(response, show_reasoning),
        }),
    };
    send_frame(
        frame_tx,
        GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
            GatewayProtocolMethod::ItemStarted,
            json!({
                "session_id": context.session_id,
                "thread_id": context.thread_id,
                "turn_id": context.turn_id,
                "item": item,
            }),
        )),
    )
}

fn send_v1_agent_delta(
    frame_tx: &mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    context: &GatewayV1StreamContext,
    delta: &str,
) -> Result<(), GatewayWebsocketHandlerError> {
    send_frame(
        frame_tx,
        GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
            GatewayProtocolMethod::ItemAgentMessageDelta,
            json!({
                "session_id": context.session_id,
                "thread_id": context.thread_id,
                "turn_id": context.turn_id,
                "item_id": context.agent_message_item_id,
                "delta": delta,
            }),
        )),
    )
}

fn send_v1_item_completed(
    frame_tx: &mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    context: &GatewayV1StreamContext,
    response: Option<&ChannelResponse>,
    show_reasoning: bool,
) -> Result<(), GatewayWebsocketHandlerError> {
    let item = GatewayThreadItem {
        item_id: context.agent_message_item_id.clone(),
        turn_id: context.turn_id.clone(),
        item_type: GatewayThreadItemType::AgentMessage,
        status: GatewayThreadItemStatus::Completed,
        payload: json!({
            "session_id": context.session_id,
            "thread_id": context.thread_id,
            "response": response.map(|response| serialize_response(response, show_reasoning)),
        }),
    };
    send_frame(
        frame_tx,
        GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
            GatewayProtocolMethod::ItemCompleted,
            json!({
                "session_id": context.session_id,
                "thread_id": context.thread_id,
                "turn_id": context.turn_id,
                "item": item,
            }),
        )),
    )
}

fn send_v1_turn_finished(
    frame_tx: &mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    context: &GatewayV1StreamContext,
    method: GatewayProtocolMethod,
    status: GatewayTurnStatus,
    response: Option<Value>,
) -> Result<(), GatewayWebsocketHandlerError> {
    send_frame(
        frame_tx,
        GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
            method,
            json!({
                "session_id": context.session_id,
                "thread_id": context.thread_id,
                "turn_id": context.turn_id,
                "request_id": context.request_id,
                "status": status,
                "response": response,
            }),
        )),
    )
}

fn send_frame(
    frame_tx: &mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    frame: GatewayWebsocketServerFrame,
) -> Result<(), GatewayWebsocketHandlerError> {
    frame_tx.send(frame).map_err(|_| {
        GatewayWebsocketHandlerError::internal("websocket connection closed before frame delivery")
    })
}

#[cfg(test)]
fn stream_events_to_frames(
    request_id: &str,
    session_key: &str,
    show_reasoning: bool,
    events: &[klaw_channel::ChannelStreamEvent],
) -> Vec<GatewayWebsocketServerFrame> {
    stream_events_to_frames_with_identity(
        request_id,
        session_key,
        None,
        None,
        show_reasoning,
        events,
    )
}

#[cfg(test)]
fn stream_events_to_frames_with_identity(
    request_id: &str,
    session_key: &str,
    thread_id: Option<&str>,
    turn_id: Option<&str>,
    show_reasoning: bool,
    events: &[klaw_channel::ChannelStreamEvent],
) -> Vec<GatewayWebsocketServerFrame> {
    let v1_context = thread_id
        .zip(turn_id)
        .map(|(thread_id, turn_id)| GatewayV1StreamContext {
            session_id: session_key.to_string(),
            thread_id: thread_id.to_string(),
            turn_id: turn_id.to_string(),
            request_id: request_id.to_string(),
            agent_message_item_id: format!("item_agent_{turn_id}"),
        });
    let (frame_tx, mut frame_rx) = mpsc::unbounded_channel();
    let mut stream_state = GatewayStreamState::new(v1_context);
    for event in events {
        stream_state
            .push_event(
                &frame_tx,
                request_id,
                session_key,
                show_reasoning,
                event.clone(),
            )
            .expect("test stream event should convert to frames");
    }
    drop(frame_tx);
    let mut frames = Vec::new();
    while let Ok(frame) = frame_rx.try_recv() {
        frames.push(frame);
    }
    frames
}

fn serialize_response(response: &ChannelResponse, show_reasoning: bool) -> Value {
    json!({
        "content": response.content,
        "reasoning": show_reasoning.then(|| response.reasoning.clone()).flatten(),
        "metadata": response.metadata,
        "attachments": response.attachments,
    })
}

fn build_web_workspace_bootstrap(
    sessions: Vec<klaw_storage::SessionIndex>,
) -> GatewayWorkspaceBootstrap {
    let sessions = sessions
        .into_iter()
        .map(|session| GatewayWorkspaceSession {
            session_key: session.session_key.clone(),
            title: session
                .title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| {
                    format!(
                        "Agent {}",
                        &session.session_key.trim_start_matches("websocket:")[..8]
                    )
                }),
            created_at_ms: session.created_at_ms,
            model_provider: session.model_provider,
            model: session.model,
        })
        .collect::<Vec<_>>();
    let active_session_key = sessions.first().map(|session| session.session_key.clone());
    GatewayWorkspaceBootstrap {
        sessions,
        active_session_key,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_web_workspace_bootstrap, finalize_visible_history_page,
        prepend_visible_history_records, resolved_history_session_key, stream_events_to_frames,
        stream_events_to_frames_with_identity,
    };
    use klaw_channel::{ChannelResponse, ChannelStreamEvent};
    use klaw_gateway::OutboundEvent;
    use klaw_session::ChatRecord;
    use klaw_storage::SessionIndex;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn stream_events_emit_delta_then_done_snapshot_updates() {
        let frames = stream_events_to_frames(
            "req-1",
            "websocket:test",
            false,
            &[
                ChannelStreamEvent::Snapshot(ChannelResponse {
                    content: "Hel".to_string(),
                    reasoning: Some("hidden".to_string()),
                    metadata: BTreeMap::new(),
                    attachments: Vec::new(),
                }),
                ChannelStreamEvent::Snapshot(ChannelResponse {
                    content: "Hello".to_string(),
                    reasoning: Some("hidden".to_string()),
                    metadata: BTreeMap::new(),
                    attachments: Vec::new(),
                }),
            ],
        );

        assert_eq!(frames.len(), 4);
        match &frames[0] {
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, payload } => {
                assert_eq!(*event, OutboundEvent::SessionMessage);
                assert_eq!(
                    payload
                        .get("response")
                        .and_then(|response| response.get("content"))
                        .and_then(serde_json::Value::as_str),
                    Some("Hel")
                );
                assert_eq!(
                    payload
                        .get("response")
                        .and_then(|response| response.get("reasoning")),
                    Some(&serde_json::Value::Null)
                );
            }
            other => panic!("unexpected frame: {other:?}"),
        }
        match &frames[1] {
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, payload } => {
                assert_eq!(*event, OutboundEvent::SessionStreamDelta);
                assert_eq!(
                    payload.get("delta").and_then(serde_json::Value::as_str),
                    Some("Hel")
                );
            }
            other => panic!("unexpected frame: {other:?}"),
        }
        match &frames[3] {
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, payload } => {
                assert_eq!(*event, OutboundEvent::SessionStreamDelta);
                assert_eq!(
                    payload.get("delta").and_then(serde_json::Value::as_str),
                    Some("lo")
                );
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    #[test]
    fn stream_events_emit_v1_item_lifecycle_when_turn_identity_is_available() {
        let frames = stream_events_to_frames_with_identity(
            "req-v1",
            "websocket:test",
            Some("thr_v1"),
            Some("turn_v1"),
            false,
            &[ChannelStreamEvent::Snapshot(ChannelResponse {
                content: "Hello".to_string(),
                reasoning: Some("hidden".to_string()),
                metadata: BTreeMap::new(),
                attachments: Vec::new(),
            })],
        );

        assert!(frames.iter().any(|frame| match frame {
            klaw_gateway::GatewayWebsocketServerFrame::Protocol(
                klaw_gateway::GatewayRpcMessage::Notification { method, params },
            ) if *method == klaw_gateway::GatewayProtocolMethod::ItemStarted => {
                params
                    .pointer("/item/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("agentMessage")
                    && params
                        .pointer("/item/turn_id")
                        .and_then(serde_json::Value::as_str)
                        == Some("turn_v1")
            }
            _ => false,
        }));
        assert!(frames.iter().any(|frame| match frame {
            klaw_gateway::GatewayWebsocketServerFrame::Protocol(
                klaw_gateway::GatewayRpcMessage::Notification { method, params },
            ) if *method == klaw_gateway::GatewayProtocolMethod::ItemAgentMessageDelta => {
                params.get("delta").and_then(serde_json::Value::as_str) == Some("Hello")
            }
            _ => false,
        }));
        assert!(frames.iter().all(|frame| matches!(
            frame,
            klaw_gateway::GatewayWebsocketServerFrame::Protocol(_)
        )));
    }

    #[test]
    fn stream_clear_resets_active_delta_state() {
        let frames = stream_events_to_frames(
            "req-2",
            "websocket:test",
            true,
            &[
                ChannelStreamEvent::Snapshot(ChannelResponse {
                    content: "Hello".to_string(),
                    reasoning: Some("visible".to_string()),
                    metadata: BTreeMap::new(),
                    attachments: Vec::new(),
                }),
                ChannelStreamEvent::Clear,
                ChannelStreamEvent::Snapshot(ChannelResponse {
                    content: "Reset".to_string(),
                    reasoning: Some("visible".to_string()),
                    metadata: BTreeMap::new(),
                    attachments: Vec::new(),
                }),
            ],
        );

        assert!(frames.iter().any(|frame| matches!(
            frame,
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, .. }
            if *event == OutboundEvent::SessionStreamClear
        )));
        let last_delta = frames.iter().rev().find_map(|frame| match frame {
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, payload }
                if *event == OutboundEvent::SessionStreamDelta =>
            {
                payload.get("delta").and_then(serde_json::Value::as_str)
            }
            _ => None,
        });
        assert_eq!(last_delta, Some("Reset"));
    }

    #[test]
    fn web_workspace_bootstrap_keeps_web_sessions_after_channel_changes() {
        let workspace = build_web_workspace_bootstrap(vec![
            SessionIndex {
                session_key: "websocket:a1b2c3d4-5678-9012-abcd-ef0123456789".to_string(),
                chat_id: "chat-new".to_string(),
                channel: "websocket".to_string(),
                title: None,
                active_session_key: None,
                model_provider: None,
                model_provider_explicit: false,
                model: None,
                model_explicit: false,
                delivery_metadata_json: None,
                is_active: true,
                created_at_ms: 50,
                updated_at_ms: 60,
                last_message_at_ms: 60,
                turn_count: 1,
                jsonl_path: "new.jsonl".to_string(),
            },
            SessionIndex {
                session_key: "websocket:old".to_string(),
                chat_id: "chat-old".to_string(),
                channel: "websocket".to_string(),
                title: Some("Saved old".to_string()),
                active_session_key: None,
                model_provider: None,
                model_provider_explicit: false,
                model: None,
                model_explicit: false,
                delivery_metadata_json: None,
                is_active: true,
                created_at_ms: 10,
                updated_at_ms: 20,
                last_message_at_ms: 20,
                turn_count: 1,
                jsonl_path: "old.jsonl".to_string(),
            },
        ]);

        assert_eq!(
            workspace.active_session_key.as_deref(),
            Some("websocket:a1b2c3d4-5678-9012-abcd-ef0123456789")
        );
        assert_eq!(workspace.sessions.len(), 2);
        assert_eq!(
            workspace.sessions[0].session_key,
            "websocket:a1b2c3d4-5678-9012-abcd-ef0123456789"
        );
        assert_eq!(workspace.sessions[0].title, "Agent a1b2c3d4");
        assert_eq!(workspace.sessions[1].session_key, "websocket:old");
        assert_eq!(workspace.sessions[1].title, "Saved old");
    }

    #[test]
    fn history_subscription_prefers_active_session_when_present() {
        let base = SessionIndex {
            session_key: "websocket:base".to_string(),
            chat_id: "chat-base".to_string(),
            channel: "websocket".to_string(),
            title: Some("Base".to_string()),
            active_session_key: Some("websocket:base:child".to_string()),
            model_provider: None,
            model_provider_explicit: false,
            model: None,
            model_explicit: false,
            delivery_metadata_json: None,
            is_active: true,
            created_at_ms: 10,
            updated_at_ms: 20,
            last_message_at_ms: 20,
            turn_count: 1,
            jsonl_path: "base.jsonl".to_string(),
        };

        assert_eq!(resolved_history_session_key(&base), "websocket:base:child");
    }

    #[test]
    fn finalize_visible_history_page_marks_has_more_on_visible_overflow() {
        let page = finalize_visible_history_page(
            vec![
                ChatRecord::new("user", "older-visible", Some("msg-1".to_string())),
                ChatRecord::new("assistant", "middle-visible", Some("msg-2".to_string())),
                ChatRecord::new("assistant", "newest-visible", Some("msg-3".to_string())),
            ],
            2,
            false,
        );

        let contents = page
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(contents, vec!["middle-visible", "newest-visible"]);
        assert!(page.has_more);
        assert_eq!(page.oldest_loaded_message_id.as_deref(), Some("msg-2"));
    }

    #[test]
    fn finalize_visible_history_page_keeps_raw_has_more_when_not_full() {
        let page = finalize_visible_history_page(
            vec![ChatRecord::new(
                "assistant",
                "only-visible",
                Some("msg-1".to_string()),
            )],
            3,
            true,
        );

        assert_eq!(page.messages.len(), 1);
        assert!(page.has_more);
        assert_eq!(page.oldest_loaded_message_id.as_deref(), Some("msg-1"));
    }

    #[test]
    fn prepend_visible_history_records_skips_heartbeat_operational_pages() {
        let heartbeat_metadata = serde_json::to_string(&BTreeMap::from([
            ("trigger.kind".to_string(), json!("heartbeat")),
            (
                "heartbeat.silent_ack_token".to_string(),
                json!("HEARTBEAT_OK"),
            ),
        ]))
        .expect("heartbeat metadata");
        let mut visible_records = vec![ChatRecord::new(
            "assistant",
            "newest-visible",
            Some("msg-3".to_string()),
        )];

        prepend_visible_history_records(
            &mut visible_records,
            vec![
                ChatRecord::new("user", "heartbeat prompt", Some("msg-2".to_string()))
                    .with_metadata_json(Some(heartbeat_metadata.clone())),
                ChatRecord::new("assistant", "HEARTBEAT_OK", Some("msg-1".to_string()))
                    .with_metadata_json(Some(heartbeat_metadata)),
            ],
        );

        let contents = visible_records
            .iter()
            .map(|record| record.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(contents, vec!["newest-visible"]);
    }
}
