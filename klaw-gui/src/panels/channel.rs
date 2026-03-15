use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore, DingtalkConfig, DingtalkProxyConfig};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct DingtalkForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    client_id: String,
    client_secret: String,
    bot_title: String,
    show_reasoning: bool,
    allowlist_text: String,
    proxy_enabled: bool,
    proxy_url: String,
}

impl DingtalkForm {
    fn new() -> Self {
        let default = DingtalkConfig::default();
        Self {
            original_id: None,
            id: String::new(),
            enabled: default.enabled,
            client_id: default.client_id,
            client_secret: default.client_secret,
            bot_title: default.bot_title,
            show_reasoning: default.show_reasoning,
            allowlist_text: String::new(),
            proxy_enabled: default.proxy.enabled,
            proxy_url: default.proxy.url,
        }
    }

    fn edit(account: &DingtalkConfig) -> Self {
        Self {
            original_id: Some(account.id.clone()),
            id: account.id.clone(),
            enabled: account.enabled,
            client_id: account.client_id.clone(),
            client_secret: account.client_secret.clone(),
            bot_title: account.bot_title.clone(),
            show_reasoning: account.show_reasoning,
            allowlist_text: account.allowlist.join("\n"),
            proxy_enabled: account.proxy.enabled,
            proxy_url: account.proxy.url.clone(),
        }
    }

    fn title(&self) -> &'static str {
        if self.original_id.is_some() {
            "Edit Dingtalk Channel"
        } else {
            "Add Dingtalk Channel"
        }
    }

    fn normalized_id(&self) -> String {
        self.id.trim().to_string()
    }

    fn allowlist(&self) -> Vec<String> {
        self.allowlist_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn to_config(&self) -> DingtalkConfig {
        DingtalkConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            client_id: self.client_id.trim().to_string(),
            client_secret: self.client_secret.trim().to_string(),
            bot_title: self.bot_title.trim().to_string(),
            show_reasoning: self.show_reasoning,
            allowlist: self.allowlist(),
            proxy: DingtalkProxyConfig {
                enabled: self.proxy_enabled,
                url: self.proxy_url.trim().to_string(),
            },
        }
    }
}

#[derive(Default)]
pub struct ChannelPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<DingtalkForm>,
    disable_session_commands_text: String,
}

impl ChannelPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                notifications.success("Channel config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.disable_session_commands_text = snapshot
            .config
            .channels
            .disable_session_commands_for
            .join("\n");
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

    fn open_add_channel(&mut self) {
        self.form = Some(DingtalkForm::new());
    }

    fn open_edit_channel(&mut self, id: &str) {
        if let Some(account) = self
            .config
            .channels
            .dingtalk
            .iter()
            .find(|item| item.id == id)
        {
            self.form = Some(DingtalkForm::edit(account));
        }
    }

    fn save_disable_session_commands(&mut self, notifications: &mut NotificationCenter) {
        let mut next = self.config.clone();
        next.channels.disable_session_commands_for = self
            .disable_session_commands_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        self.save_config(next, notifications, "Updated disable_session_commands_for");
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        match Self::apply_form(self.config.clone(), form) {
            Ok(next) => {
                self.save_config(next, notifications, "Dingtalk channel saved");
                self.form = None;
            }
            Err(err) => notifications.error(err),
        }
    }

    fn apply_form(mut config: AppConfig, form: &DingtalkForm) -> Result<AppConfig, String> {
        let account = form.to_config();
        if account.id.is_empty() {
            return Err("Channel ID cannot be empty".to_string());
        }

        let mut replaced = false;
        if let Some(original_id) = form.original_id.as_ref() {
            for item in &mut config.channels.dingtalk {
                if item.id == *original_id {
                    *item = account.clone();
                    replaced = true;
                    break;
                }
            }
        }

        if !replaced {
            let exists = config
                .channels
                .dingtalk
                .iter()
                .any(|item| item.id == account.id);
            if exists {
                return Err(format!(
                    "Channel ID '{}' already exists, choose another ID",
                    account.id
                ));
            }
            config.channels.dingtalk.push(account);
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
                ui.set_min_width(520.0);
                egui::Grid::new("channel-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("ID");
                        ui.text_edit_singleline(&mut form.id);
                        ui.end_row();

                        ui.label("Enabled");
                        ui.checkbox(&mut form.enabled, "");
                        ui.end_row();

                        ui.label("Client ID");
                        ui.text_edit_singleline(&mut form.client_id);
                        ui.end_row();

                        ui.label("Client Secret");
                        ui.text_edit_singleline(&mut form.client_secret);
                        ui.end_row();

                        ui.label("Bot Title");
                        ui.text_edit_singleline(&mut form.bot_title);
                        ui.end_row();

                        ui.label("Show Reasoning");
                        ui.checkbox(&mut form.show_reasoning, "");
                        ui.end_row();

                        ui.label("Proxy Enabled");
                        ui.checkbox(&mut form.proxy_enabled, "");
                        ui.end_row();

                        ui.label("Proxy URL");
                        ui.text_edit_singleline(&mut form.proxy_url);
                        ui.end_row();
                    });

                ui.separator();
                ui.label("Allowlist (one entry per line)");
                ui.add(
                    egui::TextEdit::multiline(&mut form.allowlist_text)
                        .desired_rows(5)
                        .desired_width(f32::INFINITY),
                );

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

impl PanelRenderer for ChannelPanel {
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
            ui.label(format!(
                "Dingtalk accounts: {}",
                self.config.channels.dingtalk.len()
            ));
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Add Dingtalk").clicked() {
                self.open_add_channel();
            }
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });

        ui.add_space(8.0);

        if self.config.channels.dingtalk.is_empty() {
            ui.label("No Dingtalk channels configured.");
        } else {
            egui::Grid::new("channel-list-grid")
                .striped(true)
                .num_columns(7)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("ID");
                    ui.strong("Enabled");
                    ui.strong("Bot Title");
                    ui.strong("Client ID");
                    ui.strong("Reasoning");
                    ui.strong("Proxy");
                    ui.strong("Actions");
                    ui.end_row();

                    let ids = self
                        .config
                        .channels
                        .dingtalk
                        .iter()
                        .map(|item| item.id.clone())
                        .collect::<Vec<_>>();

                    for id in ids {
                        let Some(account) = self
                            .config
                            .channels
                            .dingtalk
                            .iter()
                            .find(|item| item.id == id)
                        else {
                            continue;
                        };

                        ui.label(&account.id);
                        ui.label(if account.enabled { "yes" } else { "no" });
                        ui.label(&account.bot_title);
                        ui.label(&account.client_id);
                        ui.label(if account.show_reasoning { "yes" } else { "no" });
                        let proxy = if account.proxy.enabled {
                            if account.proxy.url.trim().is_empty() {
                                "enabled".to_string()
                            } else {
                                account.proxy.url.clone()
                            }
                        } else {
                            "disabled".to_string()
                        };
                        ui.label(proxy);
                        if ui.button("Edit").clicked() {
                            self.open_edit_channel(&id);
                        }
                        ui.end_row();
                    }
                });
        }

        ui.separator();
        ui.label("disable_session_commands_for (one channel per line)");
        ui.add(
            egui::TextEdit::multiline(&mut self.disable_session_commands_text)
                .desired_rows(4)
                .desired_width(f32::INFINITY),
        );
        if ui.button("Save Disabled Channel List").clicked() {
            self.save_disable_session_commands(notifications);
        }

        self.render_form_window(ui, notifications);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_form_adds_new_channel() {
        let config = AppConfig::default();
        let mut form = DingtalkForm::new();
        form.id = "ops".to_string();
        form.client_id = "client".to_string();
        form.client_secret = "secret".to_string();
        form.bot_title = "OpsBot".to_string();

        let updated = ChannelPanel::apply_form(config, &form).expect("should apply");

        assert!(updated
            .channels
            .dingtalk
            .iter()
            .any(|item| item.id == "ops"));
    }

    #[test]
    fn apply_form_rejects_duplicate_id() {
        let mut config = AppConfig::default();
        config.channels.dingtalk.push(DingtalkConfig {
            id: "ops".to_string(),
            ..DingtalkConfig::default()
        });

        let mut form = DingtalkForm::new();
        form.id = "ops".to_string();

        let err = ChannelPanel::apply_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_form_edits_existing_channel() {
        let mut config = AppConfig::default();
        config.channels.dingtalk.push(DingtalkConfig {
            id: "ops".to_string(),
            bot_title: "Old".to_string(),
            ..DingtalkConfig::default()
        });

        let source = config
            .channels
            .dingtalk
            .iter()
            .find(|item| item.id == "ops")
            .expect("channel should exist")
            .clone();
        let mut form = DingtalkForm::edit(&source);
        form.bot_title = "New".to_string();

        let updated = ChannelPanel::apply_form(config, &form).expect("should apply");

        let item = updated
            .channels
            .dingtalk
            .iter()
            .find(|entry| entry.id == "ops")
            .expect("channel should exist after edit");
        assert_eq!(item.bot_title, "New");
    }
}
