use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::request_sync_channels;
use crate::widgets::ArrayEditor;
use klaw_channel::{ChannelInstanceStatus, ChannelKind};
use klaw_config::{
    AppConfig, ConfigSnapshot, ConfigStore, DingtalkConfig, DingtalkProxyConfig, TelegramConfig,
    TelegramProxyConfig,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
struct DingtalkForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    client_id: String,
    client_secret: String,
    bot_title: String,
    show_reasoning: bool,
    allowlist_input: ArrayEditor,
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
            allowlist_input: ArrayEditor::new("Allowlist"),
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
            allowlist_input: ArrayEditor::from_vec("Allowlist", &account.allowlist),
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

    fn to_config(&self) -> DingtalkConfig {
        DingtalkConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            client_id: self.client_id.trim().to_string(),
            client_secret: self.client_secret.trim().to_string(),
            bot_title: self.bot_title.trim().to_string(),
            show_reasoning: self.show_reasoning,
            allowlist: self.allowlist_input.to_vec(),
            proxy: DingtalkProxyConfig {
                enabled: self.proxy_enabled,
                url: self.proxy_url.trim().to_string(),
            },
        }
    }
}

#[derive(Debug, Clone)]
struct TelegramForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    bot_token: String,
    show_reasoning: bool,
    allowlist_input: ArrayEditor,
    proxy_enabled: bool,
    proxy_url: String,
}

impl TelegramForm {
    fn new() -> Self {
        let default = TelegramConfig::default();
        Self {
            original_id: None,
            id: String::new(),
            enabled: default.enabled,
            bot_token: default.bot_token,
            show_reasoning: default.show_reasoning,
            allowlist_input: ArrayEditor::new("Allowlist"),
            proxy_enabled: default.proxy.enabled,
            proxy_url: default.proxy.url,
        }
    }

    fn edit(account: &TelegramConfig) -> Self {
        Self {
            original_id: Some(account.id.clone()),
            id: account.id.clone(),
            enabled: account.enabled,
            bot_token: account.bot_token.clone(),
            show_reasoning: account.show_reasoning,
            allowlist_input: ArrayEditor::from_vec("Allowlist", &account.allowlist),
            proxy_enabled: account.proxy.enabled,
            proxy_url: account.proxy.url.clone(),
        }
    }

    fn title(&self) -> &'static str {
        if self.original_id.is_some() {
            "Edit Telegram Channel"
        } else {
            "Add Telegram Channel"
        }
    }

    fn normalized_id(&self) -> String {
        self.id.trim().to_string()
    }

    fn to_config(&self) -> TelegramConfig {
        TelegramConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            bot_token: self.bot_token.trim().to_string(),
            show_reasoning: self.show_reasoning,
            allowlist: self.allowlist_input.to_vec(),
            proxy: TelegramProxyConfig {
                enabled: self.proxy_enabled,
                url: self.proxy_url.trim().to_string(),
            },
        }
    }
}

#[derive(Debug, Clone)]
enum ChannelForm {
    Dingtalk(DingtalkForm),
    Telegram(TelegramForm),
}

impl ChannelForm {
    fn title(&self) -> &'static str {
        match self {
            Self::Dingtalk(form) => form.title(),
            Self::Telegram(form) => form.title(),
        }
    }
}

#[derive(Debug, Clone)]
enum ChannelRow {
    Dingtalk(DingtalkConfig),
    Telegram(TelegramConfig),
}

impl ChannelRow {
    fn kind(&self) -> ChannelKind {
        match self {
            Self::Dingtalk(_) => ChannelKind::Dingtalk,
            Self::Telegram(_) => ChannelKind::Telegram,
        }
    }

    fn id(&self) -> &str {
        match self {
            Self::Dingtalk(config) => &config.id,
            Self::Telegram(config) => &config.id,
        }
    }

    fn enabled(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.enabled,
            Self::Telegram(config) => config.enabled,
        }
    }

    fn auth_label(&self) -> String {
        match self {
            Self::Dingtalk(config) => config.client_id.clone(),
            Self::Telegram(config) => config.bot_token.clone(),
        }
    }

    fn title_label(&self) -> String {
        match self {
            Self::Dingtalk(config) => config.bot_title.clone(),
            Self::Telegram(_) => "-".to_string(),
        }
    }

    fn proxy_label(&self) -> String {
        let (enabled, url) = match self {
            Self::Dingtalk(config) => (config.proxy.enabled, config.proxy.url.as_str()),
            Self::Telegram(config) => (config.proxy.enabled, config.proxy.url.as_str()),
        };
        if enabled {
            if url.trim().is_empty() {
                "enabled".to_string()
            } else {
                url.to_string()
            }
        } else {
            "disabled".to_string()
        }
    }

    fn show_reasoning(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.show_reasoning,
            Self::Telegram(config) => config.show_reasoning,
        }
    }
}

#[derive(Default)]
pub struct ChannelPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<ChannelForm>,
    show_disabled_dialog: bool,
    disable_session_commands_input: ArrayEditor,
    statuses: BTreeMap<String, ChannelInstanceStatus>,
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
                self.sync_channels_runtime(notifications, false);
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.disable_session_commands_input = ArrayEditor::from_vec(
            "Disable Session Commands For",
            &snapshot.config.channels.disable_session_commands_for,
        );
        self.config = snapshot.config;
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Path: {}", path.display()),
            None => "Path: (not loaded)".to_string(),
        }
    }

    fn instance_key(kind: ChannelKind, id: &str) -> String {
        format!("{}:{}", kind.as_str(), id)
    }

    fn all_rows(&self) -> Vec<ChannelRow> {
        let mut rows = self
            .config
            .channels
            .dingtalk
            .iter()
            .cloned()
            .map(ChannelRow::Dingtalk)
            .collect::<Vec<_>>();
        rows.extend(
            self.config
                .channels
                .telegram
                .iter()
                .cloned()
                .map(ChannelRow::Telegram),
        );
        rows
    }

    fn sync_channels_runtime(
        &mut self,
        notifications: &mut NotificationCenter,
        announce_success: bool,
    ) {
        match request_sync_channels() {
            Ok(result) => {
                self.apply_runtime_statuses(&result.statuses);
                if announce_success {
                    notifications.success(format!(
                        "Channels synchronized (keep: {}, start: {}, restart: {}, stop: {})",
                        result.keep.len(),
                        result.start.len(),
                        result.restart.len(),
                        result.stop.len()
                    ));
                }
            }
            Err(err) => notifications.error(format!(
                "Saved config but failed to synchronize channels: {err}"
            )),
        }
    }

    fn apply_runtime_statuses(&mut self, statuses: &[ChannelInstanceStatus]) {
        self.statuses = statuses
            .iter()
            .cloned()
            .map(|status| (status.key.as_str().to_string(), status))
            .collect();
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
                    self.sync_channels_runtime(notifications, true);
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
                self.sync_channels_runtime(notifications, true);
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn open_add_dingtalk_channel(&mut self) {
        self.form = Some(ChannelForm::Dingtalk(DingtalkForm::new()));
    }

    fn open_add_telegram_channel(&mut self) {
        self.form = Some(ChannelForm::Telegram(TelegramForm::new()));
    }

    fn open_edit_channel(&mut self, kind: ChannelKind, id: &str) {
        match kind {
            ChannelKind::Dingtalk => {
                if let Some(account) = self
                    .config
                    .channels
                    .dingtalk
                    .iter()
                    .find(|item| item.id == id)
                {
                    self.form = Some(ChannelForm::Dingtalk(DingtalkForm::edit(account)));
                }
            }
            ChannelKind::Telegram => {
                if let Some(account) = self
                    .config
                    .channels
                    .telegram
                    .iter()
                    .find(|item| item.id == id)
                {
                    self.form = Some(ChannelForm::Telegram(TelegramForm::edit(account)));
                }
            }
            ChannelKind::Feishu => {}
        }
    }

    fn save_disable_session_commands(&mut self, notifications: &mut NotificationCenter) {
        let mut next = self.config.clone();
        next.channels.disable_session_commands_for = self.disable_session_commands_input.to_vec();
        self.save_config(next, notifications, "Updated disable_session_commands_for");
    }

    fn delete_channel(
        &mut self,
        kind: ChannelKind,
        id: &str,
        notifications: &mut NotificationCenter,
    ) {
        let mut next = self.config.clone();
        match kind {
            ChannelKind::Dingtalk => {
                next.channels.dingtalk.retain(|item| item.id != id);
                self.save_config(next, notifications, "Dingtalk channel deleted");
            }
            ChannelKind::Telegram => {
                next.channels.telegram.retain(|item| item.id != id);
                self.save_config(next, notifications, "Telegram channel deleted");
            }
            ChannelKind::Feishu => {}
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let result = match form {
            ChannelForm::Dingtalk(form) => Self::apply_dingtalk_form(self.config.clone(), form)
                .map(|next| (next, "Dingtalk channel saved")),
            ChannelForm::Telegram(form) => Self::apply_telegram_form(self.config.clone(), form)
                .map(|next| (next, "Telegram channel saved")),
        };

        match result {
            Ok((next, message)) => {
                self.save_config(next, notifications, message);
                self.form = None;
            }
            Err(err) => notifications.error(err),
        }
    }

    fn apply_dingtalk_form(
        mut config: AppConfig,
        form: &DingtalkForm,
    ) -> Result<AppConfig, String> {
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

        if !replaced
            && config
                .channels
                .dingtalk
                .iter()
                .any(|item| item.id == account.id)
        {
            return Err(format!(
                "Channel ID '{}' already exists, choose another ID",
                account.id
            ));
        }
        if !replaced {
            config.channels.dingtalk.push(account);
        }

        Ok(config)
    }

    fn apply_telegram_form(
        mut config: AppConfig,
        form: &TelegramForm,
    ) -> Result<AppConfig, String> {
        let account = form.to_config();
        if account.id.is_empty() {
            return Err("Channel ID cannot be empty".to_string());
        }

        let mut replaced = false;
        if let Some(original_id) = form.original_id.as_ref() {
            for item in &mut config.channels.telegram {
                if item.id == *original_id {
                    *item = account.clone();
                    replaced = true;
                    break;
                }
            }
        }

        if !replaced
            && config
                .channels
                .telegram
                .iter()
                .any(|item| item.id == account.id)
        {
            return Err(format!(
                "Channel ID '{}' already exists, choose another ID",
                account.id
            ));
        }
        if !replaced {
            config.channels.telegram.push(account);
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
                match form {
                    ChannelForm::Dingtalk(form) => {
                        egui::Grid::new("channel-form-grid-dingtalk")
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
                        form.allowlist_input.show(ui);
                    }
                    ChannelForm::Telegram(form) => {
                        egui::Grid::new("channel-form-grid-telegram")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("ID");
                                ui.text_edit_singleline(&mut form.id);
                                ui.end_row();

                                ui.label("Enabled");
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                ui.label("Bot Token");
                                ui.text_edit_singleline(&mut form.bot_token);
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
                        form.allowlist_input.show(ui);
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

    fn render_disabled_dialog(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new("Set Disabled Channels")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(400.0);
                self.disable_session_commands_input.show(ui);

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
            self.save_disable_session_commands(notifications);
            self.show_disabled_dialog = false;
        }
        if cancel_clicked {
            self.show_disabled_dialog = false;
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

        let rows = self.all_rows();

        ui.heading(ctx.tab_title);
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.horizontal(|ui| {
            ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
            ui.label(format!("Channel instances: {}", rows.len()));
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Set Disabled Channels").clicked() {
                self.show_disabled_dialog = true;
            }
            if ui.button("Add Dingtalk").clicked() {
                self.open_add_dingtalk_channel();
            }
            if ui.button("Add Telegram").clicked() {
                self.open_add_telegram_channel();
            }
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });

        ui.add_space(8.0);

        if rows.is_empty() {
            ui.label("No channels configured.");
        } else {
            egui::Grid::new("channel-list-grid")
                .striped(true)
                .num_columns(9)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("Type");
                    ui.strong("ID");
                    ui.strong("Enabled");
                    ui.strong("Status");
                    ui.strong("Title");
                    ui.strong("Auth");
                    ui.strong("Reasoning");
                    ui.strong("Proxy");
                    ui.strong("Actions");
                    ui.end_row();

                    for row in rows {
                        let kind = row.kind();
                        let id = row.id().to_string();
                        let key = Self::instance_key(kind, &id);
                        let status = self.statuses.get(&key);
                        let status_label = status
                            .map(|status| status.state.as_str().to_string())
                            .unwrap_or_else(|| "unknown".to_string());

                        ui.label(kind.as_str());
                        ui.label(&id);
                        ui.label(if row.enabled() { "yes" } else { "no" });
                        let status_response = ui.label(&status_label);
                        if let Some(status) = status {
                            if let Some(error) = status.last_error.as_deref() {
                                status_response.on_hover_text(error);
                            }
                        }
                        ui.label(row.title_label());
                        ui.label(row.auth_label());
                        ui.label(if row.show_reasoning() { "yes" } else { "no" });
                        ui.label(row.proxy_label());
                        ui.horizontal(|ui| {
                            if ui.button("Edit").clicked() {
                                self.open_edit_channel(kind, &id);
                            }
                            if ui.button("Delete").clicked() {
                                self.delete_channel(kind, &id, notifications);
                            }
                        });
                        ui.end_row();
                    }
                });
        }

        self.render_form_window(ui, notifications);
        if self.show_disabled_dialog {
            self.render_disabled_dialog(ui, notifications);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_dingtalk_form_adds_new_channel() {
        let config = AppConfig::default();
        let mut form = DingtalkForm::new();
        form.id = "ops".to_string();
        form.client_id = "client".to_string();
        form.client_secret = "secret".to_string();
        form.bot_title = "OpsBot".to_string();

        let updated = ChannelPanel::apply_dingtalk_form(config, &form).expect("should apply");

        assert!(updated
            .channels
            .dingtalk
            .iter()
            .any(|item| item.id == "ops"));
    }

    #[test]
    fn apply_dingtalk_form_rejects_duplicate_id() {
        let mut config = AppConfig::default();
        config.channels.dingtalk.push(DingtalkConfig {
            id: "ops".to_string(),
            ..DingtalkConfig::default()
        });

        let mut form = DingtalkForm::new();
        form.id = "ops".to_string();

        let err =
            ChannelPanel::apply_dingtalk_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_dingtalk_form_edits_existing_channel() {
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

        let updated = ChannelPanel::apply_dingtalk_form(config, &form).expect("should apply");

        let item = updated
            .channels
            .dingtalk
            .iter()
            .find(|entry| entry.id == "ops")
            .expect("channel should exist after edit");
        assert_eq!(item.bot_title, "New");
    }

    #[test]
    fn apply_telegram_form_adds_new_channel() {
        let config = AppConfig::default();
        let mut form = TelegramForm::new();
        form.id = "ops-bot".to_string();
        form.bot_token = "123:secret".to_string();

        let updated = ChannelPanel::apply_telegram_form(config, &form).expect("should apply");

        assert!(updated
            .channels
            .telegram
            .iter()
            .any(|item| item.id == "ops-bot"));
    }

    #[test]
    fn apply_telegram_form_rejects_duplicate_id() {
        let mut config = AppConfig::default();
        config.channels.telegram.push(TelegramConfig {
            id: "ops-bot".to_string(),
            ..TelegramConfig::default()
        });

        let mut form = TelegramForm::new();
        form.id = "ops-bot".to_string();

        let err =
            ChannelPanel::apply_telegram_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }
}
