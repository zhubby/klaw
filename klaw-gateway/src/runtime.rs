use crate::{
    GatewayError,
    auth::GatewayAuth,
    chat_page::{chat_page_handler, chat_pkg_js_handler, chat_pkg_wasm_handler},
    handlers::{health_live_handler, health_ready_handler, health_status_handler, metrics_handler},
    home::{home_logo_handler, home_page_handler},
    routes::{
        CHAT_PATH, CHAT_PKG_JS_PATH, CHAT_PKG_WASM_PATH, HOME_LOGO_PATH, HOME_PATH,
        WEBHOOK_AGENTS_PATH, WEBHOOK_EVENTS_PATH, WS_CHAT_PATH,
    },
    state::{GatewayHandle, GatewayRuntimeInfo, GatewayState},
    tailscale::{TailscaleError, TailscaleManager},
    webhook::{
        GatewayWebhookHandler, build_webhook_state, webhook_agents_handler, webhook_handler,
    },
    websocket::ws_chat_handler,
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use klaw_config::{GatewayConfig, TailscaleMode};
use klaw_observability::{HealthRegistry, exporter::PrometheusExporter};
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
    let auth_token = config
        .auth
        .enabled
        .then(|| config.auth.resolve_token())
        .flatten();
    let state = Arc::new(GatewayState::new(health, options.prometheus, webhook));
    let app = build_router(config, state, auth_token);

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(GatewayError::Bind)?;
    let actual_addr = listener.local_addr().map_err(GatewayError::Bind)?;
    let tailscale_info = setup_tailscale(config, actual_addr.port())?;
    let tailscale_manager = create_tailscale_manager(config, actual_addr.port());
    let info = GatewayRuntimeInfo::from_socket_addr(config, actual_addr, tailscale_info);

    info!(
        listen_addr = %actual_addr,
        configured_port = config.listen_port,
        actual_port = info.actual_port,
        "gateway server started"
    );
    println!("{:<18} {}", "🌐 Gateway", info.ws_url);
    if let Some(ref ts) = info.tailscale {
        if let Some(ref url) = ts.public_url {
            println!("{:<18} {}", "🌐 Tailscale", url);
        }
    }
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

    Ok(GatewayHandle::new(
        info,
        shutdown_tx,
        task,
        tailscale_manager.map(Box::new),
    ))
}

fn setup_tailscale(
    config: &GatewayConfig,
    actual_port: u16,
) -> Result<Option<crate::tailscale::TailscaleRuntimeInfo>, GatewayError> {
    if config.tailscale.mode == TailscaleMode::Off {
        return Ok(None);
    }

    let manager = TailscaleManager::new(
        config.tailscale.mode,
        actual_port,
        config.tailscale.reset_on_exit,
    );

    let info = manager.setup().map_err(|e| match e {
        TailscaleError::CliNotFound => GatewayError::TailscaleCliNotFound,
        TailscaleError::NotLoggedIn => GatewayError::TailscaleNotLoggedIn,
        TailscaleError::HttpsNotEnabled => GatewayError::TailscaleHttpsNotEnabled,
        other => GatewayError::TailscaleSetupFailed(other.to_string()),
    })?;

    Ok(Some(info))
}

fn create_tailscale_manager(config: &GatewayConfig, actual_port: u16) -> Option<TailscaleManager> {
    if config.tailscale.mode == TailscaleMode::Off {
        return None;
    }

    Some(TailscaleManager::new(
        config.tailscale.mode,
        actual_port,
        config.tailscale.reset_on_exit,
    ))
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

fn build_router(
    config: &GatewayConfig,
    state: Arc<GatewayState>,
    auth_token: Option<String>,
) -> Router {
    let mut app = Router::new()
        .route(HOME_PATH, get(home_page_handler))
        .route(HOME_LOGO_PATH, get(home_logo_handler))
        .route(CHAT_PATH, get(chat_page_handler))
        .route(CHAT_PKG_JS_PATH, get(chat_pkg_js_handler))
        .route(CHAT_PKG_WASM_PATH, get(chat_pkg_wasm_handler))
        .route(WS_CHAT_PATH, get(ws_chat_handler))
        .route("/health/live", get(health_live_handler))
        .route("/health/ready", get(health_ready_handler))
        .route("/health/status", get(health_status_handler))
        .route("/metrics", get(metrics_handler));

    if config.webhook.enabled {
        if config.webhook.events.enabled {
            let events_router = Router::new()
                .route(WEBHOOK_EVENTS_PATH, post(webhook_handler))
                .layer(DefaultBodyLimit::max(config.webhook.events.max_body_bytes));
            app = app.merge(events_router);
        }
        if config.webhook.agents.enabled {
            let agents_router = Router::new()
                .route(WEBHOOK_AGENTS_PATH, post(webhook_agents_handler))
                .layer(DefaultBodyLimit::max(config.webhook.agents.max_body_bytes));
            app = app.merge(agents_router);
        }
    }

    let app = app.with_state(state);

    if let Some(token) = auth_token {
        app.layer(middleware::from_fn_with_state(
            GatewayAuth::new(token),
            GatewayAuth::middleware,
        ))
    } else {
        app
    }
}
