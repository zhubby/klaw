use klaw_acp::{
    AcpPermissionDecision, AcpPermissionRequest, AcpRuntimeSnapshot, AcpSessionEvent, AcpSyncResult,
};
use klaw_channel::{ChannelInstanceKey, ChannelInstanceStatus, ChannelSyncResult};
use klaw_config::TailscaleMode;
use klaw_gateway::TailscaleHostInfo;
use klaw_llm::ToolDefinition;
use klaw_mcp::{McpRuntimeSnapshot, McpServerKey, McpSyncResult};
use klaw_util::EnvironmentCheckReport;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

pub use klaw_runtime::GatewayStatusSnapshot;

#[derive(Debug, Clone, Default)]
pub struct ProviderRuntimeSnapshot {
    pub default_provider_id: String,
    pub provider_default_models: BTreeMap<String, String>,
    pub runtime_provider_override: Option<String>,
    pub active_provider_id: String,
    pub active_model: String,
}

#[derive(Debug, Clone)]
pub enum AcpPromptEvent {
    SessionEvent(AcpSessionEvent),
    PermissionRequested {
        request_id: u64,
        request: AcpPermissionRequest,
    },
    Completed {
        final_output: String,
    },
    Stopped,
    Failed(String),
}

#[derive(Debug)]
pub enum RuntimeCommand {
    ReloadSkillsPrompt,
    SetProviderOverride {
        provider_id: Option<String>,
        response: mpsc::Sender<Result<(String, String), String>>,
    },
    SyncProviders {
        response: mpsc::Sender<Result<ProviderRuntimeSnapshot, String>>,
    },
    GetProviderStatus {
        response: mpsc::Sender<Result<ProviderRuntimeSnapshot, String>>,
    },
    SyncChannels {
        response: mpsc::Sender<Result<ChannelSyncResult, String>>,
    },
    GetChannelStatus {
        response: mpsc::Sender<Result<Vec<ChannelInstanceStatus>, String>>,
    },
    RestartChannel {
        instance_key: String,
        response: mpsc::Sender<Result<ChannelSyncResult, String>>,
    },
    RestartMcpServer {
        server_id: String,
        response: mpsc::Sender<Result<McpRuntimeSnapshot, String>>,
    },
    SyncMcp {
        response: mpsc::Sender<Result<McpSyncResult, String>>,
    },
    SyncAcp {
        response: mpsc::Sender<Result<AcpSyncResult, String>>,
    },
    SyncTools {
        response: mpsc::Sender<Result<Vec<String>, String>>,
    },
    GetToolDefinitions {
        response: mpsc::Sender<Result<Vec<ToolDefinition>, String>>,
    },
    GetMcpStatus {
        response: mpsc::Sender<Result<McpRuntimeSnapshot, String>>,
    },
    GetAcpStatus {
        response: mpsc::Sender<Result<AcpRuntimeSnapshot, String>>,
    },
    ExecuteAcpPromptStream {
        agent_id: String,
        prompt: String,
        working_directory: Option<String>,
        timeout_seconds: Option<u64>,
        events: mpsc::Sender<AcpPromptEvent>,
    },
    StopAcpPrompt {
        response: mpsc::Sender<Result<(), String>>,
    },
    ResolveAcpPermission {
        request_id: u64,
        decision: AcpPermissionDecision,
        response: mpsc::Sender<Result<(), String>>,
    },
    RunCronNow {
        cron_id: String,
        response: mpsc::Sender<Result<String, String>>,
    },
    RunHeartbeatNow {
        heartbeat_id: String,
        response: mpsc::Sender<Result<String, String>>,
    },
    RunMemoryArchiveNow {
        response: mpsc::Sender<Result<String, String>>,
    },
    GetEnvCheck {
        response: mpsc::Sender<EnvironmentCheckReport>,
    },
    GetGatewayStatus {
        response: mpsc::Sender<GatewayStatusSnapshot>,
    },
    GetTailscaleHostStatus {
        response: mpsc::Sender<Result<TailscaleHostInfo, String>>,
    },
    StartGateway {
        response: mpsc::Sender<Result<GatewayStatusSnapshot, String>>,
    },
    SetGatewayEnabled {
        enabled: bool,
        response: mpsc::Sender<Result<GatewayStatusSnapshot, String>>,
    },
    RestartGateway {
        response: mpsc::Sender<Result<GatewayStatusSnapshot, String>>,
    },
    SetTailscaleMode {
        mode: TailscaleMode,
        response: mpsc::Sender<Result<GatewayStatusSnapshot, String>>,
    },
}

static RUNTIME_COMMAND_SENDER: OnceLock<Mutex<Option<UnboundedSender<RuntimeCommand>>>> =
    OnceLock::new();
static LOG_BRIDGE: OnceLock<Mutex<Option<Arc<GuiLogBridge>>>> = OnceLock::new();
const RUNTIME_STATUS_TIMEOUT: Duration = Duration::from_secs(10);
const RUNTIME_ACTION_TIMEOUT: Duration = Duration::from_secs(5);
const RUNTIME_MCP_RESTART_TIMEOUT: Duration = Duration::from_secs(90);
const RUNTIME_MEMORY_ARCHIVE_TIMEOUT: Duration = Duration::from_secs(120);
const LOG_BRIDGE_MAX_CHUNKS: usize = 8_192;

fn sender_slot() -> &'static Mutex<Option<UnboundedSender<RuntimeCommand>>> {
    RUNTIME_COMMAND_SENDER.get_or_init(|| Mutex::new(None))
}

fn log_bridge_slot() -> &'static Mutex<Option<Arc<GuiLogBridge>>> {
    LOG_BRIDGE.get_or_init(|| Mutex::new(None))
}

fn recv_response<T>(
    receiver: mpsc::Receiver<T>,
    timeout: Duration,
    operation: &str,
) -> Result<T, String> {
    receiver.recv_timeout(timeout).map_err(|err| match err {
        mpsc::RecvTimeoutError::Timeout => {
            format!("timed out waiting for {operation} response")
        }
        mpsc::RecvTimeoutError::Disconnected => {
            "runtime command response channel closed".to_string()
        }
    })
}

pub fn install_runtime_command_sender(sender: UnboundedSender<RuntimeCommand>) {
    let mut guard = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(sender);
}

pub fn clear_runtime_command_sender() {
    let mut guard = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

pub fn install_log_receiver(receiver: mpsc::Receiver<String>) {
    let bridge = Arc::new(GuiLogBridge::default());
    {
        let mut guard = log_bridge_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(Arc::clone(&bridge));
    }

    let _ = std::thread::Builder::new()
        .name("klaw-gui-log-pump".to_string())
        .spawn(move || {
            while let Ok(chunk) = receiver.recv() {
                bridge.push_chunk(chunk);
            }
        });
}

pub fn clear_log_receiver() {
    let mut guard = log_bridge_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

pub fn drain_log_chunks(max_batch: usize) -> Vec<String> {
    if max_batch == 0 {
        return Vec::new();
    }
    let bridge = {
        let guard = log_bridge_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().map(Arc::clone)
    };
    let Some(bridge) = bridge else {
        return Vec::new();
    };

    bridge.drain_chunks(max_batch)
}

pub fn record_dropped_log_chunk(len_bytes: usize) {
    let guard = log_bridge_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(bridge) = guard.as_ref() {
        bridge.record_transport_drop(len_bytes);
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GuiLogStatsSnapshot {
    pub transport_dropped_chunks: u64,
    pub transport_dropped_bytes: u64,
    pub bridge_dropped_chunks: u64,
}

pub fn log_stats_snapshot() -> GuiLogStatsSnapshot {
    let bridge = {
        let guard = log_bridge_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().map(Arc::clone)
    };
    bridge.map_or_else(GuiLogStatsSnapshot::default, |bridge| {
        bridge.stats_snapshot()
    })
}

#[derive(Debug)]
pub struct RuntimeRequestHandle<T> {
    receiver: Option<mpsc::Receiver<Result<T, String>>>,
}

impl<T> RuntimeRequestHandle<T> {
    pub fn try_take_result(&mut self) -> Option<Result<T, String>> {
        let receiver = self.receiver.as_ref()?;
        match receiver.try_recv() {
            Ok(result) => {
                let _ = self.receiver.take();
                Some(result)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                let _ = self.receiver.take();
                Some(Err("runtime request worker closed unexpectedly".to_string()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.receiver.is_some()
    }
}

fn spawn_request<T, F>(operation: F) -> RuntimeRequestHandle<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(operation());
    });
    RuntimeRequestHandle { receiver: Some(rx) }
}

#[derive(Debug, Default)]
struct GuiLogBridge {
    chunks: Mutex<VecDeque<String>>,
    transport_dropped_chunks: AtomicU64,
    transport_dropped_bytes: AtomicU64,
    bridge_dropped_chunks: AtomicU64,
}

impl GuiLogBridge {
    fn push_chunk(&self, chunk: String) {
        let mut guard = self
            .chunks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.len() >= LOG_BRIDGE_MAX_CHUNKS {
            guard.pop_front();
            self.bridge_dropped_chunks.fetch_add(1, Ordering::Relaxed);
        }
        guard.push_back(chunk);
    }

    fn drain_chunks(&self, max_batch: usize) -> Vec<String> {
        let mut guard = self
            .chunks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut chunks = Vec::new();
        for _ in 0..max_batch {
            let Some(chunk) = guard.pop_front() else {
                break;
            };
            chunks.push(chunk);
        }
        chunks
    }

    fn record_transport_drop(&self, len_bytes: usize) {
        self.transport_dropped_chunks
            .fetch_add(1, Ordering::Relaxed);
        self.transport_dropped_bytes
            .fetch_add(len_bytes as u64, Ordering::Relaxed);
    }

    fn stats_snapshot(&self) -> GuiLogStatsSnapshot {
        GuiLogStatsSnapshot {
            transport_dropped_chunks: self.transport_dropped_chunks.load(Ordering::Relaxed),
            transport_dropped_bytes: self.transport_dropped_bytes.load(Ordering::Relaxed),
            bridge_dropped_chunks: self.bridge_dropped_chunks.load(Ordering::Relaxed),
        }
    }
}

pub fn request_reload_skills_prompt() -> Result<(), String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    sender
        .send(RuntimeCommand::ReloadSkillsPrompt)
        .map_err(|_| "failed to send runtime command".to_string())
}

pub fn request_set_provider_override(
    provider_id: Option<String>,
) -> Result<(String, String), String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SetProviderOverride {
            provider_id,
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "set provider override")?
}

pub fn begin_set_provider_override_request(
    provider_id: Option<String>,
) -> RuntimeRequestHandle<(String, String)> {
    spawn_request(move || request_set_provider_override(provider_id))
}

pub fn request_run_cron_now(cron_id: &str) -> Result<String, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::RunCronNow {
            cron_id: cron_id.to_string(),
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "run cron")?
}

pub fn begin_run_cron_now_request(cron_id: String) -> RuntimeRequestHandle<String> {
    spawn_request(move || request_run_cron_now(&cron_id))
}

pub fn request_run_memory_archive_now() -> Result<String, String> {
    let (response_tx, response_rx) = mpsc::channel();
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;

    sender
        .send(RuntimeCommand::RunMemoryArchiveNow {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_MEMORY_ARCHIVE_TIMEOUT, "memory archive")?
}

pub fn begin_run_memory_archive_now_request() -> RuntimeRequestHandle<String> {
    spawn_request(move || request_run_memory_archive_now())
}

pub fn request_run_heartbeat_now(heartbeat_id: &str) -> Result<String, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::RunHeartbeatNow {
            heartbeat_id: heartbeat_id.to_string(),
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "run heartbeat")?
}

pub fn request_sync_channels() -> Result<ChannelSyncResult, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SyncChannels {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "sync channels")?
}

pub fn request_channel_status() -> Result<Vec<ChannelInstanceStatus>, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::GetChannelStatus {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_STATUS_TIMEOUT, "channel status")?
}

pub fn begin_channel_status_request() -> RuntimeRequestHandle<Vec<ChannelInstanceStatus>> {
    spawn_request(request_channel_status)
}

pub fn request_restart_channel(instance_key: &str) -> Result<ChannelSyncResult, String> {
    let key = ChannelInstanceKey::parse(instance_key)?;
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::RestartChannel {
            instance_key: key.as_str().to_string(),
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "restart channel")?
}

pub fn begin_restart_channel_request(
    instance_key: String,
) -> RuntimeRequestHandle<ChannelSyncResult> {
    spawn_request(move || request_restart_channel(&instance_key))
}

pub fn request_restart_mcp_server(server_id: &str) -> Result<McpRuntimeSnapshot, String> {
    let server_id = McpServerKey::new(server_id).as_str().to_string();
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::RestartMcpServer {
            server_id,
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(
        response_rx,
        RUNTIME_MCP_RESTART_TIMEOUT,
        "restart mcp server",
    )?
}

pub fn begin_restart_mcp_server_request(
    server_id: String,
) -> RuntimeRequestHandle<McpRuntimeSnapshot> {
    spawn_request(move || request_restart_mcp_server(&server_id))
}

pub fn request_sync_providers() -> Result<ProviderRuntimeSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SyncProviders {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "sync providers")?
}

pub fn begin_sync_providers_request() -> RuntimeRequestHandle<ProviderRuntimeSnapshot> {
    spawn_request(request_sync_providers)
}

pub fn request_provider_status() -> Result<ProviderRuntimeSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::GetProviderStatus {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_STATUS_TIMEOUT, "provider status")?
}

pub fn begin_provider_status_request() -> RuntimeRequestHandle<ProviderRuntimeSnapshot> {
    spawn_request(request_provider_status)
}

pub fn request_env_check() -> Result<EnvironmentCheckReport, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::GetEnvCheck {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_STATUS_TIMEOUT, "environment check")
}

pub fn begin_env_check_request() -> RuntimeRequestHandle<EnvironmentCheckReport> {
    spawn_request(request_env_check)
}

pub fn request_gateway_status() -> Result<GatewayStatusSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::GetGatewayStatus {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_STATUS_TIMEOUT, "gateway status")
}

pub fn request_tailscale_host_status() -> Result<TailscaleHostInfo, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::GetTailscaleHostStatus {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_STATUS_TIMEOUT, "tailscale host status")?
}

pub fn request_start_gateway() -> Result<GatewayStatusSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::StartGateway {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "start gateway")?
}

pub fn request_set_gateway_enabled(enabled: bool) -> Result<GatewayStatusSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SetGatewayEnabled {
            enabled,
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "set gateway enabled")?
}

pub fn request_restart_gateway() -> Result<GatewayStatusSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::RestartGateway {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "restart gateway")?
}

pub fn request_set_tailscale_mode(mode: TailscaleMode) -> Result<GatewayStatusSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SetTailscaleMode {
            mode,
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "set tailscale mode")?
}

pub fn request_sync_mcp() -> Result<McpSyncResult, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SyncMcp {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "sync mcp")?
}

pub fn request_sync_acp() -> Result<AcpSyncResult, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SyncAcp {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "sync acp")?
}

pub fn request_sync_tools() -> Result<Vec<String>, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::SyncTools {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "sync tools")?
}

pub fn request_tool_definitions() -> Result<Vec<ToolDefinition>, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::GetToolDefinitions {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "tool definitions")?
}

pub fn request_mcp_status() -> Result<McpRuntimeSnapshot, String> {
    let sender = match sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
    {
        Some(s) => s,
        None => return Err("runtime command channel is not available".to_string()),
    };
    let (response_tx, response_rx) = mpsc::channel();
    if sender
        .send(RuntimeCommand::GetMcpStatus {
            response: response_tx,
        })
        .is_err()
    {
        return Err("failed to send runtime command".to_string());
    }

    match recv_response(response_rx, RUNTIME_STATUS_TIMEOUT, "mcp status") {
        Ok(result) => result,
        Err(error) => Err(error),
    }
}

pub fn request_acp_status() -> Result<AcpRuntimeSnapshot, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::GetAcpStatus {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;

    match recv_response(response_rx, RUNTIME_STATUS_TIMEOUT, "acp status") {
        Ok(result) => result,
        Err(error) => Err(error),
    }
}

pub fn request_execute_acp_prompt_stream(
    agent_id: &str,
    prompt: &str,
    working_directory: Option<String>,
    timeout_seconds: Option<u64>,
) -> Result<mpsc::Receiver<AcpPromptEvent>, String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (events_tx, events_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::ExecuteAcpPromptStream {
            agent_id: agent_id.to_string(),
            prompt: prompt.to_string(),
            working_directory,
            timeout_seconds,
            events: events_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;
    Ok(events_rx)
}

pub fn request_stop_acp_prompt() -> Result<(), String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::StopAcpPrompt {
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;
    recv_response(response_rx, RUNTIME_ACTION_TIMEOUT, "stop acp prompt")?
}

pub fn request_resolve_acp_permission(
    request_id: u64,
    decision: AcpPermissionDecision,
) -> Result<(), String> {
    let sender = sender_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| "runtime command channel is not available".to_string())?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(RuntimeCommand::ResolveAcpPermission {
            request_id,
            decision,
            response: response_tx,
        })
        .map_err(|_| "failed to send runtime command".to_string())?;
    recv_response(
        response_rx,
        RUNTIME_ACTION_TIMEOUT,
        "resolve acp permission",
    )?
}

#[cfg(test)]
mod tests {
    use super::{
        AcpPromptEvent, RuntimeCommand, RuntimeRequestHandle, clear_runtime_command_sender,
        drain_log_chunks, install_log_receiver, install_runtime_command_sender, log_stats_snapshot,
        record_dropped_log_chunk, request_resolve_acp_permission,
    };
    use klaw_acp::AcpPermissionDecision;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn acp_prompt_event_completed_carries_final_output() {
        let event = AcpPromptEvent::Completed {
            final_output: "done".to_string(),
        };
        match event {
            AcpPromptEvent::Completed { final_output } => assert_eq!(final_output, "done"),
            _ => panic!("expected completed event"),
        }
    }

    #[test]
    fn acp_prompt_event_stopped_is_distinct() {
        assert!(matches!(AcpPromptEvent::Stopped, AcpPromptEvent::Stopped));
    }

    #[test]
    fn request_resolve_acp_permission_sends_runtime_command() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        install_runtime_command_sender(sender);

        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(async move {
                match receiver.recv().await {
                    Some(RuntimeCommand::ResolveAcpPermission {
                        request_id,
                        decision,
                        response,
                    }) => {
                        assert_eq!(request_id, 42);
                        assert!(matches!(decision, AcpPermissionDecision::Cancelled));
                        response.send(Ok(())).expect("send response");
                    }
                    other => panic!("unexpected runtime command: {other:?}"),
                }
            });
        });

        let result = request_resolve_acp_permission(42, AcpPermissionDecision::Cancelled);
        clear_runtime_command_sender();
        worker.join().expect("worker thread joins");

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn runtime_request_handle_reports_pending_then_result() {
        let (tx, rx) = mpsc::channel();
        let mut handle = RuntimeRequestHandle { receiver: Some(rx) };

        assert!(handle.is_pending());
        assert!(handle.try_take_result().is_none());

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            let _ = tx.send(Ok::<_, String>("done".to_string()));
        })
        .join()
        .expect("sender thread joins");

        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(handle.try_take_result(), Some(Ok("done".to_string())));
        assert!(!handle.is_pending());
    }

    #[test]
    fn log_bridge_tracks_transport_drops_and_drains_chunks() {
        let (tx, rx) = mpsc::channel();
        install_log_receiver(rx);

        tx.send("first\n".to_string()).expect("send chunk");
        std::thread::sleep(Duration::from_millis(20));
        let drained = drain_log_chunks(16);
        assert_eq!(drained, vec!["first\n".to_string()]);

        record_dropped_log_chunk(12);
        let stats = log_stats_snapshot();
        assert!(stats.transport_dropped_chunks >= 1);
        assert!(stats.transport_dropped_bytes >= 12);
    }
}
