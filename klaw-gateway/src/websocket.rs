use crate::{
    protocol::{
        GatewayContentBlock, GatewayProtocolError, GatewayProtocolErrorCode, GatewayProtocolMethod,
        GatewayRpcMessage, GatewayThreadItem, GatewayThreadItemStatus, GatewayThreadItemType,
        GatewayTurnStatus, GatewayWebsocketProtocolInitializeParams,
        GatewayWebsocketProtocolInitializeResult, GatewayWebsocketTurnStarted,
    },
    state::GatewayState,
};
use async_trait::async_trait;
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use tokio::{
    spawn,
    sync::{RwLock, mpsc},
    task::AbortHandle,
};
use uuid::Uuid;

pub const META_WEBSOCKET_MODEL_PROVIDER: &str = "channel.websocket.model_provider";
pub const META_WEBSOCKET_MODEL: &str = "channel.websocket.model";
pub const META_WEBSOCKET_V1_THREAD_ID: &str = "channel.websocket.v1.thread_id";
pub const META_WEBSOCKET_V1_TURN_ID: &str = "channel.websocket.v1.turn_id";
pub const GATEWAY_WEBSOCKET_MAX_TEXT_FRAME_BYTES: usize = 1024 * 1024;
pub const GATEWAY_WEBSOCKET_OUTBOUND_QUEUE_CAPACITY: usize = 256;
pub const GATEWAY_WEBSOCKET_MAX_ACTIVE_TURNS_PER_CONNECTION: usize = 4;

pub type GatewayWebsocketFrameTx = mpsc::Sender<GatewayWebsocketServerFrame>;

type ActiveTurns = Arc<RwLock<HashMap<String, ActiveTurn>>>;

struct ActiveTurn {
    session_id: String,
    thread_id: String,
    turn_id: String,
    request_id: String,
    abort_handle: AbortHandle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GatewayWebsocketServerFrame {
    Protocol(GatewayRpcMessage),
}

impl Serialize for GatewayWebsocketServerFrame {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Protocol(message) => message.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for GatewayWebsocketServerFrame {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.get("type").is_some() {
            return Err(serde::de::Error::custom(
                "legacy websocket frames are not accepted by the v1 gateway protocol",
            ));
        }

        let message = GatewayRpcMessage::deserialize(value).map_err(serde::de::Error::custom)?;
        Ok(Self::Protocol(message))
    }
}

#[derive(Debug, Clone)]
pub struct GatewayWebsocketSubmitRequest {
    pub connection_id: String,
    pub request_id: String,
    pub channel_id: String,
    pub session_key: String,
    pub chat_id: String,
    pub input: String,
    pub attachments: Vec<GatewayWebsocketAttachmentRef>,
    pub metadata: BTreeMap<String, Value>,
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayWebsocketAttachmentRef {
    pub archive_id: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayProviderEntry {
    pub id: String,
    pub default_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayProviderCatalog {
    pub default_provider: String,
    pub providers: Vec<GatewayProviderEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayWorkspaceSession {
    pub session_key: String,
    pub title: String,
    pub created_at_ms: i64,
    #[serde(default)]
    pub model_provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayWorkspaceBootstrap {
    pub sessions: Vec<GatewayWorkspaceSession>,
    pub active_session_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewaySessionHistoryMessage {
    pub role: String,
    pub content: String,
    pub timestamp_ms: i64,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewaySessionHistoryPage {
    pub messages: Vec<GatewaySessionHistoryMessage>,
    pub has_more: bool,
    #[serde(default)]
    pub oldest_loaded_message_id: Option<String>,
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
    async fn bootstrap(&self) -> Result<GatewayWorkspaceBootstrap, GatewayWebsocketHandlerError>;

    async fn list_providers(&self) -> Result<GatewayProviderCatalog, GatewayWebsocketHandlerError>;

    async fn create_session(&self)
    -> Result<GatewayWorkspaceSession, GatewayWebsocketHandlerError>;

    async fn update_session(
        &self,
        session_key: &str,
        title: String,
    ) -> Result<GatewayWorkspaceSession, GatewayWebsocketHandlerError>;

    async fn delete_session(&self, session_key: &str)
    -> Result<bool, GatewayWebsocketHandlerError>;

    async fn load_session_history(
        &self,
        session_key: &str,
        before_message_id: Option<&str>,
        limit: usize,
    ) -> Result<GatewaySessionHistoryPage, GatewayWebsocketHandlerError>;

    async fn submit(
        &self,
        request: GatewayWebsocketSubmitRequest,
        frame_tx: GatewayWebsocketFrameTx,
    ) -> Result<(), GatewayWebsocketHandlerError>;
}

#[derive(Debug, Deserialize)]
struct SessionSubscribeParams {
    session_key: String,
}

#[derive(Debug, Deserialize)]
struct SessionHistoryLoadParams {
    session_key: String,
    #[serde(default)]
    before_message_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SessionUpdateParams {
    session_key: String,
    title: String,
}

#[derive(Debug, Deserialize)]
struct SessionDeleteParams {
    session_key: String,
}

#[derive(Debug, Deserialize)]
struct V1TurnControlParams {
    #[serde(default)]
    session_id: Option<String>,
    thread_id: String,
    turn_id: String,
}

#[derive(Debug, Deserialize)]
struct V1TurnStartParams {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default, alias = "chat_id")]
    thread_id: Option<String>,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    input: Vec<GatewayContentBlock>,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    model_provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, Value>,
}

pub(crate) async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<GatewayState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: Arc<GatewayState>, mut socket: WebSocket) {
    let connection_id = Uuid::new_v4().to_string();
    let (outgoing_tx, mut outgoing_rx) =
        mpsc::channel::<GatewayWebsocketServerFrame>(GATEWAY_WEBSOCKET_OUTBOUND_QUEUE_CAPACITY);
    let active_turns: ActiveTurns = Arc::new(RwLock::new(HashMap::new()));
    register_connection(&state, &connection_id, None, outgoing_tx.clone()).await;

    let mut current_session_key = None;

    loop {
        tokio::select! {
            maybe_frame = outgoing_rx.recv() => {
                let Some(frame) = maybe_frame else {
                    break;
                };
                if send_frame(&mut socket, &frame).await.is_err() {
                    break;
                }
            }
            maybe_message = socket.next() => {
                let Some(Ok(message)) = maybe_message else {
                    break;
                };
                match message {
                    Message::Text(text) => {
                        let frames = handle_text_message(
                            &state,
                            &connection_id,
                            &mut current_session_key,
                            Arc::clone(&active_turns),
                            &text,
                        )
                        .await;
                        if send_frames(&mut socket, &frames).await.is_err() {
                            break;
                        }
                    }
                    Message::Binary(_) => {
                        if send_frame(
                            &mut socket,
                            &protocol_error_frame(
                                None,
                                GatewayProtocolErrorCode::InvalidRequest,
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
        }
    }

    abort_active_turns(&active_turns).await;
    cleanup_connection(state, connection_id).await;
}

async fn handle_text_message(
    state: &Arc<GatewayState>,
    connection_id: &str,
    current_session_key: &mut Option<String>,
    active_turns: ActiveTurns,
    text: &str,
) -> Vec<GatewayWebsocketServerFrame> {
    if text.len() > GATEWAY_WEBSOCKET_MAX_TEXT_FRAME_BYTES {
        return vec![protocol_error_frame_with_data(
            None,
            GatewayProtocolErrorCode::PayloadTooLarge,
            "websocket text frame exceeds the configured payload limit",
            json!({
                "max_bytes": GATEWAY_WEBSOCKET_MAX_TEXT_FRAME_BYTES,
                "actual_bytes": text.len(),
                "retryable": false,
            }),
        )];
    }

    let raw_value = match serde_json::from_str::<Value>(text) {
        Ok(value) => value,
        Err(err) => {
            return vec![protocol_error_frame(
                None,
                GatewayProtocolErrorCode::InvalidJson,
                format!("invalid websocket frame json: {err}"),
            )];
        }
    };
    if raw_value.get("type").is_some() {
        return vec![protocol_error_frame(
            raw_value
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            GatewayProtocolErrorCode::InvalidRequest,
            "legacy websocket frames are not supported; use gateway websocket v1 JSON-RPC",
        )];
    }
    if raw_value.get("method").is_none() {
        return vec![protocol_error_frame(
            raw_value
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            GatewayProtocolErrorCode::InvalidRequest,
            "websocket frame is not a gateway websocket v1 JSON-RPC message",
        )];
    }

    handle_protocol_message(
        state,
        connection_id,
        current_session_key,
        active_turns,
        raw_value,
    )
    .await
}

async fn handle_protocol_message(
    state: &Arc<GatewayState>,
    connection_id: &str,
    current_session_key: &mut Option<String>,
    active_turns: ActiveTurns,
    raw_value: Value,
) -> Vec<GatewayWebsocketServerFrame> {
    let id = raw_value
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let method = match raw_value
        .get("method")
        .cloned()
        .and_then(|value| serde_json::from_value::<GatewayProtocolMethod>(value).ok())
    {
        Some(method) => method,
        None => {
            return vec![protocol_error_frame(
                id,
                GatewayProtocolErrorCode::MethodNotFound,
                "unsupported gateway websocket v1 method",
            )];
        }
    };
    let params = raw_value
        .get("params")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match method {
        GatewayProtocolMethod::Initialize => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "initialize requires an id",
                )];
            };
            let params =
                match serde_json::from_value::<GatewayWebsocketProtocolInitializeParams>(params) {
                    Ok(params) => params,
                    Err(err) => {
                        return vec![protocol_error_frame(
                            Some(id),
                            GatewayProtocolErrorCode::InvalidParams,
                            format!("invalid initialize params: {err}"),
                        )];
                    }
                };
            let result = GatewayWebsocketProtocolInitializeResult::negotiate(
                connection_id.to_string(),
                params,
            );
            vec![GatewayWebsocketServerFrame::Protocol(
                GatewayRpcMessage::success(id, json!(result)),
            )]
        }
        GatewayProtocolMethod::Initialized => Vec::new(),
        GatewayProtocolMethod::SessionList => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "session/list requires an id",
                )];
            };
            let Some(websocket) = state.websocket.as_ref() else {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InternalError,
                    "gateway websocket handler is not configured",
                )];
            };
            match websocket.handler.bootstrap().await {
                Ok(mut workspace) => {
                    workspace.sessions.sort_by(|left, right| {
                        right
                            .created_at_ms
                            .cmp(&left.created_at_ms)
                            .then_with(|| right.session_key.cmp(&left.session_key))
                    });
                    vec![GatewayWebsocketServerFrame::Protocol(
                        GatewayRpcMessage::success(
                            id,
                            json!({
                                "sessions": workspace.sessions,
                                "active_session_key": workspace.active_session_key,
                            }),
                        ),
                    )]
                }
                Err(err) => vec![handler_protocol_error_frame(Some(id), err)],
            }
        }
        GatewayProtocolMethod::ProviderList => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "provider/list requires an id",
                )];
            };
            let Some(websocket) = state.websocket.as_ref() else {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InternalError,
                    "gateway websocket handler is not configured",
                )];
            };
            match websocket.handler.list_providers().await {
                Ok(catalog) => vec![GatewayWebsocketServerFrame::Protocol(
                    GatewayRpcMessage::success(
                        id,
                        json!({
                            "default_provider": catalog.default_provider,
                            "providers": catalog.providers,
                        }),
                    ),
                )],
                Err(err) => vec![handler_protocol_error_frame(Some(id), err)],
            }
        }
        GatewayProtocolMethod::SessionCreate => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "session/create requires an id",
                )];
            };
            let Some(websocket) = state.websocket.as_ref() else {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InternalError,
                    "gateway websocket handler is not configured",
                )];
            };
            match websocket.handler.create_session().await {
                Ok(session) => {
                    *current_session_key = Some(session.session_key.clone());
                    track_connection_session_key(state, connection_id, session.session_key.clone())
                        .await;
                    vec![GatewayWebsocketServerFrame::Protocol(
                        GatewayRpcMessage::success(
                            id,
                            json!({
                                "session_key": session.session_key,
                                "title": session.title,
                                "created_at_ms": session.created_at_ms,
                                "model_provider": session.model_provider,
                                "model": session.model,
                            }),
                        ),
                    )]
                }
                Err(err) => vec![handler_protocol_error_frame(Some(id), err)],
            }
        }
        GatewayProtocolMethod::SessionUpdate => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "session/update requires an id",
                )];
            };
            let params = match serde_json::from_value::<SessionUpdateParams>(params) {
                Ok(params) => params,
                Err(err) => {
                    return vec![protocol_error_frame(
                        Some(id),
                        GatewayProtocolErrorCode::InvalidParams,
                        format!("invalid session/update params: {err}"),
                    )];
                }
            };
            let session_key = params.session_key.trim().to_string();
            if session_key.is_empty() {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InvalidParams,
                    "session/update requires a non-empty session_key",
                )];
            }
            let title = params.title.trim().to_string();
            if title.is_empty() {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InvalidParams,
                    "session/update requires a non-empty title",
                )];
            }
            let Some(websocket) = state.websocket.as_ref() else {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InternalError,
                    "gateway websocket handler is not configured",
                )];
            };
            match websocket.handler.update_session(&session_key, title).await {
                Ok(session) => vec![GatewayWebsocketServerFrame::Protocol(
                    GatewayRpcMessage::success(
                        id,
                        json!({
                            "session_key": session.session_key,
                            "title": session.title,
                            "created_at_ms": session.created_at_ms,
                            "model_provider": session.model_provider,
                            "model": session.model,
                            "updated": true,
                        }),
                    ),
                )],
                Err(err) => vec![handler_protocol_error_frame(Some(id), err)],
            }
        }
        GatewayProtocolMethod::SessionDelete => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "session/delete requires an id",
                )];
            };
            let params = match serde_json::from_value::<SessionDeleteParams>(params) {
                Ok(params) => params,
                Err(err) => {
                    return vec![protocol_error_frame(
                        Some(id),
                        GatewayProtocolErrorCode::InvalidParams,
                        format!("invalid session/delete params: {err}"),
                    )];
                }
            };
            let session_key = params.session_key.trim().to_string();
            if session_key.is_empty() {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InvalidParams,
                    "session/delete requires a non-empty session_key",
                )];
            }
            let Some(websocket) = state.websocket.as_ref() else {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InternalError,
                    "gateway websocket handler is not configured",
                )];
            };
            match websocket.handler.delete_session(&session_key).await {
                Ok(deleted) => vec![GatewayWebsocketServerFrame::Protocol(
                    GatewayRpcMessage::success(
                        id,
                        json!({
                            "session_key": session_key,
                            "deleted": deleted,
                        }),
                    ),
                )],
                Err(err) => vec![handler_protocol_error_frame(Some(id), err)],
            }
        }
        GatewayProtocolMethod::SessionSubscribe => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "session/subscribe requires an id",
                )];
            };
            let params = match serde_json::from_value::<SessionSubscribeParams>(params) {
                Ok(params) => params,
                Err(err) => {
                    return vec![protocol_error_frame(
                        Some(id),
                        GatewayProtocolErrorCode::InvalidParams,
                        format!("invalid session/subscribe params: {err}"),
                    )];
                }
            };
            let session_key = params.session_key.trim().to_string();
            if session_key.is_empty() {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InvalidParams,
                    "session/subscribe requires a non-empty session_key",
                )];
            }
            let Some(_websocket) = state.websocket.as_ref() else {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InternalError,
                    "gateway websocket handler is not configured",
                )];
            };
            *current_session_key = Some(session_key.clone());
            track_connection_session_key(state, connection_id, session_key.clone()).await;
            let payload = json!({ "session_key": session_key });
            vec![
                GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::success(
                    id,
                    payload.clone(),
                )),
                GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
                    GatewayProtocolMethod::SessionSubscribed,
                    payload,
                )),
            ]
        }
        GatewayProtocolMethod::SessionUnsubscribe => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "session/unsubscribe requires an id",
                )];
            };
            let previous_session_key = current_session_key.take();
            clear_connection_session_keys(state, connection_id).await;
            let payload = json!({ "session_key": previous_session_key });
            vec![
                GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::success(
                    id,
                    payload.clone(),
                )),
                GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
                    GatewayProtocolMethod::SessionUnsubscribed,
                    payload,
                )),
            ]
        }
        GatewayProtocolMethod::ThreadHistory | GatewayProtocolMethod::ThreadRead => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "thread/history requires an id",
                )];
            };
            let params = match serde_json::from_value::<SessionHistoryLoadParams>(params) {
                Ok(params) => params,
                Err(err) => {
                    return vec![protocol_error_frame(
                        Some(id),
                        GatewayProtocolErrorCode::InvalidParams,
                        format!("invalid thread/history params: {err}"),
                    )];
                }
            };
            let session_key = params.session_key.trim().to_string();
            if session_key.is_empty() {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InvalidParams,
                    "thread/history requires a non-empty session_key",
                )];
            }
            let Some(websocket) = state.websocket.as_ref() else {
                return vec![protocol_error_frame(
                    Some(id),
                    GatewayProtocolErrorCode::InternalError,
                    "gateway websocket handler is not configured",
                )];
            };
            let limit = params.limit.unwrap_or(10).max(1);
            match websocket
                .handler
                .load_session_history(&session_key, params.before_message_id.as_deref(), limit)
                .await
            {
                Ok(page) => vec![GatewayWebsocketServerFrame::Protocol(
                    GatewayRpcMessage::success(
                        id,
                        json!({
                            "session_key": session_key,
                            "thread_id": session_key,
                            "messages": page.messages,
                            "has_more": page.has_more,
                            "oldest_loaded_message_id": page.oldest_loaded_message_id,
                        }),
                    ),
                )],
                Err(err) => vec![handler_protocol_error_frame(Some(id), err)],
            }
        }
        GatewayProtocolMethod::TurnStart => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "turn/start requires an id",
                )];
            };
            let params = match serde_json::from_value::<V1TurnStartParams>(params) {
                Ok(params) => params,
                Err(err) => {
                    return vec![protocol_error_frame(
                        Some(id),
                        GatewayProtocolErrorCode::InvalidParams,
                        format!("invalid turn/start params: {err}"),
                    )];
                }
            };
            handle_protocol_turn_start(
                state,
                connection_id,
                current_session_key,
                active_turns,
                id,
                params,
            )
            .await
        }
        GatewayProtocolMethod::ApprovalRespond
        | GatewayProtocolMethod::ToolRespond
        | GatewayProtocolMethod::UserInputRespond => {
            vec![protocol_error_frame(
                id,
                GatewayProtocolErrorCode::MethodNotFound,
                "gateway websocket v1 server request responses are not wired to runtime handling yet",
            )]
        }
        GatewayProtocolMethod::TurnCancel => {
            let Some(id) = id else {
                return vec![protocol_error_frame(
                    None,
                    GatewayProtocolErrorCode::InvalidRequest,
                    "turn/cancel requires an id",
                )];
            };
            let params = match serde_json::from_value::<V1TurnControlParams>(params) {
                Ok(params) => params,
                Err(err) => {
                    return vec![protocol_error_frame(
                        Some(id),
                        GatewayProtocolErrorCode::InvalidParams,
                        format!("invalid turn/cancel params: {err}"),
                    )];
                }
            };
            handle_protocol_turn_cancel(active_turns, id, params).await
        }
        _ => vec![protocol_error_frame(
            id,
            GatewayProtocolErrorCode::MethodNotFound,
            "gateway websocket v1 method is not implemented yet",
        )],
    }
}

async fn handle_protocol_turn_start(
    state: &Arc<GatewayState>,
    connection_id: &str,
    current_session_key: &mut Option<String>,
    active_turns: ActiveTurns,
    request_id: String,
    params: V1TurnStartParams,
) -> Vec<GatewayWebsocketServerFrame> {
    let Some(websocket) = state.websocket.as_ref() else {
        return vec![protocol_error_frame(
            Some(request_id),
            GatewayProtocolErrorCode::InternalError,
            "gateway websocket handler is not configured",
        )];
    };
    let input = render_v1_input(&params.input);
    let attachments = extract_v1_attachments(&params.input);
    if input.trim().is_empty() && attachments.is_empty() {
        return vec![protocol_error_frame(
            Some(request_id),
            GatewayProtocolErrorCode::InvalidParams,
            "turn/start requires non-empty input or attachments",
        )];
    }

    let resolved_session_id = params
        .session_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| current_session_key.clone());
    let Some(session_id) = resolved_session_id else {
        return vec![protocol_error_frame(
            Some(request_id),
            GatewayProtocolErrorCode::InvalidParams,
            "turn/start requires a session_id or subscribed session",
        )];
    };

    let thread_id = params
        .thread_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| session_id.clone());
    let turn_id = params
        .turn_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("turn_{request_id}"));
    {
        let active = active_turns.read().await;
        if active.len() >= GATEWAY_WEBSOCKET_MAX_ACTIVE_TURNS_PER_CONNECTION {
            return vec![protocol_error_frame_with_data(
                Some(request_id),
                GatewayProtocolErrorCode::TooManyActiveTurns,
                "too many active websocket v1 turns for this connection",
                json!({
                    "max_active_turns": GATEWAY_WEBSOCKET_MAX_ACTIVE_TURNS_PER_CONNECTION,
                    "retryable": true,
                }),
            )];
        }
        if active.contains_key(&turn_id) {
            return vec![protocol_error_frame(
                Some(request_id),
                GatewayProtocolErrorCode::InvalidParams,
                "turn/start received a duplicate active turn_id",
            )];
        }
    }
    let channel_id = params
        .channel_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "default".to_string());

    *current_session_key = Some(session_id.clone());
    track_connection_session_key(state, connection_id, session_id.clone()).await;

    let mut metadata = params.metadata;
    metadata.insert(
        META_WEBSOCKET_V1_THREAD_ID.to_string(),
        Value::String(thread_id.clone()),
    );
    metadata.insert(
        META_WEBSOCKET_V1_TURN_ID.to_string(),
        Value::String(turn_id.clone()),
    );
    if let Some(model_provider) = params
        .model_provider
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        metadata.insert(
            META_WEBSOCKET_MODEL_PROVIDER.to_string(),
            Value::String(model_provider),
        );
    }
    if let Some(model) = params
        .model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        metadata.insert(META_WEBSOCKET_MODEL.to_string(), Value::String(model));
    }

    let (turn_frame_tx, mut turn_frame_rx) =
        mpsc::channel::<GatewayWebsocketServerFrame>(GATEWAY_WEBSOCKET_OUTBOUND_QUEUE_CAPACITY);
    let fanout_state = Arc::clone(state);
    let fanout_session_id = session_id.clone();
    let _fanout_handle = spawn(async move {
        while let Some(frame) = turn_frame_rx.recv().await {
            fanout_state
                .websocket_broadcaster
                .broadcast_to_session(&fanout_session_id, frame)
                .await;
        }
    });

    let turn = GatewayWebsocketTurnStarted {
        session_id: session_id.clone(),
        thread_id: thread_id.clone(),
        turn_id: turn_id.clone(),
        request_id: request_id.clone(),
        status: GatewayTurnStatus::InProgress,
    };
    let _ = turn_frame_tx.try_send(v1_user_message_completed_frame(
        &session_id,
        &thread_id,
        &turn_id,
        &input,
        &attachments,
        &metadata,
    ));
    let _ = turn_frame_tx.try_send(GatewayWebsocketServerFrame::Protocol(
        GatewayRpcMessage::notification(GatewayProtocolMethod::TurnStarted, json!(turn.clone())),
    ));

    let handler = Arc::clone(&websocket.handler);
    let submit_connection_id = connection_id.to_string();
    let submit_request_id = request_id.clone();
    let submit_session_id = session_id.clone();
    let submit_thread_id = thread_id.clone();
    let submit_turn_id = turn_id.clone();
    let active_turns_for_task = Arc::clone(&active_turns);
    let handle = spawn(async move {
        let result = handler
            .submit(
                GatewayWebsocketSubmitRequest {
                    connection_id: submit_connection_id,
                    request_id: submit_request_id.clone(),
                    channel_id,
                    session_key: submit_session_id.clone(),
                    chat_id: submit_thread_id.clone(),
                    input,
                    attachments,
                    metadata,
                    stream: params.stream,
                },
                turn_frame_tx.clone(),
            )
            .await;
        if let Err(err) = result {
            let _ = turn_frame_tx.try_send(v1_turn_failed_frame(
                &submit_session_id,
                &submit_thread_id,
                &submit_turn_id,
                &submit_request_id,
                err,
            ));
        }
        active_turns_for_task.write().await.remove(&submit_turn_id);
    });
    active_turns.write().await.insert(
        turn_id.clone(),
        ActiveTurn {
            session_id: session_id.clone(),
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            request_id: request_id.clone(),
            abort_handle: handle.abort_handle(),
        },
    );
    if handle.is_finished() {
        active_turns.write().await.remove(&turn_id);
    }

    vec![GatewayWebsocketServerFrame::Protocol(
        GatewayRpcMessage::success(request_id, json!({ "turn": turn })),
    )]
}

fn v1_user_message_completed_frame(
    session_id: &str,
    thread_id: &str,
    turn_id: &str,
    input: &str,
    attachments: &[GatewayWebsocketAttachmentRef],
    metadata: &BTreeMap<String, Value>,
) -> GatewayWebsocketServerFrame {
    let item = GatewayThreadItem {
        item_id: format!("item_user_{turn_id}"),
        turn_id: turn_id.to_string(),
        item_type: GatewayThreadItemType::UserMessage,
        status: GatewayThreadItemStatus::Completed,
        payload: json!({
            "session_id": session_id,
            "thread_id": thread_id,
            "message": {
                "content": input,
                "metadata": metadata,
                "attachments": attachments,
            },
        }),
    };
    GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
        GatewayProtocolMethod::ItemCompleted,
        json!({
            "session_id": session_id,
            "thread_id": thread_id,
            "turn_id": turn_id,
            "item": item,
        }),
    ))
}

async fn handle_protocol_turn_cancel(
    active_turns: ActiveTurns,
    request_id: String,
    params: V1TurnControlParams,
) -> Vec<GatewayWebsocketServerFrame> {
    let Some(active_turn) = active_turns.write().await.remove(&params.turn_id) else {
        return vec![protocol_error_frame(
            Some(request_id),
            GatewayProtocolErrorCode::TurnNotFound,
            "turn/cancel could not find an active turn with the requested turn_id",
        )];
    };
    if active_turn.thread_id != params.thread_id {
        active_turns
            .write()
            .await
            .insert(active_turn.turn_id.clone(), active_turn);
        return vec![protocol_error_frame(
            Some(request_id),
            GatewayProtocolErrorCode::ThreadNotFound,
            "turn/cancel thread_id does not match the active turn",
        )];
    }
    active_turn.abort_handle.abort();
    let payload = json!({
        "session_id": params
            .session_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(active_turn.session_id),
        "thread_id": active_turn.thread_id,
        "turn_id": active_turn.turn_id,
        "request_id": active_turn.request_id,
        "status": GatewayTurnStatus::Interrupted,
    });
    vec![
        GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::success(
            request_id,
            json!({
                "status": GatewayTurnStatus::Interrupted,
                "turn": payload,
            }),
        )),
        GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
            GatewayProtocolMethod::TurnInterrupted,
            payload,
        )),
    ]
}

async fn abort_active_turns(active_turns: &ActiveTurns) {
    let active = std::mem::take(&mut *active_turns.write().await);
    for (_, active_turn) in active {
        active_turn.abort_handle.abort();
    }
}

fn v1_turn_failed_frame(
    session_id: &str,
    thread_id: &str,
    turn_id: &str,
    request_id: &str,
    err: GatewayWebsocketHandlerError,
) -> GatewayWebsocketServerFrame {
    let error = handler_protocol_error(err);
    GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::notification(
        GatewayProtocolMethod::TurnFailed,
        json!({
            "session_id": session_id,
            "thread_id": thread_id,
            "turn_id": turn_id,
            "request_id": request_id,
            "status": GatewayTurnStatus::Failed,
            "error": error,
        }),
    ))
}

fn render_v1_input(input: &[GatewayContentBlock]) -> String {
    input
        .iter()
        .filter_map(|block| match block {
            GatewayContentBlock::Text { text } => {
                let trimmed = text.trim();
                (!trimmed.is_empty()).then_some(trimmed.to_string())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_v1_attachments(input: &[GatewayContentBlock]) -> Vec<GatewayWebsocketAttachmentRef> {
    input
        .iter()
        .filter_map(|block| match block {
            GatewayContentBlock::Attachment {
                archive_id,
                filename,
                mime_type,
                size_bytes,
            } => {
                let archive_id = archive_id.trim().to_string();
                (!archive_id.is_empty()).then_some(GatewayWebsocketAttachmentRef {
                    archive_id,
                    filename: filename.clone(),
                    mime_type: mime_type.clone(),
                    size_bytes: *size_bytes,
                })
            }
            GatewayContentBlock::Image {
                archive_id: Some(archive_id),
                mime_type,
                ..
            } => {
                let archive_id = archive_id.trim().to_string();
                (!archive_id.is_empty()).then_some(GatewayWebsocketAttachmentRef {
                    archive_id,
                    filename: None,
                    mime_type: Some(mime_type.clone()),
                    size_bytes: 0,
                })
            }
            _ => None,
        })
        .collect()
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

fn protocol_error_frame(
    id: Option<String>,
    code: GatewayProtocolErrorCode,
    message: impl Into<String>,
) -> GatewayWebsocketServerFrame {
    GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::error(id, code, message))
}

fn protocol_error_frame_with_data(
    id: Option<String>,
    code: GatewayProtocolErrorCode,
    message: impl Into<String>,
    data: Value,
) -> GatewayWebsocketServerFrame {
    GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::Error {
        id,
        error: GatewayProtocolError {
            code,
            message: message.into(),
            data: Some(data),
        },
    })
}

fn handler_protocol_error_frame(
    id: Option<String>,
    err: GatewayWebsocketHandlerError,
) -> GatewayWebsocketServerFrame {
    let error = handler_protocol_error(err);
    GatewayWebsocketServerFrame::Protocol(GatewayRpcMessage::Error { id, error })
}

fn handler_protocol_error(err: GatewayWebsocketHandlerError) -> GatewayProtocolError {
    let code = match err.code.as_str() {
        "invalid_request" => GatewayProtocolErrorCode::InvalidParams,
        "session_not_found" | "missing_session" => GatewayProtocolErrorCode::SessionNotFound,
        "thread_not_found" => GatewayProtocolErrorCode::ThreadNotFound,
        "turn_not_found" => GatewayProtocolErrorCode::TurnNotFound,
        "permission_denied" => GatewayProtocolErrorCode::PermissionDenied,
        "timeout" => GatewayProtocolErrorCode::Timeout,
        _ => GatewayProtocolErrorCode::InternalError,
    };
    GatewayProtocolError {
        code,
        message: err.message,
        data: err.data,
    }
}

async fn register_connection(
    state: &GatewayState,
    connection_id: &str,
    session_key: Option<String>,
    frame_tx: GatewayWebsocketFrameTx,
) {
    state
        .websocket_broadcaster
        .register(connection_id.to_string(), session_key, frame_tx)
        .await;
}

async fn track_connection_session_key(
    state: &GatewayState,
    connection_id: &str,
    session_key: String,
) {
    state
        .websocket_broadcaster
        .track_session_key(connection_id, session_key)
        .await;
}

async fn clear_connection_session_keys(state: &GatewayState, connection_id: &str) {
    state
        .websocket_broadcaster
        .clear_session_keys(connection_id)
        .await;
}

async fn cleanup_connection(state: Arc<GatewayState>, connection_id: String) {
    state.websocket_broadcaster.cleanup(&connection_id).await;
}
