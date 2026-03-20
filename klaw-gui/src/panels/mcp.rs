use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::KeyValueInput;
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore, McpServerConfig, McpServerMode};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct McpServerForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    mode: McpServerMode,
    command: String,
    args_text: String,
    env_input: KeyValueInput,
    cwd: String,
    url: String,
    headers_input: KeyValueInput,
}

impl McpServerForm {
    fn new() -> Self {
        Self {
            original_id: None,
            id: String::new(),
            enabled: true,
            mode: McpServerMode::Stdio,
            command: String::new(),
            args_text: String::new(),
            env_input: KeyValueInput::new("Env"),
            cwd: String::new(),
            url: String::new(),
            headers_input: KeyValueInput::new("Headers"),
        }
    }

    fn edit(server: &McpServerConfig) -> Self {
        Self {
            original_id: Some(server.id.clone()),
            id: server.id.clone(),
            enabled: server.enabled,
            mode: server.mode.clone(),
            command: server.command.clone().unwrap_or_default(),
            args_text: server.args.join("\n"),
            env_input: KeyValueInput::from_map("Env", &server.env),
            cwd: server.cwd.clone().unwrap_or_default(),
            url: server.url.clone().unwrap_or_default(),
            headers_input: KeyValueInput::from_map("Headers", &server.headers),
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

    fn parse_lines(value: &str) -> Vec<String> {
        value
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn to_config(&self) -> Result<McpServerConfig, String> {
        let command = self.command.trim();
        let cwd = self.cwd.trim();
        let url = self.url.trim();

        Ok(McpServerConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            mode: self.mode.clone(),
            command: (!command.is_empty()).then(|| command.to_string()),
            args: Self::parse_lines(&self.args_text),
            env: self.env_input.to_map(),
            cwd: (!cwd.is_empty()).then(|| cwd.to_string()),
            url: (!url.is_empty()).then(|| url.to_string()),
            headers: self.headers_input.to_map(),
        })
    }
}

#[derive(Default)]
pub struct McpPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<McpServerForm>,
    enabled: bool,
    startup_timeout_seconds_text: String,
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
                notifications.success("MCP config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.enabled = snapshot.config.mcp.enabled;
        self.startup_timeout_seconds_text = snapshot.config.mcp.startup_timeout_seconds.to_string();
        self.config = snapshot.config;
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
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn save_global_settings(&mut self, notifications: &mut NotificationCenter) {
        let timeout = match self.startup_timeout_seconds_text.trim().parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                notifications.error("startup_timeout_seconds must be a positive integer");
                return;
            }
        };

        let mut next = self.config.clone();
        next.mcp.enabled = self.enabled;
        next.mcp.startup_timeout_seconds = timeout;
        self.save_config(next, notifications, "MCP global settings saved");
    }

    fn open_add_server(&mut self) {
        self.form = Some(McpServerForm::new());
    }

    fn open_edit_server(&mut self, id: &str) {
        if let Some(server) = self.config.mcp.servers.iter().find(|item| item.id == id) {
            self.form = Some(McpServerForm::edit(server));
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

                        ui.label("Args (one per line)");
                        ui.add(
                            egui::TextEdit::multiline(&mut form.args_text)
                                .desired_rows(4)
                                .desired_width(f32::INFINITY),
                        );

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
}

impl PanelRenderer for McpPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.horizontal(|ui| {
            ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
            ui.label(format!("Servers: {}", self.config.mcp.servers.len()));
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.enabled, "MCP Enabled");
            ui.label("startup_timeout_seconds");
            ui.add(
                egui::TextEdit::singleline(&mut self.startup_timeout_seconds_text)
                    .desired_width(100.0),
            );
            if ui.button("Save MCP Settings").clicked() {
                self.save_global_settings(notifications);
            }
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("Add MCP Server").clicked() {
                self.open_add_server();
            }
        });

        ui.add_space(8.0);

        if self.config.mcp.servers.is_empty() {
            ui.label("No MCP servers configured.");
        } else {
            egui::Grid::new("mcp-list-grid")
                .striped(true)
                .num_columns(7)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("ID");
                    ui.strong("Enabled");
                    ui.strong("Mode");
                    ui.strong("Command/URL");
                    ui.strong("Args");
                    ui.strong("Env/Headers");
                    ui.strong("Actions");
                    ui.end_row();

                    let ids = self
                        .config
                        .mcp
                        .servers
                        .iter()
                        .map(|item| item.id.clone())
                        .collect::<Vec<_>>();

                    for id in ids {
                        let Some(server) =
                            self.config.mcp.servers.iter().find(|item| item.id == id)
                        else {
                            continue;
                        };

                        ui.label(&server.id);
                        ui.label(if server.enabled { "yes" } else { "no" });
                        ui.label(match server.mode {
                            McpServerMode::Stdio => "stdio",
                            McpServerMode::Sse => "sse",
                        });
                        let endpoint = match server.mode {
                            McpServerMode::Stdio => server.command.as_deref().unwrap_or("-"),
                            McpServerMode::Sse => server.url.as_deref().unwrap_or("-"),
                        };
                        ui.label(endpoint);
                        ui.label(server.args.len().to_string());
                        ui.label(format!(
                            "env:{} hdr:{}",
                            server.env.len(),
                            server.headers.len()
                        ));
                        if ui.button("Edit").clicked() {
                            self.open_edit_server(&id);
                        }
                        ui.end_row();
                    }
                });
        }

        self.render_form_window(ui, notifications);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
}
