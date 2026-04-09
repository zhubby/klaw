use crate::state::{GatewayState, GatewayWebsocketConnection};
use async_trait::async_trait;
use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

const METHOD_SESSION_PING: &str = "session.ping";
const METHOD_SESSION_SUBSCRIBE: &str = "session.subscribe";
const METHOD_SESSION_UNSUBSCRIBE: &str = "session.unsubscribe";
const METHOD_SESSION_SUBMIT: &str = "session.submit";

const EVENT_SESSION_CONNECTED: &str = "session.connected";
const EVENT_SESSION_SUBSCRIBED: &str = "session.subscribed";
const EVENT_SESSION_UNSUBSCRIBED: &str = "session.unsubscribed";

#[derive(Debug, Deserialize)]
pub(crate) struct ChatQuery {
    #[serde(default)]
    session_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GatewayWebsocketServerFrame {
    Event {
        event: String,
        #[serde(default)]
        payload: Value,
    },
    Result {
        id: String,
        #[serde(default)]
        result: Value,
    },
    Error {
        #[serde(default)]
        id: Option<String>,
        error: GatewayWebsocketErrorFrame,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayWebsocketErrorFrame {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct GatewayWebsocketSubmitRequest {
    pub connection_id: String,
    pub request_id: String,
    pub channel_id: String,
    pub session_key: String,
    pub chat_id: String,
    pub input: String,
    pub metadata: BTreeMap<String, Value>,
    pub stream: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct GatewayWebsocketHandlerError {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

impl GatewayWebsocketHandlerError {
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_request".to_string(),
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal_error".to_string(),
            message: message.into(),
            data: None,
        }
    }
}

#[async_trait]
pub trait GatewayWebsocketHandler: Send + Sync {
    async fn submit(
        &self,
        request: GatewayWebsocketSubmitRequest,
    ) -> Result<Vec<GatewayWebsocketServerFrame>, GatewayWebsocketHandlerError>;
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GatewayWebsocketClientFrame {
    Method {
        id: String,
        method: String,
        #[serde(default)]
        params: Value,
    },
}

#[derive(Debug, Deserialize)]
struct SessionSubscribeParams {
    session_key: String,
}

#[derive(Debug, Deserialize)]
struct SessionSubmitParams {
    input: String,
    #[serde(default)]
    session_key: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
}

pub(crate) async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<ChatQuery>,
) -> Response {
    let initial_session_key = query
        .session_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    ws.on_upgrade(move |socket| handle_socket(state, initial_session_key, socket))
}

async fn handle_socket(
    state: Arc<GatewayState>,
    initial_session_key: Option<String>,
    mut socket: WebSocket,
) {
    let connection_id = Uuid::new_v4().to_string();
    register_connection(&state, &connection_id, initial_session_key.clone()).await;

    let mut current_session_key = initial_session_key;
    if send_frame(
        &mut socket,
        &GatewayWebsocketServerFrame::Event {
            event: EVENT_SESSION_CONNECTED.to_string(),
            payload: json!({
                "connection_id": connection_id,
                "session_key": current_session_key,
            }),
        },
    )
    .await
    .is_err()
    {
        cleanup_connection(state, connection_id).await;
        return;
    }

    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(text) => {
                let frames =
                    handle_text_message(&state, &connection_id, &mut current_session_key, &text)
                        .await;
                if send_frames(&mut socket, &frames).await.is_err() {
                    break;
                }
            }
            Message::Binary(_) => {
                if send_frame(
                    &mut socket,
                    &error_frame(
                        None,
                        "invalid_message_type",
                        "binary websocket frames are not supported",
                    ),
                )
                .await
                .is_err()
                {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Ping(payload) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    break;
                }
            }
            Message::Pong(_) => {}
        }
    }

    cleanup_connection(state, connection_id).await;
}

async fn handle_text_message(
    state: &Arc<GatewayState>,
    connection_id: &str,
    current_session_key: &mut Option<String>,
    text: &str,
) -> Vec<GatewayWebsocketServerFrame> {
    let frame = match serde_json::from_str::<GatewayWebsocketClientFrame>(text) {
        Ok(frame) => frame,
        Err(err) => {
            return vec![error_frame(
                None,
                "invalid_json",
                format!("invalid websocket frame json: {err}"),
            )];
        }
    };

    match frame {
        GatewayWebsocketClientFrame::Method { id, method, params } => match method.as_str() {
            METHOD_SESSION_PING => vec![GatewayWebsocketServerFrame::Result {
                id,
                result: json!({ "ok": true }),
            }],
            METHOD_SESSION_SUBSCRIBE => {
                let params = match serde_json::from_value::<SessionSubscribeParams>(params) {
                    Ok(params) => params,
                    Err(err) => {
                        return vec![error_frame(
                            Some(id),
                            "invalid_params",
                            format!("invalid session.subscribe params: {err}"),
                        )];
                    }
                };
                let session_key = params.session_key.trim().to_string();
                if session_key.is_empty() {
                    return vec![error_frame(
                        Some(id),
                        "invalid_params",
                        "session.subscribe requires a non-empty session_key",
                    )];
                }
                *current_session_key = Some(session_key.clone());
                update_connection_session_key(state, connection_id, Some(session_key.clone()))
                    .await;
                vec![
                    GatewayWebsocketServerFrame::Result {
                        id,
                        result: json!({ "session_key": session_key }),
                    },
                    GatewayWebsocketServerFrame::Event {
                        event: EVENT_SESSION_SUBSCRIBED.to_string(),
                        payload: json!({ "session_key": current_session_key }),
                    },
                ]
            }
            METHOD_SESSION_UNSUBSCRIBE => {
                let previous_session_key = current_session_key.take();
                update_connection_session_key(state, connection_id, None).await;
                vec![
                    GatewayWebsocketServerFrame::Result {
                        id,
                        result: json!({ "session_key": previous_session_key }),
                    },
                    GatewayWebsocketServerFrame::Event {
                        event: EVENT_SESSION_UNSUBSCRIBED.to_string(),
                        payload: json!({ "session_key": previous_session_key }),
                    },
                ]
            }
            METHOD_SESSION_SUBMIT => {
                let params = match serde_json::from_value::<SessionSubmitParams>(params) {
                    Ok(params) => params,
                    Err(err) => {
                        return vec![error_frame(
                            Some(id),
                            "invalid_params",
                            format!("invalid session.submit params: {err}"),
                        )];
                    }
                };
                let input = params.input.trim().to_string();
                if input.is_empty() {
                    return vec![error_frame(
                        Some(id),
                        "invalid_params",
                        "session.submit requires a non-empty input",
                    )];
                }
                let resolved_session_key = params
                    .session_key
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .or_else(|| current_session_key.clone());
                let Some(session_key) = resolved_session_key else {
                    return vec![error_frame(
                        Some(id),
                        "missing_session",
                        "session.submit requires a subscribed session_key",
                    )];
                };
                *current_session_key = Some(session_key.clone());
                update_connection_session_key(state, connection_id, Some(session_key.clone()))
                    .await;
                let chat_id = params
                    .chat_id
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| session_key.clone());
                let channel_id = params
                    .channel_id
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "default".to_string());
                let Some(websocket) = state.websocket.as_ref() else {
                    return vec![error_frame(
                        Some(id),
                        "not_configured",
                        "gateway websocket handler is not configured",
                    )];
                };
                match websocket
                    .handler
                    .submit(GatewayWebsocketSubmitRequest {
                        connection_id: connection_id.to_string(),
                        request_id: id.clone(),
                        channel_id,
                        session_key,
                        chat_id,
                        input,
                        metadata: params.metadata,
                        stream: params.stream,
                    })
                    .await
                {
                    Ok(frames) => frames,
                    Err(err) => vec![GatewayWebsocketServerFrame::Error {
                        id: Some(id),
                        error: GatewayWebsocketErrorFrame {
                            code: err.code,
                            message: err.message,
                            data: err.data,
                        },
                    }],
                }
            }
            _ => vec![error_frame(
                Some(id),
                "unknown_method",
                format!("unsupported websocket method '{method}'"),
            )],
        },
    }
}

async fn send_frames(
    socket: &mut WebSocket,
    frames: &[GatewayWebsocketServerFrame],
) -> Result<(), axum::Error> {
    for frame in frames {
        send_frame(socket, frame).await?;
    }
    Ok(())
}

async fn send_frame(
    socket: &mut WebSocket,
    frame: &GatewayWebsocketServerFrame,
) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(frame).map_err(axum::Error::new)?;
    socket.send(Message::Text(payload.into())).await
}

fn error_frame(
    id: Option<String>,
    code: impl Into<String>,
    message: impl Into<String>,
) -> GatewayWebsocketServerFrame {
    GatewayWebsocketServerFrame::Error {
        id,
        error: GatewayWebsocketErrorFrame {
            code: code.into(),
            message: message.into(),
            data: None,
        },
    }
}

async fn register_connection(
    state: &GatewayState,
    connection_id: &str,
    session_key: Option<String>,
) {
    state.websocket_connections.write().await.insert(
        connection_id.to_string(),
        GatewayWebsocketConnection { session_key },
    );
}

async fn update_connection_session_key(
    state: &GatewayState,
    connection_id: &str,
    session_key: Option<String>,
) {
    let mut connections = state.websocket_connections.write().await;
    if let Some(connection) = connections.get_mut(connection_id) {
        connection.session_key = session_key;
    }
}

async fn cleanup_connection(state: Arc<GatewayState>, connection_id: String) {
    state
        .websocket_connections
        .write()
        .await
        .remove(&connection_id);
}
