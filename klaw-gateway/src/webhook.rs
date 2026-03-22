use crate::{
    state::{GatewayState, GatewayWebhookState},
    GatewayError,
};
use async_trait::async_trait;
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayWebhookRequest {
    pub event_id: String,
    pub source: String,
    pub event_type: String,
    pub content: String,
    pub session_key: String,
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

#[async_trait]
pub trait GatewayWebhookHandler: Send + Sync {
    async fn handle(
        &self,
        request: GatewayWebhookRequest,
    ) -> Result<GatewayWebhookResponse, String>;
}

#[derive(Debug, Deserialize)]
pub(crate) struct GatewayWebhookPayload {
    pub(crate) source: String,
    pub(crate) event_type: String,
    pub(crate) content: String,
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

pub(crate) fn build_webhook_state(
    config: &GatewayConfig,
    handler: Option<Arc<dyn GatewayWebhookHandler>>,
) -> Result<Option<GatewayWebhookState>, GatewayError> {
    if !config.webhook.enabled {
        return Ok(None);
    }

    let handler = handler.ok_or(GatewayError::MissingWebhookHandler)?;
    Ok(Some(GatewayWebhookState { handler }))
}

pub(crate) async fn webhook_handler(
    State(state): State<Arc<GatewayState>>,
    body: Bytes,
) -> Response {
    let Some(webhook) = state.webhook.as_ref() else {
        return (StatusCode::NOT_FOUND, "webhook is not enabled").into_response();
    };

    let payload: GatewayWebhookPayload = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid webhook payload").into_response(),
    };
    let request = match normalize_webhook_request(payload, None) {
        Ok(request) => request,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };
    match webhook.handler.handle(request).await {
        Ok(response) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Err(message) => (StatusCode::INTERNAL_SERVER_ERROR, message).into_response(),
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
    let session_key = payload
        .session_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("webhook:{source}:{}", uuid::Uuid::new_v4()));
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

    Ok(GatewayWebhookRequest {
        event_id,
        source: source.to_string(),
        event_type: event_type.to_string(),
        content: content.to_string(),
        session_key,
        chat_id,
        sender_id,
        payload: payload.payload,
        metadata,
        remote_addr: remote_addr.map(|addr| addr.to_string()),
        received_at_ms,
    })
}
