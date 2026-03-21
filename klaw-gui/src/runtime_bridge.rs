use klaw_channel::ChannelSyncResult;
use klaw_gateway::GatewayRuntimeInfo;
use klaw_util::EnvironmentCheckReport;
use std::sync::{mpsc, Mutex, OnceLock};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Default)]
pub struct GatewayStatusSnapshot {
    pub configured_enabled: bool,
    pub running: bool,
    pub transitioning: bool,
    pub info: Option<GatewayRuntimeInfo>,
    pub last_error: Option<String>,
}

#[derive(Debug)]
pub enum RuntimeCommand {
    ReloadSkillsPrompt,
    SetProviderOverride {
        provider_id: Option<String>,
        response: mpsc::Sender<Result<(String, String), String>>,
    },
    SyncChannels {
        response: mpsc::Sender<Result<ChannelSyncResult, String>>,
    },
    RunCronNow {
        cron_id: String,
        response: mpsc::Sender<Result<String, String>>,
    },
    GetEnvCheck {
        response: mpsc::Sender<EnvironmentCheckReport>,
    },
    GetGatewayStatus {
        response: mpsc::Sender<GatewayStatusSnapshot>,
    },
    SetGatewayEnabled {
        enabled: bool,
        response: mpsc::Sender<Result<GatewayStatusSnapshot, String>>,
    },
    RestartGateway {
        response: mpsc::Sender<Result<GatewayStatusSnapshot, String>>,
    },
}

static RUNTIME_COMMAND_SENDER: OnceLock<Mutex<Option<UnboundedSender<RuntimeCommand>>>> =
    OnceLock::new();
static LOG_RECEIVER: OnceLock<Mutex<Option<mpsc::Receiver<String>>>> = OnceLock::new();

fn sender_slot() -> &'static Mutex<Option<UnboundedSender<RuntimeCommand>>> {
    RUNTIME_COMMAND_SENDER.get_or_init(|| Mutex::new(None))
}

fn log_receiver_slot() -> &'static Mutex<Option<mpsc::Receiver<String>>> {
    LOG_RECEIVER.get_or_init(|| Mutex::new(None))
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
    let mut guard = log_receiver_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(receiver);
}

pub fn clear_log_receiver() {
    let mut guard = log_receiver_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

pub fn drain_log_chunks(max_batch: usize) -> Vec<String> {
    if max_batch == 0 {
        return Vec::new();
    }
    let mut guard = log_receiver_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(receiver) = guard.as_mut() else {
        return Vec::new();
    };

    let mut chunks = Vec::new();
    for _ in 0..max_batch {
        match receiver.try_recv() {
            Ok(chunk) => chunks.push(chunk),
            Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }
    chunks
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

    response_rx
        .recv()
        .map_err(|_| "runtime command response channel closed".to_string())?
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

    response_rx
        .recv()
        .map_err(|_| "runtime command response channel closed".to_string())?
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

    response_rx
        .recv()
        .map_err(|_| "runtime command response channel closed".to_string())?
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

    response_rx
        .recv()
        .map_err(|_| "runtime command response channel closed".to_string())
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

    response_rx
        .recv()
        .map_err(|_| "runtime command response channel closed".to_string())
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

    response_rx
        .recv()
        .map_err(|_| "runtime command response channel closed".to_string())?
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

    response_rx
        .recv()
        .map_err(|_| "runtime command response channel closed".to_string())?
}
