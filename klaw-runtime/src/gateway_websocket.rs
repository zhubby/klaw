use crate::{RuntimeBundle, submit_channel_request, submit_channel_request_streaming};
use async_trait::async_trait;
use klaw_channel::{ChannelResponse, ChannelStreamEvent, websocket::WebsocketSubmitEnvelope};
use klaw_config::{AppConfig, WebsocketConfig};
use klaw_gateway::{
    GatewayWebsocketHandler, GatewayWebsocketHandlerError, GatewayWebsocketServerFrame,
    GatewayWebsocketSubmitRequest,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

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
}

#[async_trait]
impl GatewayWebsocketHandler for RuntimeWebsocketHandler {
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
            let mut frames =
                stream_events_to_frames(&request_id, config.show_reasoning, &stream_events);
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
                        "response": serialize_response(response, show_reasoning),
                    }),
                });
                if !delta.is_empty() {
                    frames.push(GatewayWebsocketServerFrame::Event {
                        event: EVENT_SESSION_STREAM_DELTA.to_string(),
                        payload: json!({
                            "request_id": request_id,
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

#[cfg(test)]
mod tests {
    use super::{
        EVENT_SESSION_MESSAGE, EVENT_SESSION_STREAM_CLEAR, EVENT_SESSION_STREAM_DELTA,
        stream_events_to_frames,
    };
    use klaw_channel::{ChannelResponse, ChannelStreamEvent};
    use std::collections::BTreeMap;

    #[test]
    fn stream_events_emit_delta_then_done_snapshot_updates() {
        let frames = stream_events_to_frames(
            "req-1",
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
}
