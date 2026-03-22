use crate::tailscale::{TailscaleManager, TailscaleRuntimeInfo};
use crate::{webhook::GatewayWebhookHandler, GatewayError};
use klaw_config::GatewayConfig;
use klaw_observability::{exporter::PrometheusExporter, HealthRegistry};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{broadcast, oneshot, RwLock},
    task::JoinHandle,
};

pub(crate) const ROOM_BUFFER_SIZE: usize = 256;

pub(crate) struct GatewayWebhookState {
    pub(crate) handler: Arc<dyn GatewayWebhookHandler>,
}

pub(crate) struct GatewayState {
    pub(crate) rooms: RwLock<HashMap<String, broadcast::Sender<String>>>,
    pub(crate) health: Arc<HealthRegistry>,
    pub(crate) prometheus: Option<PrometheusExporter>,
    pub(crate) webhook: Option<GatewayWebhookState>,
}

impl GatewayState {
    pub(crate) fn new(
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRuntimeInfo {
    pub listen_ip: String,
    pub configured_port: u16,
    pub actual_port: u16,
    pub ws_url: String,
    pub health_url: String,
    pub metrics_url: String,
    pub started_at_unix_seconds: u64,
    pub tailscale: Option<TailscaleRuntimeInfo>,
    pub auth_configured: bool,
}

impl GatewayRuntimeInfo {
    pub(crate) fn from_socket_addr(
        config: &GatewayConfig,
        socket_addr: SocketAddr,
        tailscale: Option<TailscaleRuntimeInfo>,
    ) -> Self {
        let base = format!("http://{socket_addr}");
        let started_at_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let auth_configured = config.auth.is_enabled();
        Self {
            listen_ip: config.listen_ip.clone(),
            configured_port: config.listen_port,
            actual_port: socket_addr.port(),
            ws_url: format!("{base}/ws/chat"),
            health_url: format!("{base}/health/status"),
            metrics_url: format!("{base}/metrics"),
            started_at_unix_seconds,
            tailscale,
            auth_configured,
        }
    }
}

pub struct GatewayHandle {
    info: GatewayRuntimeInfo,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), GatewayError>>>,
    _tailscale_manager: Option<Box<TailscaleManager>>,
}

impl GatewayHandle {
    pub(crate) fn new(
        info: GatewayRuntimeInfo,
        shutdown_tx: oneshot::Sender<()>,
        task: JoinHandle<Result<(), GatewayError>>,
        tailscale_manager: Option<Box<TailscaleManager>>,
    ) -> Self {
        Self {
            info,
            shutdown_tx: Some(shutdown_tx),
            task: Some(task),
            _tailscale_manager: tailscale_manager,
        }
    }

    pub fn info(&self) -> &GatewayRuntimeInfo {
        &self.info
    }

    pub async fn wait(mut self) -> Result<(), GatewayError> {
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.await
            .map_err(|err| GatewayError::Join(err.to_string()))?
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
