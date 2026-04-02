use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::{
    AcpPromptEvent, request_acp_status, request_execute_acp_prompt_stream,
    request_resolve_acp_permission, request_stop_acp_prompt, request_sync_acp,
};
use crate::widgets::{ArrayEditor, KeyValueEditor};
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_acp::{
    AcpAvailableCommand, AcpConfigOption, AcpContentBlockEvent, AcpPermissionDecision,
    AcpPermissionRequest, AcpRuntimeSnapshot, AcpSessionEvent, AcpSessionEventKind, AcpSyncResult,
};
use klaw_config::{AcpAgentConfig, AppConfig, ConfigError, ConfigSnapshot, ConfigStore};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const ACP_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
struct AcpAgentForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    description: String,
    command: String,
    args_input: ArrayEditor,
    env_input: KeyValueEditor,
}

impl AcpAgentForm {
    fn new(existing_ids: &BTreeSet<String>) -> Self {
        let template = AcpAgentConfig::default();
        Self {
            original_id: None,
            id: if existing_ids.contains(&template.id) {
                String::new()
            } else {
                template.id.clone()
            },
            enabled: template.enabled,
            description: template.description.clone(),
            command: template.command.clone(),
            args_input: ArrayEditor::from_vec("Args", &template.args),
            env_input: KeyValueEditor::from_map("Env", &template.env),
        }
    }

    fn edit(agent: &AcpAgentConfig) -> Self {
        Self {
            original_id: Some(agent.id.clone()),
            id: agent.id.clone(),
            enabled: agent.enabled,
            description: agent.description.clone(),
            command: agent.command.clone(),
            args_input: ArrayEditor::from_vec("Args", &agent.args),
            env_input: KeyValueEditor::from_map("Env", &agent.env),
        }
    }

    fn title(&self) -> &'static str {
        if self.original_id.is_some() {
            "Edit ACP Agent"
        } else {
            "Add ACP Agent"
        }
    }

    fn normalized_id(&self) -> String {
        self.id.trim().to_string()
    }

    fn to_config(&self) -> AcpAgentConfig {
        AcpAgentConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            command: self.command.trim().to_string(),
            args: self.args_input.to_vec(),
            env: self.env_input.to_map(),
            description: self.description.trim().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct AcpRuntimeStatusRow {
    state: String,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct PromptTestState {
    agent_id: String,
    prompt: String,
    working_directory: String,
    timeout_seconds: String,
    output: String,
    session_events: Vec<AcpSessionEvent>,
    available_commands: Vec<AcpAvailableCommand>,
    current_mode_id: Option<String>,
    config_options: Vec<AcpConfigOption>,
    session_title: Option<String>,
    session_updated_at: Option<String>,
    permission_history: Vec<String>,
    pending_permissions: Vec<PendingPermissionState>,
    last_error: Option<String>,
    running: bool,
    stopped: bool,
    window_open: bool,
}

#[derive(Debug, Clone)]
struct PendingPermissionState {
    request_id: u64,
    request: AcpPermissionRequest,
    resolving: bool,
}

#[derive(Debug, Clone)]
struct AcpAgentDetailWindow {
    agent_id: String,
}

#[derive(Default)]
pub struct AcpPanel {
    store: Option<ConfigStore>,
    config: AppConfig,
    form: Option<AcpAgentForm>,
    global_settings_form: Option<String>,
    detail_window: Option<AcpAgentDetailWindow>,
    selected_agent: Option<String>,
    delete_confirm: Option<String>,
    runtime_statuses: BTreeMap<String, AcpRuntimeStatusRow>,
    status_fetch_rx: Option<Receiver<Result<AcpRuntimeSnapshot, String>>>,
    sync_fetch_rx: Option<Receiver<Result<AcpSyncResult, String>>>,
    prompt_fetch_rx: Option<Receiver<AcpPromptEvent>>,
    permission_action_rx: Option<Receiver<(u64, Result<(), String>)>>,
    prompt_test: PromptTestState,
    last_status_refresh_at: Option<Instant>,
    status_refresh_announce: bool,
    status_refresh_manual: bool,
    sync_announce: bool,
}

impl AcpPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                self.schedule_status_refresh(false);
                notifications.success("ACP config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
        self.ensure_prompt_target_agent();
    }

    fn ensure_prompt_target_agent(&mut self) {
        let available = self
            .config
            .acp
            .agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>();
        if available.is_empty() {
            self.prompt_test.agent_id.clear();
            return;
        }
        if available
            .iter()
            .any(|agent_id| *agent_id == self.prompt_test.agent_id)
        {
            return;
        }
        if let Some(selected) = self.selected_agent.as_deref()
            && available.iter().any(|agent_id| *agent_id == selected)
        {
            self.prompt_test.agent_id = selected.to_string();
            return;
        }
        self.prompt_test.agent_id = available[0].to_string();
    }

    fn schedule_status_refresh(&mut self, announce: bool) {
        if self.status_fetch_rx.is_some() {
            self.status_refresh_announce |= announce;
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.status_fetch_rx = Some(rx);
        self.last_status_refresh_at = Some(Instant::now());
        self.status_refresh_announce = announce;
        self.status_refresh_manual = announce;
        thread::spawn(move || {
            let _ = tx.send(request_acp_status());
        });
    }

    fn schedule_manager_sync(&mut self, announce: bool) {
        if self.sync_fetch_rx.is_some() {
            self.sync_announce |= announce;
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.sync_fetch_rx = Some(rx);
        self.sync_announce = announce;
        thread::spawn(move || {
            let _ = tx.send(request_sync_acp());
        });
    }

    fn refresh_status_if_due(&mut self) {
        if self.status_fetch_rx.is_some() {
            return;
        }
        let Some(last_refresh) = self.last_status_refresh_at else {
            self.schedule_status_refresh(false);
            return;
        };
        if last_refresh.elapsed() >= ACP_STATUS_POLL_INTERVAL {
            self.schedule_status_refresh(false);
        }
    }

    fn poll_status_refresh(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.status_fetch_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(snapshot)) => {
                self.runtime_statuses = snapshot
                    .statuses
                    .into_iter()
                    .map(|status| {
                        (
                            status.key.as_str().to_string(),
                            AcpRuntimeStatusRow {
                                state: status.state.as_str().to_string(),
                                last_error: status.last_error,
                            },
                        )
                    })
                    .collect();
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications.success("ACP status refreshed");
                }
                self.status_refresh_announce = false;
                self.status_refresh_manual = false;
            }
            Ok(Err(err)) => {
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications.error(format!("Failed to refresh ACP status: {err}"));
                }
                self.status_refresh_announce = false;
                self.status_refresh_manual = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications
                        .error("Failed to refresh ACP status: background task disconnected");
                }
                self.status_refresh_announce = false;
                self.status_refresh_manual = false;
            }
        }
    }

    fn poll_manager_sync(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.sync_fetch_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(_)) => {
                self.sync_fetch_rx = None;
                self.schedule_status_refresh(false);
                if self.sync_announce {
                    notifications.success("ACP runtime synchronized");
                }
                self.sync_announce = false;
            }
            Ok(Err(err)) => {
                self.sync_fetch_rx = None;
                notifications.error(format!("Failed to sync ACP runtime: {err}"));
                self.sync_announce = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sync_fetch_rx = None;
                notifications.error("Failed to sync ACP runtime: background task disconnected");
                self.sync_announce = false;
            }
        }
    }

    fn apply_session_event(&mut self, event: AcpSessionEvent) {
        self.append_output_for_event(&event);
        match &event.update {
            AcpSessionEventKind::AvailableCommandsUpdate { commands } => {
                self.prompt_test.available_commands = commands.clone();
            }
            AcpSessionEventKind::CurrentModeUpdate { current_mode_id } => {
                self.prompt_test.current_mode_id = Some(current_mode_id.clone());
            }
            AcpSessionEventKind::ConfigOptionUpdate { config_options } => {
                self.prompt_test.config_options = config_options.clone();
            }
            AcpSessionEventKind::SessionInfoUpdate { title, updated_at } => {
                if let Some(title) = title {
                    self.prompt_test.session_title = title.clone();
                }
                if let Some(updated_at) = updated_at {
                    self.prompt_test.session_updated_at = updated_at.clone();
                }
            }
            _ => {}
        }
        self.prompt_test.session_events.push(event);
    }

    fn append_output_for_event(&mut self, event: &AcpSessionEvent) {
        match &event.update {
            AcpSessionEventKind::AgentMessageChunk { content } => {
                self.prompt_test
                    .output
                    .push_str(&render_content_block(content));
            }
            AcpSessionEventKind::AgentThoughtChunk { content } => {
                self.push_output_line(&format!("[thought] {}", render_content_block(content)));
            }
            AcpSessionEventKind::UserMessageChunk { content } => {
                self.push_output_line(&format!("[user] {}", render_content_block(content)));
            }
            _ => self.push_output_line(&format!("[{}]", event.summary)),
        }
    }

    fn push_output_line(&mut self, line: &str) {
        if !self.prompt_test.output.is_empty() && !self.prompt_test.output.ends_with('\n') {
            self.prompt_test.output.push('\n');
        }
        self.prompt_test.output.push_str(line);
        if !self.prompt_test.output.ends_with('\n') {
            self.prompt_test.output.push('\n');
        }
    }

    fn poll_permission_action(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.permission_action_rx.as_ref() else {
            return;
        };
        let mut clear_receiver = false;
        loop {
            match rx.try_recv() {
                Ok((request_id, Ok(()))) => {
                    if let Some(permission) = pending_permission_mut(
                        &mut self.prompt_test.pending_permissions,
                        request_id,
                    ) {
                        permission.resolving = false;
                    }
                    self.prompt_test
                        .permission_history
                        .push(format!("permission resolved: request #{request_id}"));
                    self.prompt_test
                        .pending_permissions
                        .retain(|permission| permission.request_id != request_id);
                    notifications.success("ACP permission response sent");
                    clear_receiver = true;
                }
                Ok((request_id, Err(err))) => {
                    if let Some(permission) = pending_permission_mut(
                        &mut self.prompt_test.pending_permissions,
                        request_id,
                    ) {
                        permission.resolving = false;
                    }
                    notifications.error(format!("Failed to resolve ACP permission: {err}"));
                    clear_receiver = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    for permission in &mut self.prompt_test.pending_permissions {
                        permission.resolving = false;
                    }
                    notifications.error("ACP permission response task disconnected unexpectedly");
                    clear_receiver = true;
                    break;
                }
            }
        }
        if clear_receiver {
            self.permission_action_rx = None;
        }
    }

    fn poll_prompt_test(&mut self, notifications: &mut NotificationCenter) {
        let mut clear_receiver = false;
        for _ in 0..64 {
            let next_event = {
                let Some(rx) = self.prompt_fetch_rx.as_ref() else {
                    return;
                };
                rx.try_recv()
            };
            match next_event {
                Ok(AcpPromptEvent::SessionEvent(event)) => {
                    self.apply_session_event(event);
                    self.prompt_test.window_open = true;
                }
                Ok(AcpPromptEvent::PermissionRequested {
                    request_id,
                    request,
                }) => {
                    self.prompt_test
                        .pending_permissions
                        .push(PendingPermissionState {
                            request_id,
                            request: request.clone(),
                            resolving: false,
                        });
                    self.prompt_test.permission_history.push(format!(
                        "permission requested: {}",
                        permission_title(&request)
                    ));
                    self.prompt_test.window_open = true;
                }
                Ok(AcpPromptEvent::Completed { final_output }) => {
                    self.prompt_test.running = false;
                    self.prompt_test.stopped = false;
                    if self.prompt_test.output.trim().is_empty() {
                        self.prompt_test.output = final_output;
                    }
                    self.prompt_test.last_error = None;
                    self.prompt_test.window_open = true;
                    clear_receiver = true;
                    notifications.success("ACP test prompt completed");
                    break;
                }
                Ok(AcpPromptEvent::Stopped) => {
                    self.prompt_test.running = false;
                    self.prompt_test.stopped = true;
                    self.prompt_test.last_error = None;
                    self.prompt_test.pending_permissions.clear();
                    if !self.prompt_test.output.ends_with("\n[prompt stopped]\n") {
                        if !self.prompt_test.output.ends_with('\n')
                            && !self.prompt_test.output.is_empty()
                        {
                            self.prompt_test.output.push('\n');
                        }
                        self.prompt_test.output.push_str("[prompt stopped]\n");
                    }
                    self.prompt_test.window_open = true;
                    clear_receiver = true;
                    notifications.info("ACP test prompt stopped");
                    break;
                }
                Ok(AcpPromptEvent::Failed(err)) => {
                    self.prompt_test.running = false;
                    self.prompt_test.stopped = false;
                    self.prompt_test.pending_permissions.clear();
                    self.prompt_test.last_error = Some(err.clone());
                    self.prompt_test.window_open = true;
                    clear_receiver = true;
                    notifications.error(format!("ACP test prompt failed: {err}"));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.prompt_test.running = false;
                    self.prompt_test.pending_permissions.clear();
                    self.prompt_test.last_error =
                        Some("ACP test prompt task disconnected unexpectedly".to_string());
                    self.prompt_test.window_open = true;
                    clear_receiver = true;
                    notifications.error("ACP test prompt task disconnected unexpectedly");
                    break;
                }
            }
        }
        if clear_receiver {
            self.prompt_fetch_rx = None;
        }
    }

    fn save_config<F>(
        &mut self,
        notifications: &mut NotificationCenter,
        success_message: &str,
        mutate: F,
    ) -> bool
    where
        F: FnOnce(&mut AppConfig) -> Result<(), String>,
    {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return false;
        };
        match store.update_config(|config| mutate(config).map_err(ConfigError::InvalidConfig)) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.schedule_manager_sync(false);
                notifications.success(success_message);
                true
            }
            Err(err) => {
                notifications.error(format!("Save failed: {err}"));
                false
            }
        }
    }

    fn reload(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                self.schedule_manager_sync(false);
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn open_global_settings(&mut self) {
        self.global_settings_form = Some(self.config.acp.startup_timeout_seconds.to_string());
    }

    fn open_add_agent(&mut self) {
        let existing_ids = self
            .config
            .acp
            .agents
            .iter()
            .map(|agent| agent.id.clone())
            .collect::<BTreeSet<_>>();
        self.form = Some(AcpAgentForm::new(&existing_ids));
    }

    fn open_edit_agent(&mut self, id: &str) {
        if let Some(agent) = self.config.acp.agents.iter().find(|item| item.id == id) {
            self.form = Some(AcpAgentForm::edit(agent));
        }
    }

    fn open_detail_window(&mut self, id: &str) {
        self.detail_window = Some(AcpAgentDetailWindow {
            agent_id: id.to_string(),
        });
    }

    fn delete_agent(&mut self, id: &str, notifications: &mut NotificationCenter) {
        let id = id.to_string();
        let id_for_config = id.clone();
        self.save_config(
            notifications,
            &format!("ACP agent '{id}' deleted"),
            move |config| {
                config.acp.agents.retain(|agent| agent.id != id_for_config);
                Ok(())
            },
        );
        self.runtime_statuses.remove(&id);
        if self.selected_agent.as_deref() == Some(id.as_str()) {
            self.selected_agent = None;
        }
        if self
            .detail_window
            .as_ref()
            .is_some_and(|detail| detail.agent_id == id)
        {
            self.detail_window = None;
        }
        self.ensure_prompt_target_agent();
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.clone() else {
            return;
        };
        if self.save_config(notifications, "ACP agent saved", move |config| {
            let next = Self::apply_form(config.clone(), &form)?;
            *config = next;
            Ok(())
        }) {
            self.form = None;
        }
    }

    fn apply_form(mut config: AppConfig, form: &AcpAgentForm) -> Result<AppConfig, String> {
        let agent = form.to_config();
        if agent.id.is_empty() {
            return Err("ACP agent ID cannot be empty".to_string());
        }
        if agent.command.is_empty() {
            return Err("ACP agent command cannot be empty".to_string());
        }

        let mut replaced = false;
        if let Some(original_id) = form.original_id.as_ref() {
            for item in &mut config.acp.agents {
                if item.id == *original_id {
                    *item = agent.clone();
                    replaced = true;
                    break;
                }
            }
        }

        if !replaced {
            if config.acp.agents.iter().any(|item| item.id == agent.id) {
                return Err(format!(
                    "ACP agent ID '{}' already exists, choose another ID",
                    agent.id
                ));
            }
            config.acp.agents.push(agent);
        }

        Ok(config)
    }

    fn enabled_agents_count(&self) -> usize {
        self.config
            .acp
            .agents
            .iter()
            .filter(|agent| agent.enabled)
            .count()
    }

    fn running_agents_count(&self) -> usize {
        self.runtime_statuses
            .values()
            .filter(|status| status.state == "running")
            .count()
    }

    fn failed_agents_count(&self) -> usize {
        self.runtime_statuses
            .values()
            .filter(|status| status.state == "failed")
            .count()
    }

    fn registered_tools_count(&self) -> usize {
        self.runtime_statuses
            .values()
            .filter(|status| status.state == "running")
            .count()
    }

    fn tool_name_for_agent(id: &str) -> String {
        format!("acp_agent_{}", id.trim())
    }

    fn command_display(agent: &AcpAgentConfig) -> String {
        let mut parts = Vec::new();
        let command = agent.command.trim();
        if !command.is_empty() {
            parts.push(command.to_string());
        }
        parts.extend(agent.args.iter().cloned());
        if parts.is_empty() {
            "-".to_string()
        } else {
            parts.join(" ")
        }
    }

    fn trigger_test_prompt(&mut self, notifications: &mut NotificationCenter) {
        let agent_id = self.prompt_test.agent_id.trim().to_string();
        if agent_id.is_empty() {
            notifications.error("Select a target ACP agent first");
            return;
        }
        let prompt = self.prompt_test.prompt.trim().to_string();
        if prompt.is_empty() {
            notifications.error("Test prompt cannot be empty");
            return;
        }
        if self.prompt_fetch_rx.is_some() {
            notifications.info("ACP test prompt is already running");
            return;
        }
        let working_directory = self.prompt_test.working_directory.trim();
        let working_directory =
            (!working_directory.is_empty()).then(|| working_directory.to_string());
        let timeout_seconds = match self.prompt_test.timeout_seconds.trim() {
            "" => None,
            raw => match raw.parse::<u64>() {
                Ok(value) if value > 0 => Some(value),
                _ => {
                    notifications.error("timeout_seconds must be a positive integer");
                    return;
                }
            },
        };

        match request_execute_acp_prompt_stream(
            &agent_id,
            &prompt,
            working_directory,
            timeout_seconds,
        ) {
            Ok(rx) => {
                self.prompt_fetch_rx = Some(rx);
                self.prompt_test.running = true;
                self.prompt_test.stopped = false;
                self.prompt_test.last_error = None;
                self.prompt_test.output.clear();
                self.prompt_test.session_events.clear();
                self.prompt_test.available_commands.clear();
                self.prompt_test.current_mode_id = None;
                self.prompt_test.config_options.clear();
                self.prompt_test.session_title = None;
                self.prompt_test.session_updated_at = None;
                self.prompt_test.permission_history.clear();
                self.prompt_test.pending_permissions.clear();
                self.permission_action_rx = None;
                self.prompt_test.window_open = true;
            }
            Err(err) => {
                self.prompt_test.running = false;
                self.prompt_test.stopped = false;
                self.prompt_test.last_error = Some(err.clone());
                self.prompt_test.window_open = true;
                notifications.error(format!("Failed to start ACP test prompt: {err}"));
            }
        }
    }

    fn stop_test_prompt(&mut self, notifications: &mut NotificationCenter) {
        match request_stop_acp_prompt() {
            Ok(()) => {
                notifications.info("Stopping ACP test prompt...");
            }
            Err(err) => {
                notifications.error(format!("Failed to stop ACP test prompt: {err}"));
            }
        }
    }

    fn resolve_permission(
        &mut self,
        request_id: u64,
        decision: AcpPermissionDecision,
        notifications: &mut NotificationCenter,
    ) {
        if self.permission_action_rx.is_some() {
            notifications.info("An ACP permission response is already in progress");
            return;
        }
        let Some(permission) = self
            .prompt_test
            .pending_permissions
            .iter_mut()
            .find(|permission| permission.request_id == request_id)
        else {
            notifications.error("ACP permission request is no longer pending");
            return;
        };
        permission.resolving = true;
        let (tx, rx) = mpsc::channel();
        self.permission_action_rx = Some(rx);
        thread::spawn(move || {
            let result = request_resolve_acp_permission(request_id, decision);
            let _ = tx.send((request_id, result));
        });
    }

    fn render_stats(&self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.group(|ui| {
                ui.set_min_width(140.0);
                ui.label(RichText::new("Enabled Agents").strong());
                ui.label(self.enabled_agents_count().to_string());
            });
            ui.group(|ui| {
                ui.set_min_width(140.0);
                ui.label(RichText::new("Running Agents").strong());
                ui.label(self.running_agents_count().to_string());
            });
            ui.group(|ui| {
                ui.set_min_width(140.0);
                ui.label(RichText::new("Failed Agents").strong());
                ui.label(self.failed_agents_count().to_string());
            });
            ui.group(|ui| {
                ui.set_min_width(160.0);
                ui.label(RichText::new("Registered ACP Tools").strong());
                ui.label(self.registered_tools_count().to_string());
            });
        });
    }

    fn render_agent_table(&mut self, ui: &mut egui::Ui, max_height: f32) {
        ui.group(|ui| {
            ui.label(RichText::new("ACP Agents").strong());
            ui.label("Manage external ACP-compatible agents and inspect their runtime state.");
            ui.add_space(8.0);

            if self.config.acp.agents.is_empty() {
                ui.label("No ACP agents configured.");
                return;
            }

            let agent_ids = self
                .config
                .acp
                .agents
                .iter()
                .map(|agent| agent.id.clone())
                .collect::<Vec<_>>();
            let table_width = ui.available_width();
            let max_height = max_height.max(160.0);
            let mut detail_agent_id = None;
            let mut edit_agent_id = None;
            let mut delete_agent_id = None;

            egui::ScrollArea::both()
                .id_salt("acp-agent-table-scroll")
                .auto_shrink([false, false])
                .max_width(table_width)
                .max_height(max_height)
                .show(ui, |ui| {
                    ui.set_min_width(table_width);
                    TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(120.0))
                        .column(Column::remainder().at_least(220.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(140.0))
                        .column(Column::remainder().at_least(160.0))
                        .min_scrolled_height(0.0)
                        .max_scroll_height(max_height)
                        .sense(egui::Sense::click())
                        .header(22.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Enabled");
                            });
                            header.col(|ui| {
                                ui.strong("ID");
                            });
                            header.col(|ui| {
                                ui.strong("Adapter Command");
                            });
                            header.col(|ui| {
                                ui.strong("State");
                            });
                            header.col(|ui| {
                                ui.strong("Tool");
                            });
                            header.col(|ui| {
                                ui.strong("Last Error");
                            });
                        })
                        .body(|body| {
                            body.rows(24.0, agent_ids.len(), |mut row| {
                                let idx = row.index();
                                let agent_id = &agent_ids[idx];
                                let Some(agent) = self
                                    .config
                                    .acp
                                    .agents
                                    .iter()
                                    .find(|item| item.id == *agent_id)
                                else {
                                    return;
                                };
                                let is_selected =
                                    self.selected_agent.as_deref() == Some(agent_id.as_str());
                                row.set_selected(is_selected);
                                let status = self.runtime_statuses.get(agent_id);

                                row.col(|ui| {
                                    ui.label(if agent.enabled { "yes" } else { "no" });
                                });
                                row.col(|ui| {
                                    ui.label(agent_id);
                                });
                                row.col(|ui| {
                                    ui.monospace(Self::command_display(agent));
                                });
                                row.col(|ui| {
                                    let state =
                                        status.map(|item| item.state.as_str()).unwrap_or("stopped");
                                    let color = match state {
                                        "running" => Color32::LIGHT_GREEN,
                                        "failed" => Color32::LIGHT_RED,
                                        "starting" => Color32::YELLOW,
                                        _ => Color32::GRAY,
                                    };
                                    ui.label(RichText::new(state).color(color));
                                });
                                row.col(|ui| {
                                    ui.monospace(Self::tool_name_for_agent(&agent.id));
                                });
                                row.col(|ui| {
                                    if let Some(last_error) =
                                        status.and_then(|item| item.last_error.as_deref())
                                    {
                                        ui.colored_label(Color32::LIGHT_RED, last_error);
                                    } else {
                                        ui.weak("-");
                                    }
                                });

                                let response = row.response();
                                if response.clicked() {
                                    self.selected_agent = if is_selected {
                                        None
                                    } else {
                                        Some(agent_id.clone())
                                    };
                                    self.ensure_prompt_target_agent();
                                }
                                if response.secondary_clicked() && !is_selected {
                                    self.selected_agent = Some(agent_id.clone());
                                    self.ensure_prompt_target_agent();
                                }
                                if response.double_clicked() {
                                    self.selected_agent = Some(agent_id.clone());
                                    self.ensure_prompt_target_agent();
                                    detail_agent_id = Some(agent_id.clone());
                                }

                                let agent_id_clone = agent_id.clone();
                                response.context_menu(|ui| {
                                    if ui
                                        .button(format!("{} Detail", regular::FILE_TEXT))
                                        .clicked()
                                    {
                                        detail_agent_id = Some(agent_id_clone.clone());
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                        .clicked()
                                    {
                                        edit_agent_id = Some(agent_id_clone.clone());
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .add(egui::Button::new(
                                            RichText::new(format!("{} Delete", regular::TRASH))
                                                .color(Color32::RED),
                                        ))
                                        .clicked()
                                    {
                                        delete_agent_id = Some(agent_id_clone.clone());
                                        ui.close();
                                    }
                                });
                            });
                        });
                });

            if let Some(id) = edit_agent_id {
                self.selected_agent = Some(id.clone());
                self.ensure_prompt_target_agent();
                self.open_edit_agent(&id);
            }
            if let Some(id) = detail_agent_id {
                self.selected_agent = Some(id.clone());
                self.ensure_prompt_target_agent();
                self.open_detail_window(&id);
            }
            if let Some(id) = delete_agent_id {
                self.selected_agent = Some(id.clone());
                self.ensure_prompt_target_agent();
                self.delete_confirm = Some(id);
            }
        });
    }

    fn render_prompt_window(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        if !self.prompt_test.window_open {
            return;
        }

        let mut open = true;
        egui::Window::new("ACP Test Prompt")
            .open(&mut open)
            .default_size([820.0, 620.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(
                    "Run one real ACP prompt against a selected external agent and inspect the live stream below.",
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Target Agent");
                    if self.config.acp.agents.is_empty() {
                        ui.monospace("(no agent configured)");
                    } else {
                        egui::ComboBox::from_id_salt("acp-test-prompt-agent-window")
                            .selected_text(self.prompt_test.agent_id.as_str())
                            .show_ui(ui, |ui| {
                                for agent in &self.config.acp.agents {
                                    ui.selectable_value(
                                        &mut self.prompt_test.agent_id,
                                        agent.id.clone(),
                                        agent.id.as_str(),
                                    );
                                }
                            });
                    }
                });

                ui.add_space(6.0);
                ui.label("Prompt");
                ui.add(
                    egui::TextEdit::multiline(&mut self.prompt_test.prompt)
                        .desired_rows(5)
                        .desired_width(f32::INFINITY)
                        .hint_text("Ask the external ACP agent to do something small"),
                );

                ui.add_space(6.0);
                egui::Grid::new("acp-test-prompt-window-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Working Directory");
                        ui.text_edit_singleline(&mut self.prompt_test.working_directory);
                        ui.end_row();

                        ui.label("Timeout Seconds");
                        ui.text_edit_singleline(&mut self.prompt_test.timeout_seconds);
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.prompt_test.running,
                            egui::Button::new(format!("{} Run", regular::PLAY)),
                        )
                        .clicked()
                    {
                        self.trigger_test_prompt(notifications);
                    }
                    if ui
                        .add_enabled(
                            self.prompt_test.running,
                            egui::Button::new(
                                RichText::new(format!("{} Stop", regular::STOP)).color(Color32::RED),
                            ),
                        )
                        .clicked()
                    {
                        self.stop_test_prompt(notifications);
                    }
                    if ui.button("Clear Stream").clicked() {
                        self.prompt_test.output.clear();
                        self.prompt_test.last_error = None;
                        self.prompt_test.stopped = false;
                    }
                });

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Agent").strong());
                    ui.monospace(self.prompt_test.agent_id.as_str());
                    if self.prompt_test.running {
                        ui.spinner();
                        ui.label("Streaming...");
                    } else if self.prompt_test.stopped {
                        ui.colored_label(Color32::YELLOW, "Stopped");
                    } else if self.prompt_test.last_error.is_some() {
                        ui.colored_label(Color32::LIGHT_RED, "Failed");
                    } else {
                        ui.colored_label(Color32::LIGHT_GREEN, "Completed");
                    }
                });

                if !self.prompt_test.working_directory.trim().is_empty() {
                    ui.small(format!(
                        "working_directory: {}",
                        self.prompt_test.working_directory.trim()
                    ));
                }

                if let Some(error) = self.prompt_test.last_error.as_deref() {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Last Error")
                            .strong()
                            .color(Color32::LIGHT_RED),
                    );
                    ui.colored_label(Color32::LIGHT_RED, error);
                }

                ui.add_space(8.0);
                ui.label(RichText::new("Session Snapshot").strong());
                egui::Grid::new("acp-test-prompt-snapshot-grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Title");
                        ui.label(
                            self.prompt_test
                                .session_title
                                .as_deref()
                                .unwrap_or("(not set)"),
                        );
                        ui.end_row();

                        ui.label("Mode");
                        ui.label(
                            self.prompt_test
                                .current_mode_id
                                .as_deref()
                                .unwrap_or("(unknown)"),
                        );
                        ui.end_row();

                        ui.label("Updated At");
                        ui.label(
                            self.prompt_test
                                .session_updated_at
                                .as_deref()
                                .unwrap_or("(not set)"),
                        );
                        ui.end_row();

                        ui.label("Commands");
                        if self.prompt_test.available_commands.is_empty() {
                            ui.weak("(none)");
                        } else {
                            ui.label(
                                self.prompt_test
                                    .available_commands
                                    .iter()
                                    .map(|command| command.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            );
                        }
                        ui.end_row();
                    });

                if !self.prompt_test.config_options.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Config Options").strong());
                    egui::Grid::new("acp-test-prompt-config-options-grid")
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            for option in &self.prompt_test.config_options {
                                ui.label(option.name.as_str());
                                let mut value = option.current_value.clone();
                                if !option.values.is_empty() {
                                    value.push_str("  [");
                                    value.push_str(&option.values.join(", "));
                                    value.push(']');
                                }
                                ui.label(value);
                                ui.end_row();
                            }
                        });
                }

                if !self.prompt_test.pending_permissions.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Pending Permissions").strong());
                    let pending_permissions = self.prompt_test.pending_permissions.clone();
                    for permission in pending_permissions {
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!(
                                    "#{} {}",
                                    permission.request_id,
                                    permission_title(&permission.request)
                                ))
                                .strong());
                                if permission.resolving {
                                    ui.spinner();
                                    ui.weak("sending response...");
                                }
                            });
                            if let Some(kind) = permission.request.kind.as_deref() {
                                ui.small(format!("tool kind: {kind}"));
                            }
                            if let Some(status) = permission.request.status.as_deref() {
                                ui.small(format!("tool status: {status}"));
                            }
                            if let Some(raw_input) = permission.request.raw_input.as_deref() {
                                ui.small(format!("raw input: {raw_input}"));
                            }
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                for option in &permission.request.options {
                                    let button = egui::Button::new(format!(
                                        "{} ({})",
                                        option.label, option.kind
                                    ));
                                    if ui
                                        .add_enabled(!permission.resolving, button)
                                        .clicked()
                                    {
                                        self.resolve_permission(
                                            permission.request_id,
                                            AcpPermissionDecision::SelectOption {
                                                option_id: option.option_id.clone(),
                                            },
                                            notifications,
                                        );
                                    }
                                }
                                if ui
                                    .add_enabled(
                                        !permission.resolving,
                                        egui::Button::new("Cancel").fill(Color32::DARK_RED),
                                    )
                                    .clicked()
                                {
                                    self.resolve_permission(
                                        permission.request_id,
                                        AcpPermissionDecision::Cancelled,
                                        notifications,
                                    );
                                }
                            });
                        });
                    }
                }

                if !self.prompt_test.permission_history.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Permission Timeline").strong());
                    for item in self.prompt_test.permission_history.iter().rev().take(8) {
                        ui.small(item);
                    }
                }

                ui.add_space(8.0);
                ui.label(RichText::new("Structured Events").strong());
                egui::ScrollArea::vertical()
                    .id_salt("acp-test-prompt-events-window")
                    .max_height(180.0)
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if self.prompt_test.session_events.is_empty() {
                            ui.weak("Waiting for ACP session updates...");
                        } else {
                            for event in self.prompt_test.session_events.iter().rev().take(32).rev() {
                                ui.small(event.summary.as_str());
                            }
                        }
                    });

                ui.add_space(8.0);
                ui.label(RichText::new("Raw Stream").strong());
                egui::ScrollArea::vertical()
                    .id_salt("acp-test-prompt-stream-window")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if self.prompt_test.output.trim().is_empty() {
                            ui.weak("Waiting for ACP session updates...");
                        } else {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(self.prompt_test.output.as_str()).monospace(),
                                )
                                .selectable(true)
                                .wrap(),
                            );
                        }
                    });
            });

        self.prompt_test.window_open = open;
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new(form.title())
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(560.0);
                ui.label("ACP agent configuration is persisted to config.toml.");
                ui.separator();
                egui::Grid::new("acp-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("ID");
                        ui.text_edit_singleline(&mut form.id);
                        ui.end_row();

                        ui.label("Enabled");
                        ui.checkbox(&mut form.enabled, "");
                        ui.end_row();

                        ui.label("Command");
                        ui.text_edit_singleline(&mut form.command);
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.small("Runtime working directory comes from the tool/test prompt `working_directory` input.");
                ui.add_space(6.0);
                ui.label("Description");
                ui.add(
                    egui::TextEdit::multiline(&mut form.description)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                form.args_input.show(ui);
                form.env_input.show(ui);

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if save_clicked {
            self.save_form(notifications);
        }
        if cancel_clicked {
            self.form = None;
        }
    }

    fn render_global_settings_window(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        let Some(ref mut timeout_text) = self.global_settings_form else {
            return;
        };

        let mut save_clicked = false;
        let mut close = false;

        egui::Window::new("ACP Settings")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.label("ACP calls external ACP-compatible coding agents over stdio.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("startup_timeout_seconds:");
                    ui.add(egui::TextEdit::singleline(timeout_text).desired_width(90.0));
                });
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });

        if save_clicked {
            let timeout = match timeout_text.trim().parse::<u64>() {
                Ok(value) => value,
                Err(_) => {
                    notifications.error("startup_timeout_seconds must be a positive integer");
                    return;
                }
            };

            if self.save_config(notifications, "ACP settings saved", move |config| {
                config.acp.startup_timeout_seconds = timeout;
                Ok(())
            }) {
                self.global_settings_form = None;
            }
        }
        if close {
            self.global_settings_form = None;
        }
    }

    fn render_delete_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(agent_id) = self.delete_confirm.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Delete ACP Agent")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "Are you sure you want to delete ACP agent '{agent_id}'?"
                    ))
                    .strong(),
                );
                ui.add_space(8.0);
                ui.label("This removes the ACP agent from config.toml.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            RichText::new(format!("{} Delete", regular::TRASH))
                                .color(ui.visuals().warn_fg_color),
                        ))
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            self.delete_agent(&agent_id, notifications);
            self.delete_confirm = None;
        }
        if cancelled {
            self.delete_confirm = None;
        }
    }

    fn render_detail_window(&mut self, ctx: &egui::Context) {
        let Some(detail_window) = self.detail_window.as_ref() else {
            return;
        };
        let Some(agent) = self
            .config
            .acp
            .agents
            .iter()
            .find(|agent| agent.id == detail_window.agent_id)
        else {
            self.detail_window = None;
            return;
        };
        let status = self.runtime_statuses.get(&detail_window.agent_id);

        let mut open = true;
        egui::Window::new(format!("ACP Detail: {}", detail_window.agent_id))
            .open(&mut open)
            .resizable(true)
            .default_width(720.0)
            .default_height(460.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("acp-detail-window-grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("ID");
                            ui.monospace(&agent.id);
                            ui.end_row();

                            ui.label("Enabled");
                            ui.label(if agent.enabled { "yes" } else { "no" });
                            ui.end_row();

                            ui.label("State");
                            ui.label(status.map(|item| item.state.as_str()).unwrap_or("stopped"));
                            ui.end_row();

                            ui.label("Tool Name");
                            ui.monospace(Self::tool_name_for_agent(&agent.id));
                            ui.end_row();

                            ui.label("Command");
                            ui.monospace(Self::command_display(agent));
                            ui.end_row();

                            ui.label("Args");
                            ui.label(if agent.args.is_empty() {
                                "(none)".to_string()
                            } else {
                                agent.args.join(" ")
                            });
                            ui.end_row();

                            ui.label("Env Vars");
                            ui.label(agent.env.len().to_string());
                            ui.end_row();
                        });

                    if !agent.description.trim().is_empty() {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Description").strong());
                        ui.label(agent.description.trim());
                    }
                    if let Some(last_error) = status.and_then(|item| item.last_error.as_deref()) {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Last Error")
                                .strong()
                                .color(Color32::LIGHT_RED),
                        );
                        ui.colored_label(Color32::LIGHT_RED, last_error);
                    }
                    if self.prompt_test.agent_id == detail_window.agent_id
                        && !self.prompt_test.session_events.is_empty()
                    {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Latest Prompt Snapshot").strong());
                        if let Some(mode) = self.prompt_test.current_mode_id.as_deref() {
                            ui.small(format!("mode: {mode}"));
                        }
                        if let Some(title) = self.prompt_test.session_title.as_deref() {
                            ui.small(format!("title: {title}"));
                        }
                        if let Some(updated_at) = self.prompt_test.session_updated_at.as_deref() {
                            ui.small(format!("updated_at: {updated_at}"));
                        }
                        if !self.prompt_test.available_commands.is_empty() {
                            ui.small(format!(
                                "available commands: {}",
                                self.prompt_test
                                    .available_commands
                                    .iter()
                                    .map(|command| command.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                        if !self.prompt_test.config_options.is_empty() {
                            ui.small(format!(
                                "config options: {}",
                                self.prompt_test
                                    .config_options
                                    .iter()
                                    .map(|option| format!("{}={}", option.id, option.current_value))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                        ui.add_space(6.0);
                        for event in self.prompt_test.session_events.iter().rev().take(12).rev() {
                            ui.small(event.summary.as_str());
                        }
                    }
                });
            });

        if !open {
            self.detail_window = None;
        }
    }
}

impl PanelRenderer for AcpPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        self.poll_manager_sync(notifications);
        self.poll_status_refresh(notifications);
        self.poll_prompt_test(notifications);
        self.poll_permission_action(notifications);
        self.refresh_status_if_due();
        ui.ctx().request_repaint_after(ACP_STATUS_POLL_INTERVAL);
        ui.heading(ctx.tab_title);
        ui.label(
            "ACP lets klaw call external ACP-compatible coding agents through adapter commands.",
        );
        ui.small(
            "Default templates use `npx -y @zed-industries/claude-agent-acp` and `npx -y @zed-industries/codex-acp`; runtime cwd comes from `working_directory`.",
        );
        ui.add_space(8.0);
        self.render_stats(ui);
        ui.add_space(8.0);

        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("{} Config", regular::GEAR)).clicked()
                && self.global_settings_form.is_none()
            {
                self.open_global_settings();
            }
            if ui.button("Add Agent").clicked() {
                self.open_add_agent();
            }
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
            if ui
                .button(format!("{} Sync Runtime", regular::ARROWS_CLOCKWISE))
                .clicked()
            {
                self.schedule_manager_sync(true);
            }
            if ui
                .button(format!("{} Refresh Status", regular::ARROW_CLOCKWISE))
                .clicked()
            {
                self.schedule_status_refresh(true);
            }
            if ui.button(format!("{} Test", regular::FLASK)).clicked() {
                self.prompt_test.window_open = true;
            }
        });

        ui.add_space(12.0);
        let remaining_height = ui.available_height();
        self.render_agent_table(ui, remaining_height);

        self.render_detail_window(ui.ctx());
        self.render_prompt_window(ui.ctx(), notifications);
        self.render_form_window(ui, notifications);
        self.render_global_settings_window(ui, notifications);
        self.render_delete_confirm_dialog(ui.ctx(), notifications);
    }
}

fn pending_permission_mut(
    pending_permissions: &mut Vec<PendingPermissionState>,
    request_id: u64,
) -> Option<&mut PendingPermissionState> {
    pending_permissions
        .iter_mut()
        .find(|permission| permission.request_id == request_id)
}

fn render_content_block(content: &AcpContentBlockEvent) -> String {
    match content {
        AcpContentBlockEvent::Text { text } => text.clone(),
        AcpContentBlockEvent::Image {
            mime_type,
            uri,
            data_len,
        } => match uri {
            Some(uri) => format!("[image {mime_type} {data_len} bytes {uri}]"),
            None => format!("[image {mime_type} {data_len} bytes]"),
        },
        AcpContentBlockEvent::Audio {
            mime_type,
            data_len,
        } => format!("[audio {mime_type} {data_len} bytes]"),
        AcpContentBlockEvent::ResourceLink {
            name, uri, title, ..
        } => match title {
            Some(title) => format!("[resource {name} {title} {uri}]"),
            None => format!("[resource {name} {uri}]"),
        },
        AcpContentBlockEvent::EmbeddedTextResource {
            uri,
            mime_type,
            text,
        } => match mime_type {
            Some(mime_type) => format!("[embedded text {uri} {mime_type}] {text}"),
            None => format!("[embedded text {uri}] {text}"),
        },
        AcpContentBlockEvent::EmbeddedBlobResource {
            uri,
            mime_type,
            byte_len,
        } => match mime_type {
            Some(mime_type) => format!("[embedded blob {uri} {mime_type} {byte_len} bytes]"),
            None => format!("[embedded blob {uri} {byte_len} bytes]"),
        },
        AcpContentBlockEvent::Unsupported { description } => {
            format!("[unsupported content {description}]")
        }
    }
}

fn permission_title(request: &AcpPermissionRequest) -> String {
    request
        .title
        .clone()
        .unwrap_or_else(|| request.tool_call_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn command_display_joins_command_and_args() {
        let agent = AcpAgentConfig {
            id: "claude_code".to_string(),
            enabled: true,
            command: "npx".to_string(),
            args: vec![
                "-y".to_string(),
                "@zed-industries/claude-agent-acp".to_string(),
            ],
            env: BTreeMap::new(),
            description: String::new(),
        };

        assert_eq!(
            AcpPanel::command_display(&agent),
            "npx -y @zed-industries/claude-agent-acp"
        );
    }

    #[test]
    fn command_display_returns_dash_when_empty() {
        let agent = AcpAgentConfig {
            id: "empty".to_string(),
            enabled: true,
            command: "  ".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            description: String::new(),
        };

        assert_eq!(AcpPanel::command_display(&agent), "-");
    }

    #[test]
    fn render_content_block_formats_resource_link() {
        let rendered = super::render_content_block(&AcpContentBlockEvent::ResourceLink {
            name: "README".to_string(),
            uri: "file:///workspace/README.md".to_string(),
            title: Some("Workspace README".to_string()),
            description: None,
            mime_type: Some("text/markdown".to_string()),
        });

        assert_eq!(
            rendered,
            "[resource README Workspace README file:///workspace/README.md]"
        );
    }

    #[test]
    fn permission_title_prefers_explicit_title() {
        let request = AcpPermissionRequest {
            session_id: "session-1".to_string(),
            tool_call_id: "tool-1".to_string(),
            title: Some("Write output.txt".to_string()),
            kind: Some("edit".to_string()),
            status: Some("pending".to_string()),
            raw_input: None,
            raw_output: None,
            options: Vec::new(),
        };

        assert_eq!(super::permission_title(&request), "Write output.txt");
    }
}
