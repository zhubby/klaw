use crate::state::GatewayState;
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
use tokio::{spawn, sync::mpsc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, strum::EnumString, strum::AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum InboundMethod {
    #[strum(serialize = "session.ping")]
    SessionPing,
    #[strum(serialize = "provider.list")]
    ProviderList,
    #[strum(serialize = "workspace.bootstrap")]
    WorkspaceBootstrap,
    #[strum(serialize = "session.create")]
    SessionCreate,
    #[strum(serialize = "session.update")]
    SessionUpdate,
    #[strum(serialize = "session.delete")]
    SessionDelete,
    #[strum(serialize = "session.subscribe")]
    SessionSubscribe,
    #[strum(serialize = "session.history.load")]
    SessionHistoryLoad,
    #[strum(serialize = "session.unsubscribe")]
    SessionUnsubscribe,
    #[strum(serialize = "session.submit")]
    SessionSubmit,
}

pub const META_WEBSOCKET_MODEL_PROVIDER: &str = "channel.websocket.model_provider";
pub const META_WEBSOCKET_MODEL: &str = "channel.websocket.model";

#[derive(Debug, Clone, PartialEq, Eq, strum::EnumString, strum::AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum OutboundEvent {
    #[strum(serialize = "session.connected")]
    SessionConnected,
    #[strum(serialize = "session.subscribed")]
    SessionSubscribed,
    #[strum(serialize = "session.history.done")]
    SessionHistoryDone,
    #[strum(serialize = "session.unsubscribed")]
    SessionUnsubscribed,
    #[strum(serialize = "session.message")]
    SessionMessage,
    #[strum(serialize = "session.stream.clear")]
    SessionStreamClear,
    #[strum(serialize = "session.stream.delta")]
    SessionStreamDelta,
    #[strum(serialize = "session.stream.done")]
    SessionStreamDone,
}

impl Serialize for OutboundEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_ref())
    }
}

impl<'de> Deserialize<'de> for OutboundEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<OutboundEvent>()
            .map_err(|_| serde::de::Error::custom(format!("unknown event: {}", s)))
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatQuery {
    #[serde(default)]
    session_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GatewayWebsocketServerFrame {
    Event {
        event: OutboundEvent,
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
        frame_tx: mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    ) -> Result<(), GatewayWebsocketHandlerError>;
}

fn normalize_submit_attachments(
    archive_id: Option<String>,
    attachments: Vec<GatewayWebsocketAttachmentRef>,
) -> Vec<GatewayWebsocketAttachmentRef> {
    let mut normalized = attachments
        .into_iter()
        .filter_map(|attachment| {
            let archive_id = attachment.archive_id.trim().to_string();
            (!archive_id.is_empty()).then_some(GatewayWebsocketAttachmentRef {
                archive_id,
                filename: attachment
                    .filename
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                mime_type: attachment
                    .mime_type
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                size_bytes: attachment.size_bytes,
            })
        })
        .collect::<Vec<_>>();

    if normalized.is_empty()
        && let Some(archive_id) = archive_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    {
        normalized.push(GatewayWebsocketAttachmentRef {
            archive_id,
            filename: None,
            mime_type: None,
            size_bytes: 0,
        });
    }

    normalized
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
    model_provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    archive_id: Option<String>,
    #[serde(default)]
    attachments: Vec<GatewayWebsocketAttachmentRef>,
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
    let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<GatewayWebsocketServerFrame>();
    register_connection(
        &state,
        &connection_id,
        initial_session_key.clone(),
        outgoing_tx.clone(),
    )
    .await;

    let mut current_session_key = initial_session_key;
    if outgoing_tx
        .send(GatewayWebsocketServerFrame::Event {
            event: OutboundEvent::SessionConnected,
            payload: json!({
                "connection_id": connection_id,
                "session_key": current_session_key,
            }),
        })
        .is_err()
    {
        cleanup_connection(state, connection_id).await;
        return;
    }

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
                            &text,
                            outgoing_tx.clone(),
                        )
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
        }
    }

    cleanup_connection(state, connection_id).await;
}

async fn handle_text_message(
    state: &Arc<GatewayState>,
    connection_id: &str,
    current_session_key: &mut Option<String>,
    text: &str,
    outgoing_tx: mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
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
        GatewayWebsocketClientFrame::Method { id, method, params } => {
            let Ok(method) = method.parse::<InboundMethod>() else {
                return vec![error_frame(
                    Some(id),
                    "unknown_method",
                    format!("unsupported websocket method '{method}'"),
                )];
            };
            match method {
                InboundMethod::SessionPing => vec![GatewayWebsocketServerFrame::Result {
                    id,
                    result: json!({ "ok": true }),
                }],
                InboundMethod::ProviderList => {
                    let Some(websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
                            "gateway websocket handler is not configured",
                        )];
                    };
                    match websocket.handler.list_providers().await {
                        Ok(catalog) => vec![GatewayWebsocketServerFrame::Result {
                            id,
                            result: json!({
                                "default_provider": catalog.default_provider,
                                "providers": catalog.providers,
                            }),
                        }],
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
                InboundMethod::WorkspaceBootstrap => {
                    let Some(websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
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
                            vec![GatewayWebsocketServerFrame::Result {
                                id,
                                result: json!({
                                    "sessions": workspace.sessions,
                                    "active_session_key": workspace.active_session_key,
                                }),
                            }]
                        }
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
                InboundMethod::SessionCreate => {
                    let Some(websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
                            "gateway websocket handler is not configured",
                        )];
                    };
                    match websocket.handler.create_session().await {
                        Ok(session) => {
                            *current_session_key = Some(session.session_key.clone());
                            track_connection_session_key(
                                state,
                                connection_id,
                                session.session_key.clone(),
                            )
                            .await;
                            vec![GatewayWebsocketServerFrame::Result {
                                id,
                                result: json!({
                                    "session_key": session.session_key,
                                    "title": session.title,
                                    "created_at_ms": session.created_at_ms,
                                    "model_provider": session.model_provider,
                                    "model": session.model,
                                }),
                            }]
                        }
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
                InboundMethod::SessionUpdate => {
                    let params = match serde_json::from_value::<SessionUpdateParams>(params) {
                        Ok(params) => params,
                        Err(err) => {
                            return vec![error_frame(
                                Some(id),
                                "invalid_params",
                                format!("invalid session.update params: {err}"),
                            )];
                        }
                    };
                    let session_key = params.session_key.trim().to_string();
                    if session_key.is_empty() {
                        return vec![error_frame(
                            Some(id),
                            "invalid_params",
                            "session.update requires a non-empty session_key",
                        )];
                    }
                    let title = params.title.trim().to_string();
                    if title.is_empty() {
                        return vec![error_frame(
                            Some(id),
                            "invalid_params",
                            "session.update requires a non-empty title",
                        )];
                    }
                    let Some(websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
                            "gateway websocket handler is not configured",
                        )];
                    };
                    match websocket.handler.update_session(&session_key, title).await {
                        Ok(session) => vec![GatewayWebsocketServerFrame::Result {
                            id,
                            result: json!({
                                "session_key": session.session_key,
                                "title": session.title,
                                "created_at_ms": session.created_at_ms,
                                "model_provider": session.model_provider,
                                "model": session.model,
                                "updated": true,
                            }),
                        }],
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
                InboundMethod::SessionDelete => {
                    let params = match serde_json::from_value::<SessionDeleteParams>(params) {
                        Ok(params) => params,
                        Err(err) => {
                            return vec![error_frame(
                                Some(id),
                                "invalid_params",
                                format!("invalid session.delete params: {err}"),
                            )];
                        }
                    };
                    let session_key = params.session_key.trim().to_string();
                    if session_key.is_empty() {
                        return vec![error_frame(
                            Some(id),
                            "invalid_params",
                            "session.delete requires a non-empty session_key",
                        )];
                    }
                    let Some(websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
                            "gateway websocket handler is not configured",
                        )];
                    };
                    match websocket.handler.delete_session(&session_key).await {
                        Ok(deleted) => vec![GatewayWebsocketServerFrame::Result {
                            id,
                            result: json!({
                                "session_key": session_key,
                                "deleted": deleted,
                            }),
                        }],
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
                InboundMethod::SessionSubscribe => {
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
                    track_connection_session_key(state, connection_id, session_key.clone()).await;
                    let Some(_websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
                            "gateway websocket handler is not configured",
                        )];
                    };
                    vec![
                        GatewayWebsocketServerFrame::Result {
                            id,
                            result: json!({ "session_key": session_key }),
                        },
                        GatewayWebsocketServerFrame::Event {
                            event: OutboundEvent::SessionSubscribed,
                            payload: json!({ "session_key": session_key }),
                        },
                    ]
                }
                InboundMethod::SessionHistoryLoad => {
                    let params = match serde_json::from_value::<SessionHistoryLoadParams>(params) {
                        Ok(params) => params,
                        Err(err) => {
                            return vec![error_frame(
                                Some(id),
                                "invalid_params",
                                format!("invalid session.history.load params: {err}"),
                            )];
                        }
                    };
                    let session_key = params.session_key.trim().to_string();
                    if session_key.is_empty() {
                        return vec![error_frame(
                            Some(id),
                            "invalid_params",
                            "session.history.load requires a non-empty session_key",
                        )];
                    }
                    let Some(websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
                            "gateway websocket handler is not configured",
                        )];
                    };
                    let limit = params.limit.unwrap_or(10).max(1);
                    match websocket
                        .handler
                        .load_session_history(
                            &session_key,
                            params.before_message_id.as_deref(),
                            limit,
                        )
                        .await
                    {
                        Ok(page) => vec![GatewayWebsocketServerFrame::Result {
                            id,
                            result: json!({
                                "session_key": session_key,
                                "messages": page.messages,
                                "has_more": page.has_more,
                                "oldest_loaded_message_id": page.oldest_loaded_message_id,
                            }),
                        }],
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
                InboundMethod::SessionUnsubscribe => {
                    let previous_session_key = current_session_key.take();
                    clear_connection_session_keys(state, connection_id).await;
                    vec![
                        GatewayWebsocketServerFrame::Result {
                            id,
                            result: json!({ "session_key": previous_session_key }),
                        },
                        GatewayWebsocketServerFrame::Event {
                            event: OutboundEvent::SessionUnsubscribed,
                            payload: json!({ "session_key": previous_session_key }),
                        },
                    ]
                }
                InboundMethod::SessionSubmit => {
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
                    let SessionSubmitParams {
                        input,
                        session_key: submit_session_key,
                        chat_id: submit_chat_id,
                        channel_id: submit_channel_id,
                        stream,
                        model_provider,
                        model,
                        archive_id,
                        attachments,
                        mut metadata,
                    } = params;
                    let input = input.trim().to_string();
                    if input.is_empty() && attachments.is_empty() && archive_id.is_none() {
                        return vec![error_frame(
                            Some(id),
                            "invalid_params",
                            "session.submit requires non-empty input or attachments",
                        )];
                    }
                    if let Some(model_provider) = model_provider
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                    {
                        metadata.insert(
                            META_WEBSOCKET_MODEL_PROVIDER.to_string(),
                            Value::String(model_provider),
                        );
                    }
                    if let Some(model) = model
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                    {
                        metadata.insert(META_WEBSOCKET_MODEL.to_string(), Value::String(model));
                    }
                    let resolved_session_key = submit_session_key
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
                    track_connection_session_key(state, connection_id, session_key.clone()).await;
                    let chat_id = submit_chat_id
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| session_key.clone());
                    let channel_id = submit_channel_id
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "default".to_string());
                    let attachments = normalize_submit_attachments(archive_id, attachments);
                    let Some(websocket) = state.websocket.as_ref() else {
                        return vec![error_frame(
                            Some(id),
                            "not_configured",
                            "gateway websocket handler is not configured",
                        )];
                    };
                    let handler = Arc::clone(&websocket.handler);
                    let submit_connection_id = connection_id.to_string();
                    spawn(async move {
                        let result = handler
                            .submit(
                                GatewayWebsocketSubmitRequest {
                                    connection_id: submit_connection_id,
                                    request_id: id.clone(),
                                    channel_id,
                                    session_key,
                                    chat_id,
                                    input,
                                    attachments,
                                    metadata,
                                    stream,
                                },
                                outgoing_tx.clone(),
                            )
                            .await;
                        if let Err(err) = result {
                            let _ = outgoing_tx.send(GatewayWebsocketServerFrame::Error {
                                id: Some(id),
                                error: GatewayWebsocketErrorFrame {
                                    code: err.code,
                                    message: err.message,
                                    data: err.data,
                                },
                            });
                        }
                    });
                    Vec::new()
                }
            }
        }
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
    frame_tx: mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
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
