use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_seconds;
use crate::widgets::ArrayEditor;
use crate::{
    RuntimeRequestHandle, begin_channel_status_request, begin_restart_channel_request,
    request_sync_channels,
};
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_channel::{ChannelInstanceStatus, ChannelKind, ChannelSyncResult};
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, DingtalkConfig, DingtalkProxyConfig,
    TelegramConfig, TelegramProxyConfig,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const CHANNEL_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
struct DingtalkForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    client_id: String,
    client_secret: String,
    bot_title: String,
    show_reasoning: bool,
    stream_output: bool,
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
            stream_output: default.stream_output,
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
            stream_output: account.stream_output,
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
            stream_output: self.stream_output,
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
    stream_output: bool,
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
            stream_output: default.stream_output,
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
            stream_output: account.stream_output,
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
            stream_output: self.stream_output,
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

    fn title_label(&self) -> String {
        match self {
            Self::Dingtalk(config) => config.bot_title.clone(),
            Self::Telegram(_) => "-".to_string(),
        }
    }

    fn proxy_label(&self) -> String {
        let enabled = match self {
            Self::Dingtalk(config) => config.proxy.enabled,
            Self::Telegram(config) => config.proxy.enabled,
        };
        if enabled { "on" } else { "off" }.to_string()
    }

    fn show_reasoning(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.show_reasoning,
            Self::Telegram(config) => config.show_reasoning,
        }
    }

    fn stream_output(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.stream_output,
            Self::Telegram(config) => config.stream_output,
        }
    }
}

fn channel_status_style(
    status: Option<&ChannelInstanceStatus>,
) -> (&'static str, &'static str, egui::Color32) {
    match status.map(|item| item.state) {
        Some(klaw_channel::ChannelLifecycleState::Running) => (
            regular::CHECK_CIRCLE,
            "running",
            egui::Color32::from_rgb(0x22, 0xC5, 0x5E),
        ),
        Some(klaw_channel::ChannelLifecycleState::Degraded) => (
            regular::WARNING,
            "degraded",
            egui::Color32::from_rgb(0xF5, 0x9E, 0x0B),
        ),
        Some(klaw_channel::ChannelLifecycleState::Reconnecting) => (
            regular::ARROW_CLOCKWISE,
            "reconnecting",
            egui::Color32::from_rgb(0x38, 0xB, 0xDF),
        ),
        Some(klaw_channel::ChannelLifecycleState::Starting) => (
            regular::ARROW_CLOCKWISE,
            "starting",
            egui::Color32::from_rgb(0xF5, 0x9E, 0x0B),
        ),
        Some(klaw_channel::ChannelLifecycleState::Stopped) => (
            regular::STOP_CIRCLE,
            "stopped",
            egui::Color32::from_rgb(0x94, 0xA3, 0xB8),
        ),
        Some(klaw_channel::ChannelLifecycleState::Failed) => (
            regular::WARNING_CIRCLE,
            "failed",
            egui::Color32::from_rgb(0xEF, 0x44, 0x44),
        ),
        None => (
            regular::QUESTION,
            "unknown",
            egui::Color32::from_rgb(0x94, 0xA3, 0xB8),
        ),
    }
}

#[derive(Default)]
pub struct ChannelPanel {
    store: Option<ConfigStore>,
    config: AppConfig,
    form: Option<ChannelForm>,
    show_disabled_dialog: bool,
    disable_session_commands_input: ArrayEditor,
    statuses: BTreeMap<String, ChannelInstanceStatus>,
    selected_channel: Option<(ChannelKind, String)>,
    delete_confirm: Option<(ChannelKind, String)>,
    last_runtime_status_at: Option<Instant>,
    runtime_status_request: Option<RuntimeRequestHandle<Vec<ChannelInstanceStatus>>>,
    restart_request: Option<RuntimeRequestHandle<ChannelSyncResult>>,
    restart_target_key: Option<String>,
}

impl ChannelPanel {
    fn refresh_runtime_status(&mut self) {
        if let Some(request) = self.runtime_status_request.as_mut()
            && let Some(result) = request.try_take_result()
        {
            self.runtime_status_request = None;
            if let Ok(statuses) = result {
                self.apply_runtime_statuses(&statuses);
            }
        }
        let should_refresh = self
            .last_runtime_status_at
            .is_none_or(|last| last.elapsed() >= CHANNEL_STATUS_POLL_INTERVAL);
        if !should_refresh || self.runtime_status_request.is_some() {
            return;
        }
        self.last_runtime_status_at = Some(Instant::now());
        self.runtime_status_request = Some(begin_channel_status_request());
    }

    fn poll_restart_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.restart_request.as_mut() else {
            return;
        };
        let Some(result) = request.try_take_result() else {
            return;
        };
        self.restart_request = None;
        match result {
            Ok(sync_result) => {
                self.apply_runtime_statuses(&sync_result.statuses);
                let target = self
                    .restart_target_key
                    .take()
                    .unwrap_or_else(|| "selected channel".to_string());
                notifications.success(format!("Restarted channel {}", target));
            }
            Err(err) => {
                let target = self
                    .restart_target_key
                    .take()
                    .unwrap_or_else(|| "selected channel".to_string());
                notifications.error(format!("Failed to restart {}: {}", target, err));
            }
        }
    }

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
        self.disable_session_commands_input = ArrayEditor::from_vec(
            "Disable Session Commands For",
            &snapshot.config.channels.disable_session_commands_for,
        );
        self.config = snapshot.config;
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

    fn restart_channel(
        &mut self,
        kind: ChannelKind,
        id: &str,
        notifications: &mut NotificationCenter,
    ) {
        if self.restart_request.is_some() {
            notifications.info("A channel restart is already in progress");
            return;
        }
        let key = Self::instance_key(kind, id);
        self.restart_target_key = Some(key.clone());
        self.restart_request = Some(begin_restart_channel_request(key));
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
                notifications.success(success_message);
                self.sync_channels_runtime(notifications, true);
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
        let values = self.disable_session_commands_input.to_vec();
        self.save_config(
            notifications,
            "Updated disable_session_commands_for",
            move |config| {
                config.channels.disable_session_commands_for = values;
                Ok(())
            },
        );
    }

    fn delete_channel(
        &mut self,
        kind: ChannelKind,
        id: &str,
        notifications: &mut NotificationCenter,
    ) {
        let id = id.to_string();
        match kind {
            ChannelKind::Dingtalk => {
                self.save_config(notifications, "Dingtalk channel deleted", move |config| {
                    config.channels.dingtalk.retain(|item| item.id != id);
                    Ok(())
                });
            }
            ChannelKind::Telegram => {
                self.save_config(notifications, "Telegram channel deleted", move |config| {
                    config.channels.telegram.retain(|item| item.id != id);
                    Ok(())
                });
            }
            ChannelKind::Feishu => {}
        }
    }

    fn toggle_channel(
        &mut self,
        kind: ChannelKind,
        id: &str,
        enable: bool,
        notifications: &mut NotificationCenter,
    ) {
        let id = id.to_string();
        match kind {
            ChannelKind::Dingtalk => {
                let msg = if enable {
                    "Dingtalk channel enabled"
                } else {
                    "Dingtalk channel disabled"
                };
                self.save_config(notifications, msg, move |config| {
                    if let Some(channel) = config
                        .channels
                        .dingtalk
                        .iter_mut()
                        .find(|item| item.id == id)
                    {
                        channel.enabled = enable;
                    }
                    Ok(())
                });
            }
            ChannelKind::Telegram => {
                let msg = if enable {
                    "Telegram channel enabled"
                } else {
                    "Telegram channel disabled"
                };
                self.save_config(notifications, msg, move |config| {
                    if let Some(channel) = config
                        .channels
                        .telegram
                        .iter_mut()
                        .find(|item| item.id == id)
                    {
                        channel.enabled = enable;
                    }
                    Ok(())
                });
            }
            ChannelKind::Feishu => {}
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.clone() else {
            return;
        };
        let message = match &form {
            ChannelForm::Dingtalk(_) => "Dingtalk channel saved",
            ChannelForm::Telegram(_) => "Telegram channel saved",
        };

        if self.save_config(notifications, message, move |config| {
            let next = match &form {
                ChannelForm::Dingtalk(form) => Self::apply_dingtalk_form(config.clone(), form),
                ChannelForm::Telegram(form) => Self::apply_telegram_form(config.clone(), form),
            }?;
            *config = next;
            Ok(())
        }) {
            self.form = None;
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

                                ui.label("Stream Output");
                                ui.checkbox(&mut form.stream_output, "");
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

                                ui.label("Stream Output");
                                ui.checkbox(&mut form.stream_output, "");
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

    fn render_delete_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some((kind, id)) = self.delete_confirm.clone() else {
            return;
        };

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(format!("Delete {} Channel", kind.as_str()))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("Are you sure you want to delete channel '{}'?", id))
                        .strong(),
                );
                ui.add_space(8.0);
                ui.label("This action cannot be undone.");
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
            self.delete_channel(kind, &id, notifications);
            self.delete_confirm = None;
            if self.selected_channel == Some((kind, id)) {
                self.selected_channel = None;
            }
        }
        if cancelled {
            self.delete_confirm = None;
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
        self.refresh_runtime_status();
        self.poll_restart_request(notifications);

        let rows = self.all_rows();

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            ui.label(format!("Channel instances: {}", rows.len()));
            if self.restart_request.is_some() {
                ui.label("Restarting channel...");
            }
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Set Disabled Channels", regular::WRENCH))
                .clicked()
            {
                self.show_disabled_dialog = true;
            }
            if ui
                .button(format!("{} Add Dingtalk", regular::CHAT_CIRCLE_DOTS))
                .clicked()
            {
                self.open_add_dingtalk_channel();
            }
            if ui
                .button(format!("{} Add Telegram", regular::PAPER_PLANE))
                .clicked()
            {
                self.open_add_telegram_channel();
            }
            if ui
                .button(format!("{} Reload", regular::ARROW_CLOCKWISE))
                .clicked()
            {
                self.reload(notifications);
            }
            if ui
                .button(format!("{} Refresh Status", regular::ARROWS_CLOCKWISE))
                .clicked()
            {
                self.last_runtime_status_at = None;
                self.refresh_runtime_status();
            }
        });

        ui.add_space(8.0);

        if rows.is_empty() {
            ui.label("No channels configured.");
        } else {
            let table_width = ui.available_width();
            let mut edit_channel: Option<(ChannelKind, String)> = None;
            let mut toggle_channel: Option<(ChannelKind, String, bool)> = None;
            let mut restart_channel: Option<(ChannelKind, String)> = None;

            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .max_width(table_width)
                .show(ui, |ui| {
                    ui.set_min_width(table_width);
                    let available_height = ui.available_height();
                    TableBuilder::new(ui)
                        .striped(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(130.0))
                        .column(Column::auto().at_least(85.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(70.0))
                        .column(Column::auto().at_least(70.0))
                        .column(Column::remainder().at_least(70.0))
                        .min_scrolled_height(0.0)
                        .max_scroll_height(available_height)
                        .sense(egui::Sense::click())
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Type");
                            });
                            header.col(|ui| {
                                ui.strong("ID");
                            });
                            header.col(|ui| {
                                ui.strong("Enabled");
                            });
                            header.col(|ui| {
                                ui.strong("Status");
                            });
                            header.col(|ui| {
                                ui.strong("Last Activity");
                            });
                            header.col(|ui| {
                                ui.strong("Reconnect");
                            });
                            header.col(|ui| {
                                ui.strong("Title");
                            });
                            header.col(|ui| {
                                ui.strong("Reasoning");
                            });
                            header.col(|ui| {
                                ui.strong("Stream");
                            });
                            header.col(|ui| {
                                ui.strong("Proxy");
                            });
                        })
                        .body(|body| {
                            body.rows(20.0, rows.len(), |mut row| {
                                let idx = row.index();
                                let channel_row = &rows[idx];
                                let kind = channel_row.kind();
                                let id = channel_row.id().to_string();
                                let key = Self::instance_key(kind, &id);
                                let status = self.statuses.get(&key);
                                let is_selected =
                                    self.selected_channel.as_ref() == Some(&(kind, id.clone()));

                                row.set_selected(is_selected);

                                row.col(|ui| {
                                    ui.label(kind.as_str());
                                });
                                row.col(|ui| {
                                    ui.label(&id);
                                });
                                row.col(|ui| {
                                    let enabled = channel_row.enabled();
                                    let icon = if enabled {
                                        regular::CHECK_CIRCLE
                                    } else {
                                        regular::CIRCLE
                                    };
                                    let text = if enabled { "yes" } else { "no" };
                                    ui.label(format!("{} {}", icon, text));
                                });
                                row.col(|ui| {
                                    let (icon, label, color) = channel_status_style(status);
                                    let status_response =
                                        ui.colored_label(color, format!("{} {}", icon, label));
                                    if let Some(s) = status {
                                        let mut hover_lines = Vec::new();
                                        if let Some(event) = s.last_event.as_deref() {
                                            hover_lines.push(format!("last event: {event}"));
                                        }
                                        if let Some(error) = s.last_error.as_deref() {
                                            hover_lines.push(format!("last error: {error}"));
                                        }
                                        if !hover_lines.is_empty() {
                                            status_response.on_hover_text(hover_lines.join("\n"));
                                        }
                                    }
                                });
                                row.col(|ui| {
                                    let label = status
                                        .and_then(|status| status.last_activity_at_unix_seconds)
                                        .map(format_timestamp_seconds)
                                        .unwrap_or_else(|| "-".to_string());
                                    ui.label(label);
                                });
                                row.col(|ui| {
                                    let reconnect_label = status
                                        .map(|status| status.reconnect_attempt.to_string())
                                        .unwrap_or_else(|| "-".to_string());
                                    ui.label(reconnect_label);
                                });
                                row.col(|ui| {
                                    ui.label(channel_row.title_label());
                                });
                                row.col(|ui| {
                                    let show = channel_row.show_reasoning();
                                    ui.label(if show { "yes" } else { "no" });
                                });
                                row.col(|ui| {
                                    let stream = channel_row.stream_output();
                                    ui.label(if stream { "yes" } else { "no" });
                                });
                                row.col(|ui| {
                                    ui.label(channel_row.proxy_label());
                                });

                                let response = row.response();

                                if response.clicked() {
                                    self.selected_channel = if is_selected {
                                        None
                                    } else {
                                        Some((kind, id.clone()))
                                    };
                                }

                                response.context_menu(|ui| {
                                    let enabled = channel_row.enabled();
                                    if ui
                                        .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                        .clicked()
                                    {
                                        edit_channel = Some((kind, id.clone()));
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Restart", regular::ARROW_CLOCKWISE))
                                        .clicked()
                                    {
                                        restart_channel = Some((kind, id.clone()));
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!(
                                            "{} {}",
                                            if enabled {
                                                regular::POWER
                                            } else {
                                                regular::POWER
                                            },
                                            if enabled { "Disable" } else { "Enable" }
                                        ))
                                        .clicked()
                                    {
                                        toggle_channel = Some((kind, id.clone(), !enabled));
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .add(egui::Button::new(
                                            RichText::new(format!("{} Delete", regular::TRASH))
                                                .color(ui.visuals().warn_fg_color),
                                        ))
                                        .clicked()
                                    {
                                        self.delete_confirm = Some((kind, id.clone()));
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button(format!("{} Copy ID", regular::COPY)).clicked() {
                                        ui.ctx().output_mut(|o| {
                                            o.commands
                                                .push(egui::OutputCommand::CopyText(id.clone()));
                                        });
                                        ui.close();
                                    }
                                });
                            });
                        });
                });

            if let Some((kind, id)) = edit_channel {
                self.open_edit_channel(kind, &id);
            }
            if let Some((kind, id, enable)) = toggle_channel {
                self.toggle_channel(kind, &id, enable, notifications);
            }
            if let Some((kind, id)) = restart_channel {
                self.restart_channel(kind, &id, notifications);
            }
        }

        self.render_form_window(ui, notifications);
        if self.show_disabled_dialog {
            self.render_disabled_dialog(ui, notifications);
        }
        self.render_delete_confirm_dialog(ui.ctx(), notifications);
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

        assert!(
            updated
                .channels
                .dingtalk
                .iter()
                .any(|item| item.id == "ops")
        );
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
    fn apply_dingtalk_form_preserves_stream_flag() {
        let config = AppConfig::default();
        let mut form = DingtalkForm::new();
        form.id = "ops".to_string();
        form.client_id = "cid".to_string();
        form.client_secret = "secret".to_string();
        form.stream_output = true;

        let updated = ChannelPanel::apply_dingtalk_form(config, &form).expect("should apply");

        assert!(updated.channels.dingtalk[0].stream_output);
    }

    #[test]
    fn apply_telegram_form_adds_new_channel() {
        let config = AppConfig::default();
        let mut form = TelegramForm::new();
        form.id = "ops-bot".to_string();
        form.bot_token = "123:secret".to_string();

        let updated = ChannelPanel::apply_telegram_form(config, &form).expect("should apply");

        assert!(
            updated
                .channels
                .telegram
                .iter()
                .any(|item| item.id == "ops-bot")
        );
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

    #[test]
    fn apply_telegram_form_preserves_stream_flag() {
        let config = AppConfig::default();
        let mut form = TelegramForm::new();
        form.id = "ops-bot".to_string();
        form.bot_token = "123:secret".to_string();
        form.stream_output = true;

        let updated = ChannelPanel::apply_telegram_form(config, &form).expect("should apply");

        assert!(updated.channels.telegram[0].stream_output);
    }
}
