use crate::{
    handlers::{health_live_handler, health_ready_handler, health_status_handler, metrics_handler},
    state::{GatewayHandle, GatewayRuntimeInfo, GatewayState},
    webhook::{build_webhook_state, webhook_handler, GatewayWebhookHandler},
    websocket::ws_chat_handler,
    GatewayError,
};
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use klaw_config::GatewayConfig;
use klaw_observability::{exporter::PrometheusExporter, HealthRegistry};
use std::{net::SocketAddr, sync::Arc};
use tracing::info;

pub struct GatewayOptions {
    pub health: Option<Arc<HealthRegistry>>,
    pub prometheus: Option<PrometheusExporter>,
    pub webhook_handler: Option<Arc<dyn GatewayWebhookHandler>>,
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

    let socket_addr = parse_socket_addr(config)?;
    let health = build_health_registry(options.health);
    let webhook = build_webhook_state(config, options.webhook_handler)?;
    let state = Arc::new(GatewayState::new(health, options.prometheus, webhook));
    let app = build_router(config, state);

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

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(GatewayError::Serve)
    });

    Ok(GatewayHandle::new(info, shutdown_tx, task))
}

fn parse_socket_addr(config: &GatewayConfig) -> Result<SocketAddr, GatewayError> {
    format!("{}:{}", config.listen_ip, config.listen_port)
        .parse()
        .map_err(|err| {
            GatewayError::InvalidListenAddress(config.listen_ip.clone(), config.listen_port, err)
        })
}

fn build_health_registry(health: Option<Arc<HealthRegistry>>) -> Arc<HealthRegistry> {
    health.unwrap_or_else(|| {
        let registry = HealthRegistry::new();
        registry.register("gateway");
        Arc::new(registry)
    })
}

fn build_router(config: &GatewayConfig, state: Arc<GatewayState>) -> Router {
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

    app.with_state(state)
}
