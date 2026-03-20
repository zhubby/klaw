use klaw_channel::ChannelSyncResult;
use std::sync::{mpsc, Mutex, OnceLock};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum RuntimeCommand {
    ReloadSkillsPrompt,
    SyncChannels {
        response: mpsc::Sender<Result<ChannelSyncResult, String>>,
    },
    RunCronNow {
        cron_id: String,
        response: mpsc::Sender<Result<String, String>>,
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
