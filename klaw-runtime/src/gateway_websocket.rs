use crate::{RuntimeBundle, submit_channel_request, submit_channel_request_streaming};
use async_trait::async_trait;
use klaw_channel::{ChannelResponse, ChannelStreamEvent, websocket::WebsocketSubmitEnvelope};
use klaw_config::{AppConfig, WebsocketConfig};
use klaw_gateway::{
    GatewaySessionHistoryMessage, GatewayWebsocketHandler, GatewayWebsocketHandlerError,
    GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest, GatewayWorkspaceBootstrap,
    GatewayWorkspaceSession,
};
use klaw_session::{SessionListQuery, SessionManager};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

const EVENT_SESSION_MESSAGE: &str = "session.message";
const EVENT_SESSION_STREAM_CLEAR: &str = "session.stream.clear";
const EVENT_SESSION_STREAM_DELTA: &str = "session.stream.delta";
const EVENT_SESSION_STREAM_DONE: &str = "session.stream.done";

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
                .list_sessions(SessionListQuery::default())
                .await
                .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        Ok(build_web_workspace_bootstrap(sessions))
    }
}

#[async_trait]
impl GatewayWebsocketHandler for RuntimeWebsocketHandler {
    async fn bootstrap(&self) -> Result<GatewayWorkspaceBootstrap, GatewayWebsocketHandlerError> {
        self.load_web_workspace().await
    }

    async fn create_session(
        &self,
    ) -> Result<GatewayWorkspaceSession, GatewayWebsocketHandlerError> {
        let manager =
            klaw_session::SqliteSessionManager::from_store(self.runtime.session_store.clone());
        let session_key = format!("web:{}", Uuid::new_v4());
        manager
            .touch_session(&session_key, &session_key, "web")
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
    ) -> Result<Vec<GatewaySessionHistoryMessage>, GatewayWebsocketHandlerError> {
        let manager =
            klaw_session::SqliteSessionManager::from_store(self.runtime.session_store.clone());
        let records = manager
            .read_chat_records(session_key)
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        Ok(records
            .into_iter()
            .map(|record| GatewaySessionHistoryMessage {
                role: record.role,
                content: record.content,
                timestamp_ms: record.ts_ms,
                message_id: record.message_id,
            })
            .collect())
    }

    async fn submit(
        &self,
        request: GatewayWebsocketSubmitRequest,
    ) -> Result<Vec<GatewayWebsocketServerFrame>, GatewayWebsocketHandlerError> {
        let config = self.resolve_config(&request.channel_id)?;
        let stream_output = request.stream.unwrap_or(config.stream_output);
        let request_id = request.request_id.clone();
        let channel_request = WebsocketSubmitEnvelope {
            channel_id: request.channel_id.clone(),
            connection_id: request.connection_id,
            request_id: request.request_id.clone(),
            session_key: request.session_key.clone(),
            chat_id: request.chat_id.clone(),
            input: request.input,
            metadata: request.metadata,
        }
        .into_channel_request();

        if stream_output {
            let (stream_events, response) =
                submit_channel_request_streaming(self.runtime.as_ref(), channel_request)
                    .await
                    .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
            let mut frames = stream_events_to_frames(
                &request_id,
                &request.session_key,
                config.show_reasoning,
                &stream_events,
            );
            frames.push(GatewayWebsocketServerFrame::Event {
                event: EVENT_SESSION_STREAM_DONE.to_string(),
                payload: json!({
                    "request_id": request_id,
                    "response": response.as_ref().map(|response| serialize_response(response, config.show_reasoning)),
                    "session_key": request.session_key,
                }),
            });
            frames.push(GatewayWebsocketServerFrame::Result {
                id: request_id,
                result: json!({
                    "response": response.as_ref().map(|response| serialize_response(response, config.show_reasoning)),
                    "session_key": request.session_key,
                    "stream": true,
                }),
            });
            return Ok(frames);
        }

        let response = submit_channel_request(self.runtime.as_ref(), channel_request)
            .await
            .map_err(|err| GatewayWebsocketHandlerError::internal(err.to_string()))?;
        Ok(vec![GatewayWebsocketServerFrame::Result {
            id: request_id,
            result: json!({
                "response": response.as_ref().map(|response| serialize_response(response, config.show_reasoning)),
                "session_key": request.session_key,
                "stream": false,
            }),
        }])
    }
}

fn stream_events_to_frames(
    request_id: &str,
    session_key: &str,
    show_reasoning: bool,
    events: &[ChannelStreamEvent],
) -> Vec<GatewayWebsocketServerFrame> {
    let mut frames = Vec::new();
    let mut last_snapshot: Option<String> = None;
    for event in events {
        match event {
            ChannelStreamEvent::Snapshot(response) => {
                let delta = match last_snapshot.as_deref() {
                    Some(previous) if response.content.starts_with(previous) => {
                        response.content[previous.len()..].to_string()
                    }
                    _ => response.content.clone(),
                };
                last_snapshot = Some(response.content.clone());
                frames.push(GatewayWebsocketServerFrame::Event {
                    event: EVENT_SESSION_MESSAGE.to_string(),
                    payload: json!({
                        "request_id": request_id,
                        "session_key": session_key,
                        "response": serialize_response(response, show_reasoning),
                    }),
                });
                if !delta.is_empty() {
                    frames.push(GatewayWebsocketServerFrame::Event {
                        event: EVENT_SESSION_STREAM_DELTA.to_string(),
                        payload: json!({
                            "request_id": request_id,
                            "session_key": session_key,
                            "delta": delta,
                        }),
                    });
                }
            }
            ChannelStreamEvent::Clear => {
                last_snapshot = None;
                frames.push(GatewayWebsocketServerFrame::Event {
                    event: EVENT_SESSION_STREAM_CLEAR.to_string(),
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
    mut sessions: Vec<klaw_storage::SessionIndex>,
) -> GatewayWorkspaceBootstrap {
    sessions.retain(|session| session.session_key.starts_with("web:"));
    sessions.sort_by(|left, right| {
        left.created_at_ms
            .cmp(&right.created_at_ms)
            .then_with(|| left.session_key.cmp(&right.session_key))
    });
    let mut sessions = sessions
        .into_iter()
        .enumerate()
        .map(|(index, session)| GatewayWorkspaceSession {
            session_key: session.session_key,
            title: session
                .title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| format!("Agent {}", index + 1)),
            created_at_ms: session.created_at_ms,
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| {
        right
            .created_at_ms
            .cmp(&left.created_at_ms)
            .then_with(|| right.session_key.cmp(&left.session_key))
    });
    let active_session_key = sessions.first().map(|session| session.session_key.clone());
    GatewayWorkspaceBootstrap {
        sessions,
        active_session_key,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EVENT_SESSION_MESSAGE, EVENT_SESSION_STREAM_CLEAR, EVENT_SESSION_STREAM_DELTA,
        build_web_workspace_bootstrap, stream_events_to_frames,
    };
    use klaw_channel::{ChannelResponse, ChannelStreamEvent};
    use klaw_storage::SessionIndex;
    use std::collections::BTreeMap;

    #[test]
    fn stream_events_emit_delta_then_done_snapshot_updates() {
        let frames = stream_events_to_frames(
            "req-1",
            "web:test",
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
                assert_eq!(event, EVENT_SESSION_MESSAGE);
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
                assert_eq!(event, EVENT_SESSION_STREAM_DELTA);
                assert_eq!(
                    payload.get("delta").and_then(serde_json::Value::as_str),
                    Some("Hel")
                );
            }
            other => panic!("unexpected frame: {other:?}"),
        }
        match &frames[3] {
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, payload } => {
                assert_eq!(event, EVENT_SESSION_STREAM_DELTA);
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
            "web:test",
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
            if event == EVENT_SESSION_STREAM_CLEAR
        )));
        let last_delta = frames.iter().rev().find_map(|frame| match frame {
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, payload }
                if event == EVENT_SESSION_STREAM_DELTA =>
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
                session_key: "web:old".to_string(),
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
            SessionIndex {
                session_key: "terminal:ignore".to_string(),
                chat_id: "chat-ignore".to_string(),
                channel: "terminal".to_string(),
                title: Some("Terminal".to_string()),
                active_session_key: None,
                model_provider: None,
                model_provider_explicit: false,
                model: None,
                model_explicit: false,
                delivery_metadata_json: None,
                created_at_ms: 30,
                updated_at_ms: 40,
                last_message_at_ms: 40,
                turn_count: 1,
                jsonl_path: "ignore.jsonl".to_string(),
            },
            SessionIndex {
                session_key: "web:new".to_string(),
                chat_id: "chat-new".to_string(),
                channel: "web".to_string(),
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
        ]);

        assert_eq!(workspace.active_session_key.as_deref(), Some("web:new"));
        assert_eq!(workspace.sessions.len(), 2);
        assert_eq!(workspace.sessions[0].session_key, "web:new");
        assert_eq!(workspace.sessions[0].title, "Agent 2");
        assert_eq!(workspace.sessions[1].session_key, "web:old");
        assert_eq!(workspace.sessions[1].title, "Saved old");
    }
}
