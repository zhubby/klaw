use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::{
    RuntimeRequestHandle, begin_restart_mcp_server_request, request_mcp_status, request_sync_mcp,
};
use crate::widgets::{ArrayEditor, KeyValueEditor, markdown};
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, McpServerConfig, McpServerMode,
};
use klaw_mcp::{McpLifecycleState, McpRuntimeSnapshot, McpServerDetail, McpSyncResult};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

const MCP_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
struct McpServerForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    mode: McpServerMode,
    tool_timeout_seconds: String,
    command: String,
    args_input: ArrayEditor,
    env_input: KeyValueEditor,
    cwd: String,
    url: String,
    headers_input: KeyValueEditor,
}

impl McpServerForm {
    fn new() -> Self {
        Self {
            original_id: None,
            id: String::new(),
            enabled: true,
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: "60".to_string(),
            command: String::new(),
            args_input: ArrayEditor::new("Args"),
            env_input: KeyValueEditor::new("Env"),
            cwd: String::new(),
            url: String::new(),
            headers_input: KeyValueEditor::new("Headers"),
        }
    }

    fn edit(server: &McpServerConfig) -> Self {
        Self {
            original_id: Some(server.id.clone()),
            id: server.id.clone(),
            enabled: server.enabled,
            mode: server.mode.clone(),
            tool_timeout_seconds: server.tool_timeout_seconds.to_string(),
            command: server.command.clone().unwrap_or_default(),
            args_input: ArrayEditor::from_vec("Args", &server.args),
            env_input: KeyValueEditor::from_map("Env", &server.env),
            cwd: server.cwd.clone().unwrap_or_default(),
            url: server.url.clone().unwrap_or_default(),
            headers_input: KeyValueEditor::from_map("Headers", &server.headers),
        }
    }

    fn title(&self) -> &'static str {
        if self.original_id.is_some() {
            "Edit MCP Server"
        } else {
            "Add MCP Server"
        }
    }

    fn normalized_id(&self) -> String {
        self.id.trim().to_string()
    }

    fn to_config(&self) -> Result<McpServerConfig, String> {
        let command = self.command.trim();
        let cwd = self.cwd.trim();
        let url = self.url.trim();
        let tool_timeout_seconds = self
            .tool_timeout_seconds
            .trim()
            .parse::<u64>()
            .map_err(|_| "tool_timeout_seconds must be a positive integer".to_string())?;
        if tool_timeout_seconds == 0 {
            return Err("tool_timeout_seconds must be a positive integer".to_string());
        }

        Ok(McpServerConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            mode: self.mode.clone(),
            tool_timeout_seconds,
            command: (!command.is_empty()).then(|| command.to_string()),
            args: self.args_input.to_vec(),
            env: self.env_input.to_map(),
            cwd: (!cwd.is_empty()).then(|| cwd.to_string()),
            url: (!url.is_empty()).then(|| url.to_string()),
            headers: self.headers_input.to_map(),
        })
    }
}

#[derive(Debug, Clone)]
struct ServerRuntimeStatus {
    state: McpLifecycleState,
    tool_count: usize,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct McpServerDetailWindow {
    server_id: String,
    markdown: String,
}

#[derive(Default)]
pub struct McpPanel {
    store: Option<ConfigStore>,
    config: AppConfig,
    form: Option<McpServerForm>,
    global_settings_form: Option<String>,
    selected_server: Option<String>,
    server_statuses: BTreeMap<String, ServerRuntimeStatus>,
    server_details: BTreeMap<String, McpServerDetail>,
    detail_window: Option<McpServerDetailWindow>,
    detail_markdown_cache: markdown::MarkdownCache,
    status_fetch_rx: Option<Receiver<Result<McpRuntimeSnapshot, String>>>,
    sync_fetch_rx: Option<Receiver<Result<McpSyncResult, String>>>,
    restart_request: Option<RuntimeRequestHandle<McpRuntimeSnapshot>>,
    restart_target_key: Option<String>,
    last_status_refresh_at: Option<Instant>,
    status_refresh_announce: bool,
    status_refresh_manual: bool,
    sync_announce: bool,
}

impl McpPanel {
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
                notifications.success("MCP config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
    }

    fn open_global_settings(&mut self) {
        self.global_settings_form = Some(self.config.mcp.startup_timeout_seconds.to_string());
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
            let _ = tx.send(request_mcp_status());
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
            let _ = tx.send(request_sync_mcp());
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

        if last_refresh.elapsed() >= MCP_STATUS_POLL_INTERVAL {
            self.schedule_status_refresh(false);
        }
    }

    fn apply_runtime_snapshot(&mut self, result: McpRuntimeSnapshot) {
        self.server_statuses = result
            .statuses
            .into_iter()
            .map(|status| {
                (
                    status.key.as_str().to_string(),
                    ServerRuntimeStatus {
                        state: status.state,
                        tool_count: status.tool_count,
                        last_error: status.last_error,
                    },
                )
            })
            .collect();
        self.server_details = result
            .details
            .into_iter()
            .map(|detail| (detail.key.as_str().to_string(), detail))
            .collect();
    }

    fn poll_status_refresh(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.status_fetch_rx.as_ref() else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.apply_runtime_snapshot(result);
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications.success("MCP status refreshed");
                }
                self.status_refresh_announce = false;
                self.status_refresh_manual = false;
            }
            Ok(Err(err)) => {
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications.error(format!("Failed to refresh MCP status: {err}"));
                }
                self.status_refresh_announce = false;
                self.status_refresh_manual = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications
                        .error("Failed to refresh MCP status: background task disconnected");
                }
                self.status_refresh_announce = false;
                self.status_refresh_manual = false;
            }
        }
    }

    fn poll_restart_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.restart_request.as_mut() else {
            return;
        };
        let Some(result) = request.try_take_result() else {
            return;
        };

        self.restart_request = None;
        let target = self
            .restart_target_key
            .take()
            .unwrap_or_else(|| "selected mcp server".to_string());
        match result {
            Ok(snapshot) => {
                self.apply_runtime_snapshot(snapshot);
                notifications.success(format!("Restarted MCP server {}", target));
            }
            Err(err) => {
                notifications.error(format!("Failed to restart {}: {}", target, err));
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
                    notifications.success("MCP runtime synchronized");
                }
                self.sync_announce = false;
            }
            Ok(Err(err)) => {
                self.sync_fetch_rx = None;
                if self.sync_announce {
                    notifications.error(format!("Failed to sync MCP runtime: {err}"));
                } else {
                    notifications.error(format!("Failed to sync MCP runtime: {err}"));
                }
                self.sync_announce = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sync_fetch_rx = None;
                notifications.error("Failed to sync MCP runtime: background task disconnected");
                self.sync_announce = false;
            }
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

    fn restart_server(&mut self, id: &str, notifications: &mut NotificationCenter) {
        if self.restart_request.is_some() {
            notifications.info("An MCP server restart is already in progress");
            return;
        }
        self.mark_server_restarting(id);
        self.restart_target_key = Some(id.to_string());
        self.restart_request = Some(begin_restart_mcp_server_request(id.to_string()));
    }

    fn mark_server_restarting(&mut self, id: &str) {
        // Right after the user requests a restart, the old runtime snapshot may still
        // report `running`. Force the row into `starting` until the fresh snapshot arrives.
        self.server_statuses.insert(
            id.to_string(),
            ServerRuntimeStatus {
                state: McpLifecycleState::Starting,
                tool_count: 0,
                last_error: None,
            },
        );
    }

    fn open_add_server(&mut self) {
        self.form = Some(McpServerForm::new());
    }

    fn open_edit_server(&mut self, id: &str) {
        if let Some(server) = self.config.mcp.servers.iter().find(|item| item.id == id) {
            self.form = Some(McpServerForm::edit(server));
        }
    }

    fn delete_server(&mut self, id: &str, notifications: &mut NotificationCenter) {
        let id = id.to_string();
        let id_for_config = id.clone();
        self.save_config(
            notifications,
            &format!("MCP server '{id}' deleted"),
            move |config| {
                config.mcp.servers.retain(|s| s.id != id_for_config);
                Ok(())
            },
        );
        self.server_statuses.remove(&id);
        self.server_details.remove(&id);
        if self.selected_server.as_deref() == Some(id.as_str()) {
            self.selected_server = None;
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.clone() else {
            return;
        };
        if self.save_config(notifications, "MCP server saved", move |config| {
            let next = Self::apply_form(config.clone(), &form)?;
            *config = next;
            Ok(())
        }) {
            self.form = None;
        }
    }

    fn apply_form(mut config: AppConfig, form: &McpServerForm) -> Result<AppConfig, String> {
        let server = form.to_config()?;
        if server.id.is_empty() {
            return Err("MCP server ID cannot be empty".to_string());
        }

        let mut replaced = false;
        if let Some(original_id) = form.original_id.as_ref() {
            for item in &mut config.mcp.servers {
                if item.id == *original_id {
                    *item = server.clone();
                    replaced = true;
                    break;
                }
            }
        }

        if !replaced {
            if config.mcp.servers.iter().any(|item| item.id == server.id) {
                return Err(format!(
                    "MCP server ID '{}' already exists, choose another ID",
                    server.id
                ));
            }
            config.mcp.servers.push(server);
        }

        Ok(config)
    }

    fn server_state(&self, id: &str) -> McpLifecycleState {
        self.server_statuses
            .get(id)
            .map(|status| status.state)
            .unwrap_or(McpLifecycleState::Stopped)
    }

    fn server_tool_count(&self, id: &str) -> usize {
        self.server_statuses
            .get(id)
            .map(|status| status.tool_count)
            .unwrap_or(0)
    }

    fn open_detail_window(&mut self, server_id: &str) {
        let detail = self.server_details.get(server_id).cloned();
        let status = self.server_statuses.get(server_id).cloned();
        self.detail_window = Some(McpServerDetailWindow {
            server_id: server_id.to_string(),
            markdown: build_server_detail_markdown(server_id, status.as_ref(), detail.as_ref()),
        });
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
                egui::Grid::new("mcp-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("ID");
                        ui.text_edit_singleline(&mut form.id);
                        ui.end_row();

                        ui.label("Enabled");
                        ui.checkbox(&mut form.enabled, "");
                        ui.end_row();

                        ui.label("Mode");
                        egui::ComboBox::from_id_salt("mcp-mode")
                            .selected_text(match form.mode {
                                McpServerMode::Stdio => "stdio",
                                McpServerMode::Sse => "sse",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut form.mode, McpServerMode::Stdio, "stdio");
                                ui.selectable_value(&mut form.mode, McpServerMode::Sse, "sse");
                            });
                        ui.end_row();

                        ui.label("Tool Timeout Seconds");
                        ui.text_edit_singleline(&mut form.tool_timeout_seconds);
                        ui.end_row();
                    });

                ui.separator();

                match form.mode {
                    McpServerMode::Stdio => {
                        egui::Grid::new("mcp-stdio-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("Command");
                                ui.text_edit_singleline(&mut form.command);
                                ui.end_row();

                                ui.label("CWD");
                                ui.text_edit_singleline(&mut form.cwd);
                                ui.end_row();
                            });

                        form.args_input.show(ui);
                        form.env_input.show(ui);
                    }
                    McpServerMode::Sse => {
                        egui::Grid::new("mcp-sse-grid")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("URL");
                                ui.text_edit_singleline(&mut form.url);
                                ui.end_row();
                            });

                        form.headers_input.show(ui);
                    }
                }

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

        egui::Window::new("MCP Settings")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_width(320.0);
                ui.horizontal(|ui| {
                    ui.label("startup_timeout_seconds:");
                    ui.add(egui::TextEdit::singleline(timeout_text).desired_width(80.0));
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

            if self.save_config(notifications, "MCP settings saved", move |config| {
                config.mcp.startup_timeout_seconds = timeout;
                Ok(())
            }) {
                self.global_settings_form = None;
            }
        }
        if close {
            self.global_settings_form = None;
        }
    }

    fn render_detail_window(&mut self, ctx: &egui::Context) {
        let Some(detail_window) = self.detail_window.as_mut() else {
            return;
        };

        let mut open = true;
        egui::Window::new(format!("MCP Detail: {}", detail_window.server_id))
            .open(&mut open)
            .resizable(true)
            .default_width(860.0)
            .default_height(620.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("mcp-detail-scroll", &detail_window.server_id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        markdown::render(
                            ui,
                            &mut self.detail_markdown_cache,
                            &detail_window.markdown,
                        )
                    });
            });

        if !open {
            self.detail_window = None;
        }
    }
}

impl PanelRenderer for McpPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        self.poll_manager_sync(notifications);
        self.poll_status_refresh(notifications);
        self.poll_restart_request(notifications);
        self.refresh_status_if_due();
        ui.ctx().request_repaint_after(MCP_STATUS_POLL_INTERVAL);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            ui.label(format!("Servers: {}", self.config.mcp.servers.len()));
            if self.sync_fetch_rx.is_some() {
                ui.spinner();
                ui.label("Applying MCP changes...");
            }
            if self.status_fetch_rx.is_some() {
                if self.status_refresh_manual {
                    ui.spinner();
                    ui.label("Refreshing runtime status...");
                }
            }
            if self.restart_request.is_some() {
                ui.spinner();
                ui.label("Restarting MCP server...");
            }
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button(format!("{} Config", regular::GEAR)).clicked()
                && self.global_settings_form.is_none()
            {
                self.open_global_settings();
            }
            if ui.button("Add").clicked() {
                self.open_add_server();
            }
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
            if ui
                .button(format!("{} Refresh Status", regular::ARROW_CLOCKWISE))
                .clicked()
            {
                self.schedule_status_refresh(true);
            }
        });

        ui.add_space(8.0);

        let server_ids = self
            .config
            .mcp
            .servers
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();

        if server_ids.is_empty() {
            ui.label("No MCP servers configured.");
        } else {
            let mut edit_server_id = None;
            let mut delete_server_id = None;
            let mut detail_server_id = None;
            let mut restart_server_id = None;
            let mut open_settings_from_menu = false;
            let available_height = ui.available_height();

            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(120.0))
                .column(Column::auto().at_least(60.0))
                .column(Column::auto().at_least(90.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::remainder().at_least(240.0))
                .column(Column::auto().at_least(60.0))
                .column(Column::auto().at_least(60.0))
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height)
                .sense(egui::Sense::click())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("ID");
                    });
                    header.col(|ui| {
                        ui.strong("On");
                    });
                    header.col(|ui| {
                        ui.strong("Status");
                    });
                    header.col(|ui| {
                        ui.strong("Mode");
                    });
                    header.col(|ui| {
                        ui.strong("Command/URL");
                    });
                    header.col(|ui| {
                        ui.strong("Args");
                    });
                    header.col(|ui| {
                        ui.strong("Tools");
                    });
                })
                .body(|body| {
                    body.rows(22.0, server_ids.len(), |mut row| {
                        let idx = row.index();
                        let server_id = &server_ids[idx];
                        let Some(server) = self
                            .config
                            .mcp
                            .servers
                            .iter()
                            .find(|item| item.id == *server_id)
                        else {
                            return;
                        };

                        let is_selected = self.selected_server.as_deref() == Some(server_id);
                        row.set_selected(is_selected);

                        let state = self.server_state(server_id);
                        let tool_count = self.server_tool_count(server_id);

                        row.col(|ui| {
                            ui.label(&server.id);
                        });
                        row.col(|ui| {
                            ui.label(if server.enabled { "yes" } else { "no" });
                        });
                        row.col(|ui| {
                            ui.label(
                                RichText::new(server_state_label(state))
                                    .color(server_state_color(state)),
                            );
                        });
                        row.col(|ui| {
                            ui.label(match server.mode {
                                McpServerMode::Stdio => "stdio",
                                McpServerMode::Sse => "sse",
                            });
                        });
                        row.col(|ui| {
                            ui.label(command_display(server));
                        });
                        row.col(|ui| {
                            ui.label(server.args.len().to_string());
                        });
                        row.col(|ui| {
                            ui.label(tool_count.to_string());
                        });

                        let response = row.response();
                        if response.clicked() {
                            self.selected_server = if is_selected {
                                None
                            } else {
                                Some(server_id.clone())
                            };
                        }

                        let server_id_clone = server_id.clone();
                        response.context_menu(|ui| {
                            if ui
                                .button(format!("{} Detail", regular::FILE_TEXT))
                                .clicked()
                            {
                                detail_server_id = Some(server_id_clone.clone());
                                ui.close();
                            }
                            if ui
                                .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                .clicked()
                            {
                                edit_server_id = Some(server_id_clone.clone());
                                ui.close();
                            }
                            if ui.button(format!("{} Config", regular::GEAR)).clicked() {
                                open_settings_from_menu = true;
                                ui.close();
                            }
                            if can_restart_server(server)
                                && ui
                                    .button(format!("{} Restart", regular::ARROW_CLOCKWISE))
                                    .clicked()
                            {
                                restart_server_id = Some(server_id_clone.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add(egui::Button::new(
                                    RichText::new(format!("{} Delete", regular::TRASH))
                                        .color(egui::Color32::RED),
                                ))
                                .clicked()
                            {
                                delete_server_id = Some(server_id_clone.clone());
                                ui.close();
                            }
                        });
                    });
                });

            if let Some(id) = edit_server_id {
                self.open_edit_server(&id);
            }
            if let Some(id) = detail_server_id {
                self.open_detail_window(&id);
            }
            if open_settings_from_menu && self.global_settings_form.is_none() {
                self.open_global_settings();
            }
            if let Some(id) = restart_server_id {
                self.restart_server(&id, notifications);
            }
            if let Some(id) = delete_server_id {
                self.delete_server(&id, notifications);
            }
        }

        self.render_global_settings_window(ui, notifications);
        self.render_detail_window(ui.ctx());
        self.render_form_window(ui, notifications);
    }
}

fn build_server_detail_markdown(
    server_id: &str,
    status: Option<&ServerRuntimeStatus>,
    detail: Option<&McpServerDetail>,
) -> String {
    let state = status
        .map(|item| server_state_label(item.state))
        .unwrap_or(server_state_label(McpLifecycleState::Stopped));
    let tool_count = status.map(|item| item.tool_count).unwrap_or(0);
    let last_error = status
        .and_then(|item| item.last_error.as_deref())
        .unwrap_or("-");
    let tools_list_response = detail
        .and_then(|item| item.tools_list_response.as_ref())
        .map(pretty_json)
        .unwrap_or_else(|| "null".to_string());

    format!(
        "# MCP Server Detail\n\n- Server: `{server_id}`\n- State: `{state}`\n- Tools: `{tool_count}`\n- Last Error: `{last_error}`\n\n## tools/list response\n\n```json\n{tools_list_response}\n```"
    )
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|err| format!("{{\"error\":\"failed to render json: {err}\"}}"))
}

fn command_display(server: &McpServerConfig) -> String {
    match server.mode {
        McpServerMode::Stdio => {
            let mut parts = Vec::new();
            if let Some(command) = server.command.as_deref() {
                let command = command.trim();
                if !command.is_empty() {
                    parts.push(command.to_string());
                }
            }
            parts.extend(server.args.iter().cloned());
            if parts.is_empty() {
                "-".to_string()
            } else {
                parts.join(" ")
            }
        }
        McpServerMode::Sse => server.url.clone().unwrap_or_else(|| "-".to_string()),
    }
}

fn can_restart_server(server: &McpServerConfig) -> bool {
    server.mode == McpServerMode::Stdio && server.enabled
}

fn server_state_label(state: McpLifecycleState) -> &'static str {
    match state {
        McpLifecycleState::Starting => "starting",
        McpLifecycleState::Running => "running",
        McpLifecycleState::Stopped => "stopped",
        McpLifecycleState::Failed => "failed",
    }
}

fn server_state_color(state: McpLifecycleState) -> egui::Color32 {
    match state {
        McpLifecycleState::Starting => egui::Color32::from_rgb(245, 191, 66),
        McpLifecycleState::Running => egui::Color32::from_rgb(82, 196, 107),
        McpLifecycleState::Stopped => egui::Color32::from_rgb(132, 146, 166),
        McpLifecycleState::Failed => egui::Color32::from_rgb(232, 86, 86),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_form_adds_new_server() {
        let config = AppConfig::default();
        let mut form = McpServerForm::new();
        form.id = "docs".to_string();
        form.mode = McpServerMode::Sse;
        form.url = "https://example.com/sse".to_string();

        let updated = McpPanel::apply_form(config, &form).expect("should apply");

        assert!(updated.mcp.servers.iter().any(|item| item.id == "docs"));
    }

    #[test]
    fn apply_form_rejects_duplicate_id() {
        let mut config = AppConfig::default();
        config.mcp.servers.push(McpServerConfig {
            id: "docs".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: 60,
            command: Some("uvx mcp-docs".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        });

        let mut form = McpServerForm::new();
        form.id = "docs".to_string();

        let err = McpPanel::apply_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_form_edits_existing_server() {
        let mut config = AppConfig::default();
        config.mcp.servers.push(McpServerConfig {
            id: "docs".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: 90,
            command: Some("old".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        });

        let source = config
            .mcp
            .servers
            .iter()
            .find(|item| item.id == "docs")
            .expect("server should exist")
            .clone();
        let mut form = McpServerForm::edit(&source);
        form.command = "new".to_string();

        let updated = McpPanel::apply_form(config, &form).expect("should apply");
        let server = updated
            .mcp
            .servers
            .iter()
            .find(|item| item.id == "docs")
            .expect("server should exist");
        assert_eq!(server.command.as_deref(), Some("new"));
        assert_eq!(server.tool_timeout_seconds, 90);
    }

    #[test]
    fn server_status_defaults_to_stopped_without_runtime_data() {
        let panel = McpPanel::default();

        assert_eq!(panel.server_state("missing"), McpLifecycleState::Stopped);
        assert_eq!(
            server_state_label(panel.server_state("missing")),
            server_state_label(McpLifecycleState::Stopped)
        );
        assert_eq!(panel.server_tool_count("missing"), 0);
    }

    #[test]
    fn command_display_joins_stdio_command_and_args() {
        let server = McpServerConfig {
            id: "browser".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: 60,
            command: Some("npx".to_string()),
            args: vec!["@browsermcp/mcp@latest".to_string()],
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        };

        assert_eq!(command_display(&server), "npx @browsermcp/mcp@latest");
    }

    #[test]
    fn restart_action_only_shows_for_enabled_stdio_servers() {
        let enabled_stdio = McpServerConfig {
            id: "browser".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            tool_timeout_seconds: 60,
            command: Some("npx".to_string()),
            args: vec!["@browsermcp/mcp".to_string()],
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        };
        let disabled_stdio = McpServerConfig {
            enabled: false,
            ..enabled_stdio.clone()
        };
        let sse_server = McpServerConfig {
            id: "context7".to_string(),
            enabled: true,
            mode: McpServerMode::Sse,
            tool_timeout_seconds: 60,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: Some("https://mcp.context7.com/mcp".to_string()),
            headers: BTreeMap::new(),
        };

        assert!(can_restart_server(&enabled_stdio));
        assert!(!can_restart_server(&disabled_stdio));
        assert!(!can_restart_server(&sse_server));
    }

    #[test]
    fn mark_server_restarting_updates_row_state_immediately() {
        let mut panel = McpPanel::default();
        panel.server_statuses.insert(
            "browser".to_string(),
            ServerRuntimeStatus {
                state: McpLifecycleState::Running,
                tool_count: 12,
                last_error: Some("old error".to_string()),
            },
        );

        panel.mark_server_restarting("browser");

        let status = panel
            .server_statuses
            .get("browser")
            .expect("server status should exist");
        assert_eq!(status.state, McpLifecycleState::Starting);
        assert_eq!(status.tool_count, 0);
        assert_eq!(status.last_error, None);
    }

    #[test]
    fn server_state_helpers_define_distinct_labels_and_colors() {
        assert_eq!(server_state_label(McpLifecycleState::Starting), "starting");
        assert_eq!(server_state_label(McpLifecycleState::Running), "running");
        assert_eq!(server_state_label(McpLifecycleState::Stopped), "stopped");
        assert_eq!(server_state_label(McpLifecycleState::Failed), "failed");

        assert_ne!(
            server_state_color(McpLifecycleState::Stopped),
            server_state_color(McpLifecycleState::Running)
        );
        assert_ne!(
            server_state_color(McpLifecycleState::Stopped),
            server_state_color(McpLifecycleState::Failed)
        );
        assert_ne!(
            server_state_color(McpLifecycleState::Stopped),
            server_state_color(McpLifecycleState::Starting)
        );
    }
}
