use crate::{
    RuntimeBundle, submit_channel_request, submit_channel_request_streaming_with_callback,
};
use async_trait::async_trait;
use klaw_channel::{ChannelResponse, websocket::WebsocketSubmitEnvelope};
use klaw_config::{AppConfig, WebsocketConfig};
use klaw_core::{MediaReference, MediaSourceKind};
use klaw_gateway::{
    GatewayProviderCatalog, GatewayProviderEntry, GatewaySessionHistoryMessage,
    GatewaySessionHistoryPage, GatewayWebsocketAttachmentRef, GatewayWebsocketHandler,
    GatewayWebsocketHandlerError, GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest,
    GatewayWorkspaceBootstrap, GatewayWorkspaceSession, OutboundEvent,
};
use klaw_session::{SessionListQuery, SessionManager};
use klaw_storage::StorageError;
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
        let page = manager
            .read_chat_records_page(
                resolved_history_session_key(&session),
                before_message_id,
                limit,
            )
            .await
            .map_err(|err| match err {
                klaw_session::SessionError::Storage(StorageError::InvalidHistoryCursor(_)) => {
                    GatewayWebsocketHandlerError::invalid_request(err.to_string())
                }
                _ => GatewayWebsocketHandlerError::internal(err.to_string()),
            })?;
        Ok(GatewaySessionHistoryPage {
            messages: page
                .records
                .into_iter()
                .map(|record| GatewaySessionHistoryMessage {
                    role: record.role,
                    content: record.content,
                    timestamp_ms: record.ts_ms,
                    metadata: parse_chat_record_metadata(record.metadata_json.as_deref()),
                    message_id: record.message_id,
                })
                .collect(),
            has_more: page.has_more,
            oldest_loaded_message_id: page.oldest_message_id,
        })
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
            let mut stream_state = GatewayStreamState::default();
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

#[derive(Default)]
struct GatewayStreamState {
    last_snapshot: Option<String>,
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
    let mut frames = Vec::new();
    let mut last_snapshot: Option<String> = None;
    for event in events {
        match event {
            klaw_channel::ChannelStreamEvent::Snapshot(response) => {
                let delta = match last_snapshot.as_deref() {
                    Some(previous) if response.content.starts_with(previous) => {
                        response.content[previous.len()..].to_string()
                    }
                    _ => response.content.clone(),
                };
                last_snapshot = Some(response.content.clone());
                frames.push(GatewayWebsocketServerFrame::Event {
                    event: OutboundEvent::SessionMessage,
                    payload: json!({
                        "request_id": request_id,
                        "session_key": session_key,
                        "response": serialize_response(response, show_reasoning),
                    }),
                });
                if !delta.is_empty() {
                    frames.push(GatewayWebsocketServerFrame::Event {
                        event: OutboundEvent::SessionStreamDelta,
                        payload: json!({
                            "request_id": request_id,
                            "session_key": session_key,
                            "delta": delta,
                        }),
                    });
                }
            }
            klaw_channel::ChannelStreamEvent::Clear => {
                last_snapshot = None;
                frames.push(GatewayWebsocketServerFrame::Event {
                    event: OutboundEvent::SessionStreamClear,
                    payload: json!({
                        "request_id": request_id,
                        "session_key": session_key,
                    }),
                });
            }
        }
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
        build_web_workspace_bootstrap, resolved_history_session_key, stream_events_to_frames,
    };
    use klaw_channel::{ChannelResponse, ChannelStreamEvent};
    use klaw_gateway::OutboundEvent;
    use klaw_storage::SessionIndex;
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
            created_at_ms: 10,
            updated_at_ms: 20,
            last_message_at_ms: 20,
            turn_count: 1,
            jsonl_path: "base.jsonl".to_string(),
        };

        assert_eq!(resolved_history_session_key(&base), "websocket:base:child");
    }
}
