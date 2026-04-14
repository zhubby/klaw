use crate::{
    GatewayError,
    webhook::GatewayWebhookHandler,
    websocket::{GatewayWebsocketHandler, GatewayWebsocketServerFrame},
};
use crate::{
    auth::WebhookAuth,
    tailscale::{TailscaleManager, TailscaleRuntimeInfo},
};
use klaw_archive::ArchiveService;
use klaw_config::{GatewayConfig, ModelProviderConfig};
use klaw_observability::{HealthRegistry, exporter::PrometheusExporter};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    net::SocketAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{RwLock, mpsc, oneshot},
    task::JoinHandle,
};

pub(crate) struct GatewayWebhookState {
    pub(crate) handler: Arc<dyn GatewayWebhookHandler>,
    pub(crate) auth: WebhookAuth,
}

pub(crate) struct GatewayWebsocketState {
    pub(crate) handler: Arc<dyn GatewayWebsocketHandler>,
}

pub struct GatewayArchiveState {
    pub service: Arc<dyn ArchiveService>,
}

pub struct GatewayProvidersState {
    pub providers: BTreeMap<String, ModelProviderConfig>,
    pub default_provider: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GatewayWebsocketConnection {
    pub(crate) current_session_key: Option<String>,
    pub(crate) subscribed_session_keys: BTreeSet<String>,
    pub(crate) frame_tx: Option<mpsc::UnboundedSender<GatewayWebsocketServerFrame>>,
}

#[derive(Debug, Default)]
pub struct GatewayWebsocketBroadcaster {
    connections: RwLock<HashMap<String, GatewayWebsocketConnection>>,
}

impl GatewayWebsocketBroadcaster {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(
        &self,
        connection_id: String,
        session_key: Option<String>,
        frame_tx: mpsc::UnboundedSender<GatewayWebsocketServerFrame>,
    ) {
        let subscribed_session_keys = session_key.iter().cloned().collect();
        self.connections.write().await.insert(
            connection_id,
            GatewayWebsocketConnection {
                current_session_key: session_key,
                subscribed_session_keys,
                frame_tx: Some(frame_tx),
            },
        );
    }

    pub async fn track_session_key(&self, connection_id: &str, session_key: String) {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(connection_id) {
            connection.current_session_key = Some(session_key.clone());
            connection.subscribed_session_keys.insert(session_key);
        }
    }

    pub async fn clear_session_keys(&self, connection_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(connection_id) {
            connection.current_session_key = None;
            connection.subscribed_session_keys.clear();
        }
    }

    pub async fn cleanup(&self, connection_id: &str) {
        self.connections.write().await.remove(connection_id);
    }

    pub async fn broadcast_to_session(
        &self,
        session_key: &str,
        frame: GatewayWebsocketServerFrame,
    ) -> usize {
        let mut stale_connection_ids = Vec::new();
        let mut delivered = 0usize;
        {
            let connections = self.connections.read().await;
            for (connection_id, connection) in connections.iter() {
                if !connection.subscribed_session_keys.contains(session_key) {
                    continue;
                }
                let Some(frame_tx) = connection.frame_tx.as_ref() else {
                    stale_connection_ids.push(connection_id.clone());
                    continue;
                };
                if frame_tx.send(frame.clone()).is_ok() {
                    delivered += 1;
                } else {
                    stale_connection_ids.push(connection_id.clone());
                }
            }
        }

        if !stale_connection_ids.is_empty() {
            let mut connections = self.connections.write().await;
            for connection_id in stale_connection_ids {
                connections.remove(&connection_id);
            }
        }

        delivered
    }
}

#[cfg(test)]
mod tests {
    use super::GatewayWebsocketBroadcaster;
    use crate::{GatewayWebsocketServerFrame, OutboundEvent};
    use serde_json::json;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn broadcaster_keeps_all_tracked_sessions_for_connection() {
        let broadcaster = GatewayWebsocketBroadcaster::new();
        let (frame_tx, mut frame_rx) = mpsc::unbounded_channel();
        broadcaster
            .register(
                "conn-1".to_string(),
                Some("websocket:alpha".to_string()),
                frame_tx,
            )
            .await;
        broadcaster
            .track_session_key("conn-1", "websocket:beta".to_string())
            .await;

        let delivered_alpha = broadcaster
            .broadcast_to_session(
                "websocket:alpha",
                GatewayWebsocketServerFrame::Event {
                    event: OutboundEvent::SessionMessage,
                    payload: json!({ "session_key": "websocket:alpha" }),
                },
            )
            .await;
        let delivered_beta = broadcaster
            .broadcast_to_session(
                "websocket:beta",
                GatewayWebsocketServerFrame::Event {
                    event: OutboundEvent::SessionMessage,
                    payload: json!({ "session_key": "websocket:beta" }),
                },
            )
            .await;

        assert_eq!(delivered_alpha, 1);
        assert_eq!(delivered_beta, 1);
        assert!(frame_rx.recv().await.is_some());
        assert!(frame_rx.recv().await.is_some());
    }

    #[tokio::test]
    async fn broadcaster_clear_session_keys_removes_all_subscriptions() {
        let broadcaster = GatewayWebsocketBroadcaster::new();
        let (frame_tx, _frame_rx) = mpsc::unbounded_channel();
        broadcaster
            .register(
                "conn-1".to_string(),
                Some("websocket:alpha".to_string()),
                frame_tx,
            )
            .await;
        broadcaster
            .track_session_key("conn-1", "websocket:beta".to_string())
            .await;

        broadcaster.clear_session_keys("conn-1").await;

        let delivered = broadcaster
            .broadcast_to_session(
                "websocket:alpha",
                GatewayWebsocketServerFrame::Event {
                    event: OutboundEvent::SessionMessage,
                    payload: json!({ "session_key": "websocket:alpha" }),
                },
            )
            .await;
        assert_eq!(delivered, 0);
    }
}

pub(crate) struct GatewayState {
    pub(crate) websocket_broadcaster: Arc<GatewayWebsocketBroadcaster>,
    pub(crate) health: Arc<HealthRegistry>,
    pub(crate) prometheus: Option<PrometheusExporter>,
    pub(crate) webhook: Option<GatewayWebhookState>,
    pub(crate) websocket: Option<GatewayWebsocketState>,
    pub(crate) archive: Option<Arc<GatewayArchiveState>>,
    pub(crate) providers: Option<Arc<GatewayProvidersState>>,
}

impl GatewayState {
    pub(crate) fn new(
        websocket_broadcaster: Arc<GatewayWebsocketBroadcaster>,
        health: Arc<HealthRegistry>,
        prometheus: Option<PrometheusExporter>,
        webhook: Option<GatewayWebhookState>,
        websocket: Option<GatewayWebsocketState>,
        archive: Option<Arc<GatewayArchiveState>>,
        providers: Option<Arc<GatewayProvidersState>>,
    ) -> Self {
        Self {
            websocket_broadcaster,
            health,
            prometheus,
            webhook,
            websocket,
            archive,
            providers,
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
