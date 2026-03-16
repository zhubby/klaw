use std::sync::{mpsc, Mutex, OnceLock};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum RuntimeCommand {
    ReloadSkillsPrompt,
    RunCronNow {
        cron_id: String,
        response: mpsc::Sender<Result<String, String>>,
    },
}

static RUNTIME_COMMAND_SENDER: OnceLock<Mutex<Option<UnboundedSender<RuntimeCommand>>>> =
    OnceLock::new();

fn sender_slot() -> &'static Mutex<Option<UnboundedSender<RuntimeCommand>>> {
    RUNTIME_COMMAND_SENDER.get_or_init(|| Mutex::new(None))
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
