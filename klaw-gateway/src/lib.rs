use async_trait::async_trait;
use axum::{
    body::{Body, Bytes},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use klaw_config::GatewayConfig;
use klaw_observability::{exporter::PrometheusExporter, HealthRegistry};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::{
    sync::{broadcast, oneshot, RwLock},
    task::JoinHandle,
};
use tracing::info;

const ROOM_BUFFER_SIZE: usize = 256;

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("invalid listen address '{0}:{1}': {2}")]
    InvalidListenAddress(String, u16, std::net::AddrParseError),
    #[error("TLS listener is not implemented yet; set gateway.tls.enabled=false")]
    TlsNotImplemented,
    #[error("failed to bind gateway listener: {0}")]
    Bind(#[source] std::io::Error),
    #[error("gateway server failed: {0}")]
    Serve(#[source] std::io::Error),
    #[error("gateway server task failed: {0}")]
    Join(String),
    #[error("failed to create prometheus exporter: {0}")]
    PrometheusExporter(String),
    #[error("gateway webhook token could not be resolved")]
    MissingWebhookToken,
    #[error("gateway webhook handler is required when gateway.webhook.enabled=true")]
    MissingWebhookHandler,
}

struct GatewayWebhookState {
    token: String,
    handler: Arc<dyn GatewayWebhookHandler>,
}

struct GatewayState {
    rooms: RwLock<HashMap<String, broadcast::Sender<String>>>,
    health: Arc<HealthRegistry>,
    prometheus: Option<PrometheusExporter>,
    webhook: Option<GatewayWebhookState>,
}

impl GatewayState {
    fn new(
        health: Arc<HealthRegistry>,
        prometheus: Option<PrometheusExporter>,
        webhook: Option<GatewayWebhookState>,
    ) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            health,
            prometheus,
            webhook,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatQuery {
    session_key: Option<String>,
}

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
struct GatewayWebhookPayload {
    source: String,
    event_type: String,
    content: String,
    #[serde(default)]
    session_key: Option<String>,
    #[serde(default)]
    chat_id: Option<String>,
    #[serde(default)]
    sender_id: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
    #[serde(default)]
    metadata: Option<BTreeMap<String, Value>>,
}

pub struct GatewayOptions {
    pub health: Option<Arc<HealthRegistry>>,
    pub prometheus: Option<PrometheusExporter>,
    pub webhook_handler: Option<Arc<dyn GatewayWebhookHandler>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRuntimeInfo {
    pub listen_ip: String,
    pub configured_port: u16,
    pub actual_port: u16,
    pub ws_url: String,
    pub health_url: String,
    pub metrics_url: String,
    pub started_at_unix_seconds: u64,
}

impl GatewayRuntimeInfo {
    fn from_socket_addr(config: &GatewayConfig, socket_addr: SocketAddr) -> Self {
        let base = format!("http://{socket_addr}");
        let started_at_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            listen_ip: config.listen_ip.clone(),
            configured_port: config.listen_port,
            actual_port: socket_addr.port(),
            ws_url: format!("{base}/ws/chat"),
            health_url: format!("{base}/health/status"),
            metrics_url: format!("{base}/metrics"),
            started_at_unix_seconds,
        }
    }
}

#[derive(Debug)]
pub struct GatewayHandle {
    info: GatewayRuntimeInfo,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), GatewayError>>>,
}

impl GatewayHandle {
    pub fn info(&self) -> &GatewayRuntimeInfo {
        &self.info
    }

    pub async fn wait(mut self) -> Result<(), GatewayError> {
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.await.map_err(|err| GatewayError::Join(err.to_string()))?
    }

    pub async fn shutdown(mut self) -> Result<(), GatewayError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.wait().await
    }
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Default for GatewayOptions {
    fn default() -> Self {
        Self {
            health: None,
            prometheus: None,
            webhook_handler: None,
        }
    }
}

pub async fn run_gateway(config: &GatewayConfig) -> Result<(), GatewayError> {
    run_gateway_with_options(config, GatewayOptions::default()).await
}

pub async fn spawn_gateway(config: &GatewayConfig) -> Result<GatewayHandle, GatewayError> {
    spawn_gateway_with_options(config, GatewayOptions::default()).await
}

pub async fn run_gateway_with_options(
    config: &GatewayConfig,
    options: GatewayOptions,
) -> Result<(), GatewayError> {
    let handle = spawn_gateway_with_options(config, options).await?;
    handle.wait().await
}

pub async fn spawn_gateway_with_options(
    config: &GatewayConfig,
    options: GatewayOptions,
) -> Result<GatewayHandle, GatewayError> {
    if config.tls.enabled {
        return Err(GatewayError::TlsNotImplemented);
    }

    let socket_addr: SocketAddr = format!("{}:{}", config.listen_ip, config.listen_port)
        .parse()
        .map_err(|err| {
            GatewayError::InvalidListenAddress(config.listen_ip.clone(), config.listen_port, err)
        })?;

    let health = options.health.unwrap_or_else(|| {
        let registry = HealthRegistry::new();
        registry.register("gateway");
        Arc::new(registry)
    });

    let webhook = if config.webhook.enabled {
        let token = resolve_webhook_token(config).ok_or(GatewayError::MissingWebhookToken)?;
        let handler = options
            .webhook_handler
            .ok_or(GatewayError::MissingWebhookHandler)?;
        Some(GatewayWebhookState { token, handler })
    } else {
        None
    };
    let state = Arc::new(GatewayState::new(health, options.prometheus, webhook));

    let mut app = Router::new()
        .route("/ws/chat", get(ws_chat_handler))
        .route("/health/live", get(health_live_handler))
        .route("/health/ready", get(health_ready_handler))
        .route("/health/status", get(health_status_handler))
        .route("/metrics", get(metrics_handler));
    if config.webhook.enabled {
        let webhook_router = Router::new()
            .route(&config.webhook.path, post(webhook_handler))
            .layer(DefaultBodyLimit::max(config.webhook.max_body_bytes));
        app = app.merge(webhook_router);
    }
    let app = app.with_state(state);

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(GatewayError::Bind)?;
    let actual_addr = listener.local_addr().map_err(GatewayError::Bind)?;
    let info = GatewayRuntimeInfo::from_socket_addr(config, actual_addr);
    info!(
        listen_addr = %actual_addr,
        configured_port = config.listen_port,
        actual_port = info.actual_port,
        "gateway server started"
    );
    println!("{:<18} {}", "🌐 Gateway", info.ws_url);
    println!("{:<18} {}", "💚 Health", info.health_url);
    println!("{:<18} {}", "📊 Metrics", info.metrics_url);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(GatewayError::Serve)
    });

    Ok(GatewayHandle {
        info,
        shutdown_tx: Some(shutdown_tx),
        task: Some(task),
    })
}

async fn ws_chat_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<GatewayState>>,
    Query(query): Query<ChatQuery>,
) -> Response {
    let Some(session_key) = query
        .session_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return (StatusCode::BAD_REQUEST, "missing non-empty `session_key`").into_response();
    };

    ws.on_upgrade(move |socket| handle_socket(state, session_key, socket))
}

async fn webhook_handler(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(webhook) = state.webhook.as_ref() else {
        return (StatusCode::NOT_FOUND, "webhook is not enabled").into_response();
    };
    if !is_authorized(&headers, &webhook.token) {
        return (StatusCode::UNAUTHORIZED, "invalid webhook token").into_response();
    }

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

async fn handle_socket(state: Arc<GatewayState>, session_key: String, socket: WebSocket) {
    let tx = room_sender(&state, &session_key).await;
    let mut rx = tx.subscribe();
    let (mut ws_sink, mut ws_stream) = socket.split();
    let send_key = session_key.clone();
    let send_state = Arc::clone(&state);

    let send_task = tokio::spawn(async move {
        while let Ok(message) = rx.recv().await {
            if ws_sink.send(Message::Text(message.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = ws_stream.next().await {
        match message {
            Message::Text(text) => {
                let _ = tx.send(text.to_string());
            }
            Message::Binary(bytes) => {
                let payload = String::from_utf8_lossy(&bytes).to_string();
                let _ = tx.send(payload);
            }
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => {}
        }
    }

    send_task.abort();
    cleanup_room(send_state, send_key).await;
}

async fn room_sender(state: &GatewayState, session_key: &str) -> broadcast::Sender<String> {
    if let Some(sender) = state.rooms.read().await.get(session_key).cloned() {
        return sender;
    }

    let mut rooms = state.rooms.write().await;
    rooms
        .entry(session_key.to_string())
        .or_insert_with(|| {
            let (sender, _) = broadcast::channel(ROOM_BUFFER_SIZE);
            sender
        })
        .clone()
}

async fn cleanup_room(state: Arc<GatewayState>, session_key: String) {
    let mut rooms = state.rooms.write().await;
    let Some(sender) = rooms.get(&session_key) else {
        return;
    };
    if sender.receiver_count() == 0 {
        rooms.remove(&session_key);
    }
}

async fn health_live_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let status = state.health.liveness();
    let code = if status.is_healthy() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, format!("{}\n", status.as_str())).into_response()
}

async fn health_ready_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let status = state.health.readiness();
    let code = if status.is_healthy() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, format!("{}\n", status.as_str())).into_response()
}

async fn health_status_handler(State(state): State<Arc<GatewayState>>) -> Response {
    let status = state.health.overall_status();
    let components: Vec<serde_json::Value> = state
        .health
        .all_components()
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "status": c.status.as_str(),
                "message": c.message,
            })
        })
        .collect();
    let body = serde_json::json!({
        "status": status.as_str(),
        "components": components,
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap_or_default()))
        .unwrap()
}

async fn metrics_handler(State(state): State<Arc<GatewayState>>) -> Response {
    match &state.prometheus {
        Some(exporter) => match exporter.render_metrics() {
            Ok(body) => Response::builder()
                .status(StatusCode::OK)
                .header(
                    header::CONTENT_TYPE,
                    "text/plain; version=0.0.4; charset=utf-8",
                )
                .body(Body::from(body))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("failed to build response"))
                        .unwrap()
                }),
            Err(err) => {
                tracing::warn!(error = %err, "failed to render prometheus metrics");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!("failed to render metrics: {}", err)))
                    .unwrap()
            }
        },
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Prometheus metrics not enabled\n"))
            .unwrap(),
    }
}

fn resolve_webhook_token(config: &GatewayConfig) -> Option<String> {
    config
        .webhook
        .token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            config
                .webhook
                .env_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|env_key| std::env::var(env_key).ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn is_authorized(headers: &HeaderMap, expected_token: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected_token)
}

fn normalize_webhook_request(
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
    metadata.insert("trigger.kind".to_string(), Value::String("webhook".to_string()));
    metadata.insert("webhook.source".to_string(), Value::String(source.to_string()));
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

#[cfg(test)]
mod tests {
    use super::{
        is_authorized, normalize_webhook_request, spawn_gateway, GatewayConfig,
        GatewayWebhookPayload,
    };
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    #[tokio::test]
    async fn spawn_gateway_uses_actual_random_port() {
        let config = GatewayConfig {
            enabled: true,
            listen_ip: "127.0.0.1".to_string(),
            listen_port: 0,
            tls: Default::default(),
            webhook: Default::default(),
        };

        let handle = spawn_gateway(&config).await.expect("gateway should start");
        assert!(handle.info().actual_port > 0);
        assert!(handle.info().ws_url.contains(&handle.info().actual_port.to_string()));

        handle.shutdown().await.expect("gateway should stop");
    }

    #[test]
    fn webhook_authorization_accepts_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token"),
        );
        assert!(is_authorized(&headers, "secret-token"));
        assert!(!is_authorized(&headers, "wrong-token"));
    }

    #[test]
    fn normalize_webhook_request_applies_defaults() {
        let request = normalize_webhook_request(
            GatewayWebhookPayload {
                source: "github".to_string(),
                event_type: "issue_comment.created".to_string(),
                content: "New comment".to_string(),
                session_key: None,
                chat_id: None,
                sender_id: None,
                payload: Some(json!({"action":"created"})),
                metadata: None,
            },
            None,
        )
        .expect("payload should normalize");

        assert!(request.session_key.starts_with("webhook:github:"));
        assert_eq!(request.chat_id, request.session_key);
        assert_eq!(request.sender_id, "github:webhook");
        assert_eq!(
            request.metadata.get("trigger.kind"),
            Some(&json!("webhook"))
        );
    }
}
