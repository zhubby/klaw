use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::request_sync_mcp;
use crate::widgets::{ArrayEditor, KeyValueEditor};
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore, McpServerConfig, McpServerMode};
use klaw_mcp::McpSyncResult;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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

    fn to_config(&self) -> McpServerConfig {
        let command = self.command.trim();
        let cwd = self.cwd.trim();
        let url = self.url.trim();

        McpServerConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            mode: self.mode.clone(),
            command: (!command.is_empty()).then(|| command.to_string()),
            args: self.args_input.to_vec(),
            env: self.env_input.to_map(),
            cwd: (!cwd.is_empty()).then(|| cwd.to_string()),
            url: (!url.is_empty()).then(|| url.to_string()),
            headers: self.headers_input.to_map(),
        }
    }
}

#[derive(Debug, Clone)]
struct ServerRuntimeStatus {
    state: String,
    tool_count: usize,
}

#[derive(Default)]
pub struct McpPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<McpServerForm>,
    global_settings_form: Option<(bool, String)>,
    selected_server: Option<String>,
    server_statuses: BTreeMap<String, ServerRuntimeStatus>,
    status_fetch_rx: Option<Receiver<Result<McpSyncResult, String>>>,
    last_status_refresh_at: Option<Instant>,
    status_refresh_announce: bool,
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
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.config = snapshot.config;
    }

    fn open_global_settings(&mut self) {
        self.global_settings_form = Some((
            self.config.mcp.enabled,
            self.config.mcp.startup_timeout_seconds.to_string(),
        ));
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

    fn poll_status_refresh(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.status_fetch_rx.as_ref() else {
            return;
        };

        match rx.try_recv() {
            Ok(Ok(result)) => {
                self.server_statuses = result
                    .statuses
                    .into_iter()
                    .map(|status| {
                        (
                            status.key.as_str().to_string(),
                            ServerRuntimeStatus {
                                state: status.state.as_str().to_string(),
                                tool_count: status.tool_count,
                            },
                        )
                    })
                    .collect();
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications.success("MCP status refreshed");
                }
                self.status_refresh_announce = false;
            }
            Ok(Err(err)) => {
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications.error(format!("Failed to refresh MCP status: {err}"));
                }
                self.status_refresh_announce = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status_fetch_rx = None;
                if self.status_refresh_announce {
                    notifications
                        .error("Failed to refresh MCP status: background task disconnected");
                }
                self.status_refresh_announce = false;
            }
        }
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Path: {}", path.display()),
            None => "Path: (not loaded)".to_string(),
        }
    }

    fn save_config(
        &mut self,
        next: AppConfig,
        notifications: &mut NotificationCenter,
        success_message: &str,
    ) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match toml::to_string_pretty(&next) {
            Ok(raw) => match store.save_raw_toml(&raw) {
                Ok(snapshot) => {
                    self.apply_snapshot(snapshot);
                    self.schedule_status_refresh(false);
                    notifications.success(success_message);
                }
                Err(err) => notifications.error(format!("Save failed: {err}")),
            },
            Err(err) => notifications.error(format!("Failed to render config TOML: {err}")),
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
                self.schedule_status_refresh(false);
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
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
        let mut next = self.config.clone();
        next.mcp.servers.retain(|s| s.id != id);
        self.save_config(next, notifications, &format!("MCP server '{id}' deleted"));
        self.server_statuses.remove(id);
        if self.selected_server.as_deref() == Some(id) {
            self.selected_server = None;
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        match Self::apply_form(self.config.clone(), form) {
            Ok(next) => {
                self.save_config(next, notifications, "MCP server saved");
                self.form = None;
            }
            Err(err) => notifications.error(err),
        }
    }

    fn apply_form(mut config: AppConfig, form: &McpServerForm) -> Result<AppConfig, String> {
        let server = form.to_config();
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

    fn server_status_text(&self, id: &str) -> &str {
        self.server_statuses
            .get(id)
            .map(|status| status.state.as_str())
            .unwrap_or("stopped")
    }

    fn server_tool_count(&self, id: &str) -> usize {
        self.server_statuses
            .get(id)
            .map(|status| status.tool_count)
            .unwrap_or(0)
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
        let Some((ref mut enabled, ref mut timeout_text)) = self.global_settings_form else {
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
                ui.checkbox(enabled, "MCP Enabled");
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

            let mut next = self.config.clone();
            next.mcp.enabled = *enabled;
            next.mcp.startup_timeout_seconds = timeout;
            self.save_config(next, notifications, "MCP settings saved");
            self.global_settings_form = None;
        }
        if close {
            self.global_settings_form = None;
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
        self.poll_status_refresh(notifications);
        self.refresh_status_if_due();
        ui.ctx().request_repaint_after(MCP_STATUS_POLL_INTERVAL);

        ui.heading(ctx.tab_title);
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.horizontal(|ui| {
            ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
            ui.label(format!("Servers: {}", self.config.mcp.servers.len()));
            if self.status_fetch_rx.is_some() {
                ui.spinner();
                ui.label("Refreshing runtime status...");
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
            let mut open_settings_from_menu = false;
            let available_height = ui.available_height();

            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(120.0))
                .column(Column::auto().at_least(60.0))
                .column(Column::auto().at_least(90.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::remainder().at_least(180.0))
                .column(Column::auto().at_least(60.0))
                .column(Column::auto().at_least(100.0))
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
                        ui.strong("Env/Headers");
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

                        let status = self.server_status_text(server_id);
                        let tool_count = self.server_tool_count(server_id);

                        row.col(|ui| {
                            ui.label(&server.id);
                        });
                        row.col(|ui| {
                            ui.label(if server.enabled { "yes" } else { "no" });
                        });
                        row.col(|ui| {
                            let color = match status {
                                "running" => egui::Color32::LIGHT_GREEN,
                                "failed" => egui::Color32::LIGHT_RED,
                                "starting" => egui::Color32::YELLOW,
                                _ => egui::Color32::GRAY,
                            };
                            ui.label(RichText::new(status).color(color));
                        });
                        row.col(|ui| {
                            ui.label(match server.mode {
                                McpServerMode::Stdio => "stdio",
                                McpServerMode::Sse => "sse",
                            });
                        });
                        row.col(|ui| {
                            let endpoint = match server.mode {
                                McpServerMode::Stdio => server.command.as_deref().unwrap_or("-"),
                                McpServerMode::Sse => server.url.as_deref().unwrap_or("-"),
                            };
                            ui.label(endpoint);
                        });
                        row.col(|ui| {
                            ui.label(server.args.len().to_string());
                        });
                        row.col(|ui| {
                            ui.label(format!(
                                "env:{} hdr:{}",
                                server.env.len(),
                                server.headers.len()
                            ));
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
            if open_settings_from_menu && self.global_settings_form.is_none() {
                self.open_global_settings();
            }
            if let Some(id) = delete_server_id {
                self.delete_server(&id, notifications);
            }
        }

        self.render_global_settings_window(ui, notifications);
        self.render_form_window(ui, notifications);
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
    }

    #[test]
    fn server_status_defaults_to_stopped_without_runtime_data() {
        let panel = McpPanel::default();

        assert_eq!(panel.server_status_text("missing"), "stopped");
        assert_eq!(panel.server_tool_count("missing"), 0);
    }
}
