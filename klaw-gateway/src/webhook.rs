use crate::{
    GatewayError,
    state::{GatewayState, GatewayWebhookState},
};
use async_trait::async_trait;
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use klaw_config::GatewayConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayWebhookRequest {
    pub event_id: String,
    pub source: String,
    pub event_type: String,
    pub content: String,
    pub session_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_session_key: Option<String>,
    pub chat_id: String,
    pub sender_id: String,
    pub payload: Option<Value>,
    pub metadata: BTreeMap<String, Value>,
    pub remote_addr: Option<String>,
    pub received_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayWebhookResponse {
    pub event_id: String,
    pub status: String,
    pub session_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayWebhookAgentRequest {
    pub request_id: String,
    pub hook_id: String,
    pub session_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_session_key: Option<String>,
    pub chat_id: String,
    pub sender_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub body: Value,
    pub metadata: BTreeMap<String, Value>,
    pub remote_addr: Option<String>,
    pub received_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayWebhookAgentResponse {
    pub request_id: String,
    pub status: String,
    pub hook_id: String,
    pub session_key: String,
}

#[derive(Debug, Clone)]
pub struct GatewayWebhookHandlerError {
    pub status: StatusCode,
    pub message: String,
}

impl GatewayWebhookHandlerError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait GatewayWebhookHandler: Send + Sync {
    async fn handle_event(
        &self,
        request: GatewayWebhookRequest,
    ) -> Result<GatewayWebhookResponse, GatewayWebhookHandlerError>;

    async fn handle_agent(
        &self,
        request: GatewayWebhookAgentRequest,
    ) -> Result<GatewayWebhookAgentResponse, GatewayWebhookHandlerError>;
}

#[derive(Debug, Deserialize)]
pub(crate) struct GatewayWebhookPayload {
    pub(crate) source: String,
    pub(crate) event_type: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) base_session_key: Option<String>,
    #[serde(default)]
    pub(crate) session_key: Option<String>,
    #[serde(default)]
    pub(crate) chat_id: Option<String>,
    #[serde(default)]
    pub(crate) sender_id: Option<String>,
    #[serde(default)]
    pub(crate) payload: Option<Value>,
    #[serde(default)]
    pub(crate) metadata: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GatewayWebhookAgentQuery {
    pub(crate) hook_id: String,
    #[serde(default)]
    pub(crate) base_session_key: Option<String>,
    #[serde(default)]
    pub(crate) session_key: Option<String>,
    #[serde(default)]
    pub(crate) chat_id: Option<String>,
    #[serde(default)]
    pub(crate) sender_id: Option<String>,
    #[serde(default)]
    pub(crate) provider: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
}

pub(crate) fn build_webhook_state(
    config: &GatewayConfig,
    handler: Option<Arc<dyn GatewayWebhookHandler>>,
) -> Result<Option<GatewayWebhookState>, GatewayError> {
    if !config.webhook.enabled {
        return Ok(None);
    }

    let handler = handler.ok_or(GatewayError::MissingWebhookHandler)?;
    let auth = if config.auth.enabled {
        crate::auth::WebhookAuth::enabled(config.auth.resolve_token())
    } else {
        crate::auth::WebhookAuth::disabled()
    };
    Ok(Some(GatewayWebhookState { handler, auth }))
}

pub(crate) async fn webhook_handler(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(webhook) = state.webhook.as_ref() else {
        return (StatusCode::NOT_FOUND, "webhook is not enabled").into_response();
    };

    let validation = match webhook.auth.validate(&headers, &body) {
        Ok(result) => result,
        Err(err) => return (StatusCode::UNAUTHORIZED, err.message().to_string()).into_response(),
    };

    let payload: GatewayWebhookPayload = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid webhook payload").into_response(),
    };
    let request = match normalize_webhook_request(payload, None) {
        Ok(request) => request,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    debug!(
        webhook_kind = "events",
        event_id = request.event_id.as_str(),
        source = request.source.as_str(),
        event_type = request.event_type.as_str(),
        session_key = request.session_key.as_str(),
        remote_addr = request.remote_addr.as_deref().unwrap_or("unknown"),
        body_bytes = body.len(),
        auth_mode = validation.mode,
        "webhook event request received"
    );
    match webhook.handler.handle_event(request).await {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(err) => (err.status, err.message).into_response(),
    }
}

pub(crate) async fn webhook_agents_handler(
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<GatewayWebhookAgentQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(webhook) = state.webhook.as_ref() else {
        return (StatusCode::NOT_FOUND, "webhook is not enabled").into_response();
    };

    let validation = match webhook.auth.validate(&headers, &body) {
        Ok(result) => result,
        Err(err) => return (StatusCode::UNAUTHORIZED, err.message().to_string()).into_response(),
    };

    let raw_body: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "invalid webhook agent payload").into_response();
        }
    };
    let request = match normalize_webhook_agent_request(query, raw_body, None) {
        Ok(request) => request,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    debug!(
        webhook_kind = "agents",
        request_id = request.request_id.as_str(),
        hook_id = request.hook_id.as_str(),
        session_key = request.session_key.as_str(),
        provider = request.provider.as_deref().unwrap_or("default"),
        model = request.model.as_deref().unwrap_or("default"),
        remote_addr = request.remote_addr.as_deref().unwrap_or("unknown"),
        body_bytes = body.len(),
        auth_mode = validation.mode,
        "webhook agent request received"
    );
    match webhook.handler.handle_agent(request).await {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(err) => (err.status, err.message).into_response(),
    }
}

pub(crate) fn normalize_webhook_request(
    payload: GatewayWebhookPayload,
    remote_addr: Option<SocketAddr>,
) -> Result<GatewayWebhookRequest, &'static str> {
    let source = payload.source.trim();
    let event_type = payload.event_type.trim();
    let content = payload.content.trim();
    if source.is_empty() {
        return Err("source is required");
    }
    if event_type.is_empty() {
        return Err("event_type is required");
    }
    if content.is_empty() {
        return Err("content is required");
    }

    let event_id = uuid::Uuid::new_v4().to_string();
    let session_key = format!("webhook:{source}:{}", uuid::Uuid::new_v4());
    let base_session_key = payload
        .base_session_key
        .as_deref()
        .or(payload.session_key.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let chat_id = payload
        .chat_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| session_key.clone());
    let sender_id = payload
        .sender_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{source}:webhook"));
    let received_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let mut metadata = payload.metadata.unwrap_or_default();
    metadata.insert(
        "trigger.kind".to_string(),
        Value::String("webhook".to_string()),
    );
    metadata.insert(
        "webhook.source".to_string(),
        Value::String(source.to_string()),
    );
    metadata.insert(
        "webhook.event_type".to_string(),
        Value::String(event_type.to_string()),
    );
    metadata.insert(
        "webhook.event_id".to_string(),
        Value::String(event_id.clone()),
    );
    if let Some(base_session_key) = base_session_key.as_ref() {
        metadata.insert(
            "webhook.base_session_key".to_string(),
            Value::String(base_session_key.clone()),
        );
    }

    Ok(GatewayWebhookRequest {
        event_id,
        source: source.to_string(),
        event_type: event_type.to_string(),
        content: content.to_string(),
        session_key,
        base_session_key,
        chat_id,
        sender_id,
        payload: payload.payload,
        metadata,
        remote_addr: remote_addr.map(|addr| addr.to_string()),
        received_at_ms,
    })
}

pub(crate) fn normalize_webhook_agent_request(
    query: GatewayWebhookAgentQuery,
    body: Value,
    remote_addr: Option<SocketAddr>,
) -> Result<GatewayWebhookAgentRequest, &'static str> {
    let hook_id = query.hook_id.trim();
    if hook_id.is_empty() {
        return Err("hook_id is required");
    }
    if !hook_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("hook_id may only contain letters, numbers, '-' and '_'");
    }

    let base_session_key = query
        .base_session_key
        .as_deref()
        .or(query.session_key.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let session_key = format!("webhook:{hook_id}:{}", uuid::Uuid::new_v4());

    let chat_id = query
        .chat_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| session_key.clone());
    let sender_id = query
        .sender_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("webhook-agent:{hook_id}"));
    let provider = query
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let model = query
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let request_id = uuid::Uuid::new_v4().to_string();
    let received_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let mut metadata = BTreeMap::new();
    metadata.insert(
        "trigger.kind".to_string(),
        Value::String("webhook_agents".to_string()),
    );
    metadata.insert(
        "webhook.kind".to_string(),
        Value::String("agents".to_string()),
    );
    metadata.insert(
        "webhook.agents.hook_id".to_string(),
        Value::String(hook_id.to_string()),
    );
    metadata.insert(
        "webhook.agents.request_id".to_string(),
        Value::String(request_id.clone()),
    );
    if let Some(base_session_key) = base_session_key.as_ref() {
        metadata.insert(
            "webhook.base_session_key".to_string(),
            Value::String(base_session_key.clone()),
        );
    }

    Ok(GatewayWebhookAgentRequest {
        request_id,
        hook_id: hook_id.to_string(),
        session_key,
        base_session_key,
        chat_id,
        sender_id,
        provider,
        model,
        body,
        metadata,
        remote_addr: remote_addr.map(|addr| addr.to_string()),
        received_at_ms,
    })
}
