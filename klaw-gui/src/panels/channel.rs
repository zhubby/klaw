use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::current_ui_language;
use crate::time_format::format_timestamp_seconds;
use crate::widgets::ArrayEditor;
use crate::{
    RuntimeRequestHandle, begin_channel_status_request, begin_restart_channel_request,
    begin_sync_channels_request,
};
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_channel::{ChannelInstanceStatus, ChannelKind, ChannelSyncResult};
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, DingtalkConfig, DingtalkProxyConfig,
    TelegramConfig, TelegramProxyConfig, WebsocketConfig,
};
use klaw_ui_kit::{LocaleDomain, Translator, label_with_hint};
use std::collections::{BTreeMap, HashMap};
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
    stream_template_id: String,
    stream_content_key: String,
    allowlist_input: ArrayEditor,
    proxy_enabled: bool,
    proxy_url: String,
}

impl DingtalkForm {
    fn new(t: &Translator) -> Self {
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
            stream_template_id: default.stream_template_id,
            stream_content_key: default.stream_content_key,
            allowlist_input: ArrayEditor::new(t.text("channel-form-allowlist")),
            proxy_enabled: default.proxy.enabled,
            proxy_url: default.proxy.url,
        }
    }

    fn edit(account: &DingtalkConfig, t: &Translator) -> Self {
        Self {
            original_id: Some(account.id.clone()),
            id: account.id.clone(),
            enabled: account.enabled,
            client_id: account.client_id.clone(),
            client_secret: account.client_secret.clone(),
            bot_title: account.bot_title.clone(),
            show_reasoning: account.show_reasoning,
            stream_output: account.stream_output,
            stream_template_id: account.stream_template_id.clone(),
            stream_content_key: account.stream_content_key.clone(),
            allowlist_input: ArrayEditor::from_vec(
                t.text("channel-form-allowlist"),
                &account.allowlist,
            ),
            proxy_enabled: account.proxy.enabled,
            proxy_url: account.proxy.url.clone(),
        }
    }

    fn title(&self) -> String {
        let t = Translator::new(LocaleDomain::Gui, current_ui_language());
        if self.original_id.is_some() {
            t.text("channel-form-title-edit-dingtalk")
        } else {
            t.text("channel-form-title-add-dingtalk")
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
            stream_template_id: self.stream_template_id.trim().to_string(),
            stream_content_key: self.stream_content_key.trim().to_string(),
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
    fn new(t: &Translator) -> Self {
        let default = TelegramConfig::default();
        Self {
            original_id: None,
            id: String::new(),
            enabled: default.enabled,
            bot_token: default.bot_token,
            show_reasoning: default.show_reasoning,
            stream_output: default.stream_output,
            allowlist_input: ArrayEditor::new(t.text("channel-form-allowlist")),
            proxy_enabled: default.proxy.enabled,
            proxy_url: default.proxy.url,
        }
    }

    fn edit(account: &TelegramConfig, t: &Translator) -> Self {
        Self {
            original_id: Some(account.id.clone()),
            id: account.id.clone(),
            enabled: account.enabled,
            bot_token: account.bot_token.clone(),
            show_reasoning: account.show_reasoning,
            stream_output: account.stream_output,
            allowlist_input: ArrayEditor::from_vec(
                t.text("channel-form-allowlist"),
                &account.allowlist,
            ),
            proxy_enabled: account.proxy.enabled,
            proxy_url: account.proxy.url.clone(),
        }
    }

    fn title(&self) -> String {
        let t = Translator::new(LocaleDomain::Gui, current_ui_language());
        if self.original_id.is_some() {
            t.text("channel-form-title-edit-telegram")
        } else {
            t.text("channel-form-title-add-telegram")
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
struct WebsocketForm {
    original_id: Option<String>,
    id: String,
    enabled: bool,
    show_reasoning: bool,
    stream_output: bool,
}

impl WebsocketForm {
    fn new() -> Self {
        let default = WebsocketConfig::default();
        Self {
            original_id: None,
            id: String::new(),
            enabled: default.enabled,
            show_reasoning: default.show_reasoning,
            stream_output: default.stream_output,
        }
    }

    fn edit(channel: &WebsocketConfig) -> Self {
        Self {
            original_id: Some(channel.id.clone()),
            id: channel.id.clone(),
            enabled: channel.enabled,
            show_reasoning: channel.show_reasoning,
            stream_output: channel.stream_output,
        }
    }

    fn title(&self) -> String {
        let t = Translator::new(LocaleDomain::Gui, current_ui_language());
        if self.original_id.is_some() {
            t.text("channel-form-title-edit-websocket")
        } else {
            t.text("channel-form-title-add-websocket")
        }
    }

    fn normalized_id(&self) -> String {
        self.id.trim().to_string()
    }

    fn to_config(&self) -> WebsocketConfig {
        WebsocketConfig {
            id: self.normalized_id(),
            enabled: self.enabled,
            show_reasoning: self.show_reasoning,
            stream_output: self.stream_output,
        }
    }
}

#[derive(Debug, Clone)]
enum ChannelForm {
    Dingtalk(DingtalkForm),
    Telegram(TelegramForm),
    Websocket(WebsocketForm),
}

impl ChannelForm {
    fn title(&self) -> String {
        match self {
            Self::Dingtalk(form) => form.title(),
            Self::Telegram(form) => form.title(),
            Self::Websocket(form) => form.title(),
        }
    }
}

#[derive(Debug, Clone)]
enum ChannelRow {
    Dingtalk(DingtalkConfig),
    Telegram(TelegramConfig),
    Websocket(WebsocketConfig),
}

impl ChannelRow {
    fn kind(&self) -> ChannelKind {
        match self {
            Self::Dingtalk(_) => ChannelKind::Dingtalk,
            Self::Telegram(_) => ChannelKind::Telegram,
            Self::Websocket(_) => ChannelKind::Websocket,
        }
    }

    fn id(&self) -> &str {
        match self {
            Self::Dingtalk(config) => &config.id,
            Self::Telegram(config) => &config.id,
            Self::Websocket(config) => &config.id,
        }
    }

    fn enabled(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.enabled,
            Self::Telegram(config) => config.enabled,
            Self::Websocket(config) => config.enabled,
        }
    }

    fn title_label(&self) -> String {
        match self {
            Self::Dingtalk(config) => config.bot_title.clone(),
            Self::Telegram(_) => "-".to_string(),
            Self::Websocket(_) => "-".to_string(),
        }
    }

    fn proxy_label(&self) -> String {
        let enabled = match self {
            Self::Dingtalk(config) => config.proxy.enabled,
            Self::Telegram(config) => config.proxy.enabled,
            Self::Websocket(_) => false,
        };
        if enabled { "on" } else { "off" }.to_string()
    }

    fn show_reasoning(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.show_reasoning,
            Self::Telegram(config) => config.show_reasoning,
            Self::Websocket(config) => config.show_reasoning,
        }
    }

    fn stream_output(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.stream_output,
            Self::Telegram(config) => config.stream_output,
            Self::Websocket(config) => config.stream_output,
        }
    }
}

fn channel_status_style(
    status: Option<&ChannelInstanceStatus>,
    t: &Translator,
) -> (&'static str, String, egui::Color32) {
    match status.map(|item| item.state) {
        Some(klaw_channel::ChannelLifecycleState::Running) => (
            regular::CHECK_CIRCLE,
            t.text("channel-status-running"),
            egui::Color32::from_rgb(0x22, 0xC5, 0x5E),
        ),
        Some(klaw_channel::ChannelLifecycleState::Degraded) => (
            regular::WARNING,
            t.text("channel-status-degraded"),
            egui::Color32::from_rgb(0xF5, 0x9E, 0x0B),
        ),
        Some(klaw_channel::ChannelLifecycleState::Reconnecting) => (
            regular::ARROW_CLOCKWISE,
            t.text("channel-status-reconnecting"),
            egui::Color32::from_rgb(0x38, 0xB, 0xDF),
        ),
        Some(klaw_channel::ChannelLifecycleState::Starting) => (
            regular::ARROW_CLOCKWISE,
            t.text("channel-status-starting"),
            egui::Color32::from_rgb(0xF5, 0x9E, 0x0B),
        ),
        Some(klaw_channel::ChannelLifecycleState::Stopped) => (
            regular::STOP_CIRCLE,
            t.text("channel-status-stopped"),
            egui::Color32::from_rgb(0x94, 0xA3, 0xB8),
        ),
        Some(klaw_channel::ChannelLifecycleState::Failed) => (
            regular::WARNING_CIRCLE,
            t.text("channel-status-failed"),
            egui::Color32::from_rgb(0xEF, 0x44, 0x44),
        ),
        None => (
            regular::QUESTION,
            t.text("channel-status-unknown"),
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
    sync_request: Option<RuntimeRequestHandle<ChannelSyncResult>>,
    sync_announce: bool,
    restart_request: Option<RuntimeRequestHandle<ChannelSyncResult>>,
    restart_target_key: Option<String>,
}

impl ChannelPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

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
        let target = self
            .restart_target_key
            .take()
            .unwrap_or_else(|| "selected channel".to_string());
        match result {
            Ok(sync_result) => {
                self.apply_runtime_statuses(&sync_result.statuses);
                notifications.success(format!("Restarted channel {}", target));
            }
            Err(err) => {
                notifications.error(format!("Failed to restart {}: {}", target, err));
            }
        }
    }

    fn poll_sync_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.sync_request.as_mut() else {
            return;
        };
        let Some(result) = request.try_take_result() else {
            return;
        };
        self.sync_request = None;
        match result {
            Ok(result) => {
                self.apply_runtime_statuses(&result.statuses);
                if self.sync_announce {
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
        self.sync_announce = false;
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
        let t = Self::translator();
        self.disable_session_commands_input = ArrayEditor::from_vec(
            t.text("channel-btn-disabled"),
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
        rows.extend(
            self.config
                .channels
                .websocket
                .iter()
                .cloned()
                .map(ChannelRow::Websocket),
        );
        rows
    }

    fn sync_channels_runtime(
        &mut self,
        notifications: &mut NotificationCenter,
        announce_success: bool,
    ) {
        if self.sync_request.is_some() {
            self.sync_announce |= announce_success;
            return;
        }
        self.sync_announce = announce_success;
        self.sync_request = Some(begin_sync_channels_request());
        if announce_success {
            notifications.info("Synchronizing channels in the background...");
        }
    }

    fn has_pending_runtime_action(&self) -> bool {
        self.sync_request.is_some() || self.restart_request.is_some()
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
        let t = Self::translator();
        self.form = Some(ChannelForm::Dingtalk(DingtalkForm::new(&t)));
    }

    fn open_add_telegram_channel(&mut self) {
        let t = Self::translator();
        self.form = Some(ChannelForm::Telegram(TelegramForm::new(&t)));
    }

    fn open_add_websocket_channel(&mut self) {
        self.form = Some(ChannelForm::Websocket(WebsocketForm::new()));
    }

    fn open_edit_channel(&mut self, kind: ChannelKind, id: &str) {
        let t = Self::translator();
        match kind {
            ChannelKind::Dingtalk => {
                if let Some(account) = self
                    .config
                    .channels
                    .dingtalk
                    .iter()
                    .find(|item| item.id == id)
                {
                    self.form = Some(ChannelForm::Dingtalk(DingtalkForm::edit(account, &t)));
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
                    self.form = Some(ChannelForm::Telegram(TelegramForm::edit(account, &t)));
                }
            }
            ChannelKind::Websocket => {
                if let Some(channel) = self
                    .config
                    .channels
                    .websocket
                    .iter()
                    .find(|item| item.id == id)
                {
                    self.form = Some(ChannelForm::Websocket(WebsocketForm::edit(channel)));
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
            ChannelKind::Websocket => {
                self.save_config(notifications, "WebSocket channel deleted", move |config| {
                    config.channels.websocket.retain(|item| item.id != id);
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
            ChannelKind::Websocket => {
                let msg = if enable {
                    "WebSocket channel enabled"
                } else {
                    "WebSocket channel disabled"
                };
                self.save_config(notifications, msg, move |config| {
                    if let Some(channel) = config
                        .channels
                        .websocket
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
            ChannelForm::Websocket(_) => "WebSocket channel saved",
        };

        if self.save_config(notifications, message, move |config| {
            let next = match &form {
                ChannelForm::Dingtalk(form) => Self::apply_dingtalk_form(config.clone(), form),
                ChannelForm::Telegram(form) => Self::apply_telegram_form(config.clone(), form),
                ChannelForm::Websocket(form) => Self::apply_websocket_form(config.clone(), form),
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

    fn apply_websocket_form(
        mut config: AppConfig,
        form: &WebsocketForm,
    ) -> Result<AppConfig, String> {
        let channel = form.to_config();
        if channel.id.is_empty() {
            return Err("Channel ID cannot be empty".to_string());
        }

        let mut replaced = false;
        if let Some(original_id) = form.original_id.as_ref() {
            for item in &mut config.channels.websocket {
                if item.id == *original_id {
                    *item = channel.clone();
                    replaced = true;
                    break;
                }
            }
        }

        if !replaced
            && config
                .channels
                .websocket
                .iter()
                .any(|item| item.id == channel.id)
        {
            return Err(format!(
                "Channel ID '{}' already exists, choose another ID",
                channel.id
            ));
        }
        if !replaced {
            config.channels.websocket.push(channel);
        }

        Ok(config)
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        let Some(form) = self.form.as_mut() else {
            return;
        };

        let t = Self::translator();

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
                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-id"),
                                    &t.text("channel-form-id-hint-dingtalk"),
                                );
                                ui.text_edit_singleline(&mut form.id);
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-enabled"),
                                    &t.text("channel-form-enabled-hint-dingtalk"),
                                );
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-client-id"),
                                    &t.text("channel-form-client-id-hint"),
                                );
                                ui.text_edit_singleline(&mut form.client_id);
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-client-secret"),
                                    &t.text("channel-form-client-secret-hint"),
                                );
                                ui.text_edit_singleline(&mut form.client_secret);
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-bot-title"),
                                    &t.text("channel-form-bot-title-hint"),
                                );
                                ui.text_edit_singleline(&mut form.bot_title);
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-show-reasoning"),
                                    &t.text("channel-form-show-reasoning-hint-dingtalk"),
                                );
                                ui.checkbox(&mut form.show_reasoning, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-stream-output"),
                                    &t.text("channel-form-stream-output-hint"),
                                );
                                ui.checkbox(&mut form.stream_output, "");
                                ui.end_row();

                                if form.stream_output {
                                    label_with_hint(
                                        ui,
                                        &t.text("channel-form-stream-template-id"),
                                        &t.text("channel-form-stream-template-id-hint"),
                                    );
                                    ui.text_edit_singleline(&mut form.stream_template_id);
                                    ui.end_row();

                                    label_with_hint(
                                        ui,
                                        &t.text("channel-form-stream-content-key"),
                                        &t.text("channel-form-stream-content-key-hint"),
                                    );
                                    ui.text_edit_singleline(&mut form.stream_content_key);
                                    ui.end_row();
                                }

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-proxy-enabled"),
                                    &t.text("channel-form-proxy-enabled-hint-dingtalk"),
                                );
                                ui.checkbox(&mut form.proxy_enabled, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-proxy-url"),
                                    &t.text("channel-form-proxy-url-hint-dingtalk"),
                                );
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
                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-id"),
                                    &t.text("channel-form-id-hint-telegram"),
                                );
                                ui.text_edit_singleline(&mut form.id);
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-enabled"),
                                    &t.text("channel-form-enabled-hint-telegram"),
                                );
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-bot-token"),
                                    &t.text("channel-form-bot-token-hint"),
                                );
                                ui.text_edit_singleline(&mut form.bot_token);
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-show-reasoning"),
                                    &t.text("channel-form-show-reasoning-hint-telegram"),
                                );
                                ui.checkbox(&mut form.show_reasoning, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-stream-output"),
                                    &t.text("channel-form-stream-output-hint"),
                                );
                                ui.checkbox(&mut form.stream_output, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-proxy-enabled"),
                                    &t.text("channel-form-proxy-enabled-hint-telegram"),
                                );
                                ui.checkbox(&mut form.proxy_enabled, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-proxy-url"),
                                    &t.text("channel-form-proxy-url-hint-telegram"),
                                );
                                ui.text_edit_singleline(&mut form.proxy_url);
                                ui.end_row();
                            });

                        ui.separator();
                        form.allowlist_input.show(ui);
                    }
                    ChannelForm::Websocket(form) => {
                        egui::Grid::new("channel-form-grid-websocket")
                            .num_columns(2)
                            .spacing([12.0, 8.0])
                            .show(ui, |ui| {
                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-id"),
                                    &t.text("channel-form-id-hint-websocket"),
                                );
                                ui.text_edit_singleline(&mut form.id);
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-enabled"),
                                    &t.text("channel-form-enabled-hint-websocket"),
                                );
                                ui.checkbox(&mut form.enabled, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-show-reasoning"),
                                    &t.text("channel-form-show-reasoning-hint-websocket"),
                                );
                                ui.checkbox(&mut form.show_reasoning, "");
                                ui.end_row();

                                label_with_hint(
                                    ui,
                                    &t.text("channel-form-stream-output"),
                                    &t.text("channel-form-stream-output-hint"),
                                );
                                ui.checkbox(&mut form.stream_output, "");
                                ui.end_row();
                            });
                    }
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t.text("channel-form-save")).clicked() {
                        save_clicked = true;
                    }
                    if ui.button(t.text("channel-form-cancel")).clicked() {
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

        let t = Self::translator();

        egui::Window::new(t.text("channel-disabled-title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(400.0);
                self.disable_session_commands_input.show(ui);

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t.text("channel-disabled-save")).clicked() {
                        save_clicked = true;
                    }
                    if ui.button(t.text("channel-disabled-cancel")).clicked() {
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

        let t = Self::translator();
        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(t.text_args(
            "channel-delete-title",
            HashMap::from([("kind", kind.as_str().to_string())]),
        ))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(t.text_args(
                    "channel-delete-message",
                    HashMap::from([("id", id.clone())]),
                ))
                .strong(),
            );
            ui.add_space(8.0);
            ui.label(t.text("channel-delete-info"));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        RichText::new(t.text_args(
                            "channel-delete-btn",
                            HashMap::from([("icon", regular::TRASH.to_string())]),
                        ))
                        .color(ui.visuals().warn_fg_color),
                    ))
                    .clicked()
                {
                    confirmed = true;
                }
                if ui.button(t.text("channel-delete-cancel")).clicked() {
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
        self.poll_sync_request(notifications);
        self.refresh_runtime_status();
        self.poll_restart_request(notifications);
        if self.has_pending_runtime_action() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        let rows = self.all_rows();

        let t = Self::translator();

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            ui.label(t.text("channel-subtitle"));
            ui.add_space(4.0);
            if self.restart_request.is_some() {
                ui.label(RichText::new(t.text("channel-restarting")).small());
            }
            if self.sync_request.is_some() {
                ui.label(RichText::new(t.text("channel-synchronizing")).small());
            }
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui
                .button(t.text_args(
                    "channel-btn-disabled",
                    HashMap::from([("icon", regular::WRENCH.to_string())]),
                ))
                .clicked()
            {
                self.show_disabled_dialog = true;
            }
            if ui
                .button(t.text_args(
                    "channel-btn-add-dingtalk",
                    HashMap::from([("icon", regular::CHAT_CIRCLE_DOTS.to_string())]),
                ))
                .clicked()
            {
                self.open_add_dingtalk_channel();
            }
            if ui
                .button(t.text_args(
                    "channel-btn-add-telegram",
                    HashMap::from([("icon", regular::PAPER_PLANE.to_string())]),
                ))
                .clicked()
            {
                self.open_add_telegram_channel();
            }
            if ui
                .button(t.text_args(
                    "channel-btn-add-websocket",
                    HashMap::from([("icon", regular::BROADCAST.to_string())]),
                ))
                .clicked()
            {
                self.open_add_websocket_channel();
            }
            if ui
                .button(t.text_args(
                    "channel-btn-reload",
                    HashMap::from([("icon", regular::ARROW_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.reload(notifications);
            }
            if ui
                .button(t.text_args(
                    "channel-btn-refresh-status",
                    HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.last_runtime_status_at = None;
                self.refresh_runtime_status();
            }
        });

        ui.add_space(8.0);

        if rows.is_empty() {
            ui.label(t.text("channel-no-channels"));
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
                                ui.strong(t.text("channel-col-type"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-id"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-enabled"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-status"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-last-activity"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-reconnect"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-title"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-reasoning"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-stream"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("channel-col-proxy"));
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
                                    let text = if enabled {
                                        t.text("channel-yes")
                                    } else {
                                        t.text("channel-no")
                                    };
                                    ui.label(format!("{} {}", icon, text));
                                });
                                row.col(|ui| {
                                    let (icon, label, color) = channel_status_style(status, &t);
                                    let status_response =
                                        ui.colored_label(color, format!("{} {}", icon, label));
                                    if let Some(s) = status {
                                        let mut hover_lines = Vec::new();
                                        if let Some(event) = s.last_event.as_deref() {
                                            hover_lines.push(t.text_args(
                                                "channel-hover-last-event",
                                                HashMap::from([("event", event.to_string())]),
                                            ));
                                        }
                                        if let Some(error) = s.last_error.as_deref() {
                                            hover_lines.push(t.text_args(
                                                "channel-hover-last-error",
                                                HashMap::from([("error", error.to_string())]),
                                            ));
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
                                    ui.label(if show {
                                        t.text("channel-yes")
                                    } else {
                                        t.text("channel-no")
                                    });
                                });
                                row.col(|ui| {
                                    let stream = channel_row.stream_output();
                                    ui.label(if stream {
                                        t.text("channel-yes")
                                    } else {
                                        t.text("channel-no")
                                    });
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
                                        .button(t.text_args(
                                            "channel-ctx-edit",
                                            HashMap::from([(
                                                "icon",
                                                regular::PENCIL_SIMPLE.to_string(),
                                            )]),
                                        ))
                                        .clicked()
                                    {
                                        edit_channel = Some((kind, id.clone()));
                                        ui.close();
                                    }
                                    if ui
                                        .button(t.text_args(
                                            "channel-ctx-restart",
                                            HashMap::from([(
                                                "icon",
                                                regular::ARROW_CLOCKWISE.to_string(),
                                            )]),
                                        ))
                                        .clicked()
                                    {
                                        restart_channel = Some((kind, id.clone()));
                                        ui.close();
                                    }
                                    if ui
                                        .button(t.text_args(
                                            if enabled {
                                                "channel-ctx-disable"
                                            } else {
                                                "channel-ctx-enable"
                                            },
                                            HashMap::from([("icon", regular::POWER.to_string())]),
                                        ))
                                        .clicked()
                                    {
                                        toggle_channel = Some((kind, id.clone(), !enabled));
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .add(egui::Button::new(
                                            RichText::new(t.text_args(
                                                "channel-ctx-delete",
                                                HashMap::from([(
                                                    "icon",
                                                    regular::TRASH.to_string(),
                                                )]),
                                            ))
                                            .color(ui.visuals().warn_fg_color),
                                        ))
                                        .clicked()
                                    {
                                        self.delete_confirm = Some((kind, id.clone()));
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(t.text_args(
                                            "channel-ctx-copy-id",
                                            HashMap::from([("icon", regular::COPY.to_string())]),
                                        ))
                                        .clicked()
                                    {
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
    use klaw_ui_kit::UiLanguage;

    fn test_translator() -> Translator {
        Translator::new(LocaleDomain::Gui, UiLanguage::English)
    }

    #[test]
    fn apply_dingtalk_form_adds_new_channel() {
        let t = test_translator();
        let config = AppConfig::default();
        let mut form = DingtalkForm::new(&t);
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

        let t = test_translator();
        let mut form = DingtalkForm::new(&t);
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
        let t = test_translator();
        let mut form = DingtalkForm::edit(&source, &t);
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
        let t = test_translator();
        let mut form = DingtalkForm::new(&t);
        form.id = "ops".to_string();
        form.client_id = "cid".to_string();
        form.client_secret = "secret".to_string();
        form.stream_output = true;
        form.stream_template_id = "template-1.schema".to_string();
        form.stream_content_key = "content".to_string();

        let updated = ChannelPanel::apply_dingtalk_form(config, &form).expect("should apply");

        assert!(updated.channels.dingtalk[0].stream_output);
        assert_eq!(
            updated.channels.dingtalk[0].stream_template_id,
            "template-1.schema"
        );
        assert_eq!(updated.channels.dingtalk[0].stream_content_key, "content");
    }

    #[test]
    fn apply_telegram_form_adds_new_channel() {
        let t = test_translator();
        let config = AppConfig::default();
        let mut form = TelegramForm::new(&t);
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

        let t = test_translator();
        let mut form = TelegramForm::new(&t);
        form.id = "ops-bot".to_string();

        let err =
            ChannelPanel::apply_telegram_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_telegram_form_preserves_stream_flag() {
        let config = AppConfig::default();
        let t = test_translator();
        let mut form = TelegramForm::new(&t);
        form.id = "ops-bot".to_string();
        form.bot_token = "123:secret".to_string();
        form.stream_output = true;

        let updated = ChannelPanel::apply_telegram_form(config, &form).expect("should apply");

        assert!(updated.channels.telegram[0].stream_output);
    }

    #[test]
    fn apply_websocket_form_adds_new_channel() {
        let config = AppConfig::default();
        let mut form = WebsocketForm::new();
        form.id = "browser".to_string();
        form.show_reasoning = true;

        let updated = ChannelPanel::apply_websocket_form(config, &form).expect("should apply");

        assert!(
            updated
                .channels
                .websocket
                .iter()
                .any(|item| item.id == "browser")
        );
        assert!(updated.channels.websocket[0].show_reasoning);
    }

    #[test]
    fn apply_websocket_form_rejects_duplicate_id() {
        let mut config = AppConfig::default();
        config.channels.websocket.push(WebsocketConfig {
            id: "browser".to_string(),
            ..WebsocketConfig::default()
        });

        let mut form = WebsocketForm::new();
        form.id = "browser".to_string();

        let err =
            ChannelPanel::apply_websocket_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_websocket_form_preserves_stream_flag() {
        let config = AppConfig::default();
        let mut form = WebsocketForm::new();
        form.id = "browser".to_string();
        form.stream_output = false;

        let updated = ChannelPanel::apply_websocket_form(config, &form).expect("should apply");

        assert!(!updated.channels.websocket[0].stream_output);
    }
}
