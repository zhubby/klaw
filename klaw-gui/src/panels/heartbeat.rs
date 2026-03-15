use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_config::{
    AppConfig, ConfigSnapshot, ConfigStore, HeartbeatDefaultsConfig, HeartbeatSessionConfig,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionEnabledMode {
    InheritDefault,
    Enabled,
    Disabled,
}

impl SessionEnabledMode {
    fn from_option(value: Option<bool>) -> Self {
        match value {
            Some(true) => Self::Enabled,
            Some(false) => Self::Disabled,
            None => Self::InheritDefault,
        }
    }

    fn to_option(self) -> Option<bool> {
        match self {
            Self::InheritDefault => None,
            Self::Enabled => Some(true),
            Self::Disabled => Some(false),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::InheritDefault => "Inherit Default",
            Self::Enabled => "Enabled",
            Self::Disabled => "Disabled",
        }
    }
}

#[derive(Debug, Clone)]
struct HeartbeatSessionForm {
    original_session_key: Option<String>,
    session_key: String,
    channel: String,
    chat_id: String,
    enabled_mode: SessionEnabledMode,
    every: String,
    prompt: String,
    silent_ack_token: String,
    timezone: String,
}

impl HeartbeatSessionForm {
    fn new() -> Self {
        Self {
            original_session_key: None,
            session_key: String::new(),
            channel: String::new(),
            chat_id: String::new(),
            enabled_mode: SessionEnabledMode::InheritDefault,
            every: String::new(),
            prompt: String::new(),
            silent_ack_token: String::new(),
            timezone: String::new(),
        }
    }

    fn edit(session: &HeartbeatSessionConfig) -> Self {
        Self {
            original_session_key: Some(session.session_key.clone()),
            session_key: session.session_key.clone(),
            channel: session.channel.clone(),
            chat_id: session.chat_id.clone(),
            enabled_mode: SessionEnabledMode::from_option(session.enabled),
            every: session.every.clone().unwrap_or_default(),
            prompt: session.prompt.clone().unwrap_or_default(),
            silent_ack_token: session.silent_ack_token.clone().unwrap_or_default(),
            timezone: session.timezone.clone().unwrap_or_default(),
        }
    }

    fn title(&self) -> &'static str {
        if self.original_session_key.is_some() {
            "Edit Heartbeat Session"
        } else {
            "Add Heartbeat Session"
        }
    }

    fn to_session_config(&self) -> HeartbeatSessionConfig {
        HeartbeatSessionConfig {
            session_key: self.session_key.trim().to_string(),
            chat_id: self.chat_id.trim().to_string(),
            channel: self.channel.trim().to_string(),
            enabled: self.enabled_mode.to_option(),
            every: optional_trimmed(&self.every),
            prompt: optional_trimmed(&self.prompt),
            silent_ack_token: optional_trimmed(&self.silent_ack_token),
            timezone: optional_trimmed(&self.timezone),
        }
    }
}

#[derive(Default)]
pub struct HeartbeatPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    defaults_every: String,
    defaults_prompt: String,
    defaults_silent_ack_token: String,
    defaults_timezone: String,
    defaults_enabled: bool,
    form: Option<HeartbeatSessionForm>,
    delete_confirm_key: Option<String>,
}

impl HeartbeatPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                notifications.success("Heartbeat config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.defaults_every = snapshot.config.heartbeat.defaults.every.clone();
        self.defaults_prompt = snapshot.config.heartbeat.defaults.prompt.clone();
        self.defaults_silent_ack_token =
            snapshot.config.heartbeat.defaults.silent_ack_token.clone();
        self.defaults_timezone = snapshot.config.heartbeat.defaults.timezone.clone();
        self.defaults_enabled = snapshot.config.heartbeat.defaults.enabled;
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

    fn save_defaults(&mut self, notifications: &mut NotificationCenter) {
        let every = self.defaults_every.trim().to_string();
        let prompt = self.defaults_prompt.trim().to_string();
        let token = self.defaults_silent_ack_token.trim().to_string();
        let timezone = self.defaults_timezone.trim().to_string();

        if every.is_empty() {
            notifications.error("heartbeat.defaults.every cannot be empty");
            return;
        }
        if prompt.is_empty() {
            notifications.error("heartbeat.defaults.prompt cannot be empty");
            return;
        }
        if token.is_empty() {
            notifications.error("heartbeat.defaults.silent_ack_token cannot be empty");
            return;
        }
        if timezone.is_empty() {
            notifications.error("heartbeat.defaults.timezone cannot be empty");
            return;
        }

        let mut next = self.config.clone();
        next.heartbeat.defaults = HeartbeatDefaultsConfig {
            enabled: self.defaults_enabled,
            every,
            prompt,
            silent_ack_token: token,
            timezone,
        };
        self.save_config(next, notifications, "heartbeat.defaults saved");
    }

    fn open_add_session(&mut self) {
        self.form = Some(HeartbeatSessionForm::new());
    }

    fn open_edit_session(&mut self, session_key: &str) {
        if let Some(session) = self
            .config
            .heartbeat
            .sessions
            .iter()
            .find(|item| item.session_key == session_key)
        {
            self.form = Some(HeartbeatSessionForm::edit(session));
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        match Self::apply_form(self.config.clone(), form) {
            Ok(next) => {
                self.save_config(next, notifications, "Heartbeat session saved");
                self.form = None;
            }
            Err(err) => notifications.error(err),
        }
    }

    fn apply_form(mut config: AppConfig, form: &HeartbeatSessionForm) -> Result<AppConfig, String> {
        let session = form.to_session_config();
        if session.session_key.is_empty() {
            return Err("Session key cannot be empty".to_string());
        }
        if session.channel.is_empty() {
            return Err("Channel cannot be empty".to_string());
        }
        if session.chat_id.is_empty() {
            return Err("Chat ID cannot be empty".to_string());
        }

        let mut replaced = false;
        if let Some(original_session_key) = form.original_session_key.as_ref() {
            for item in &mut config.heartbeat.sessions {
                if item.session_key == *original_session_key {
                    *item = session.clone();
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                return Err(format!("Session '{}' was not found", original_session_key));
            }
        }

        if !replaced {
            let duplicate = config
                .heartbeat
                .sessions
                .iter()
                .any(|item| item.session_key == session.session_key);
            if duplicate {
                return Err(format!(
                    "Session key '{}' already exists",
                    session.session_key
                ));
            }
            config.heartbeat.sessions.push(session);
        }

        Ok(config)
    }

    fn remove_session(&mut self, session_key: &str, notifications: &mut NotificationCenter) {
        let mut next = self.config.clone();
        let before = next.heartbeat.sessions.len();
        next.heartbeat
            .sessions
            .retain(|item| item.session_key != session_key);

        if next.heartbeat.sessions.len() == before {
            notifications.error(format!("Session '{}' was not found", session_key));
            return;
        }

        self.save_config(next, notifications, "Heartbeat session deleted");
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
                ui.set_min_width(620.0);
                egui::Grid::new("heartbeat-session-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Session Key");
                        ui.text_edit_singleline(&mut form.session_key);
                        ui.end_row();

                        ui.label("Channel");
                        ui.text_edit_singleline(&mut form.channel);
                        ui.end_row();

                        ui.label("Chat ID");
                        ui.text_edit_singleline(&mut form.chat_id);
                        ui.end_row();

                        ui.label("Enabled");
                        egui::ComboBox::from_id_salt("heartbeat-enabled-mode")
                            .selected_text(form.enabled_mode.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut form.enabled_mode,
                                    SessionEnabledMode::InheritDefault,
                                    SessionEnabledMode::InheritDefault.label(),
                                );
                                ui.selectable_value(
                                    &mut form.enabled_mode,
                                    SessionEnabledMode::Enabled,
                                    SessionEnabledMode::Enabled.label(),
                                );
                                ui.selectable_value(
                                    &mut form.enabled_mode,
                                    SessionEnabledMode::Disabled,
                                    SessionEnabledMode::Disabled.label(),
                                );
                            });
                        ui.end_row();

                        ui.label("Every");
                        ui.text_edit_singleline(&mut form.every);
                        ui.end_row();

                        ui.label("Timezone");
                        ui.text_edit_singleline(&mut form.timezone);
                        ui.end_row();

                        ui.label("Silent Ack Token");
                        ui.text_edit_singleline(&mut form.silent_ack_token);
                        ui.end_row();
                    });

                ui.separator();
                ui.label("Prompt (leave empty to inherit defaults.prompt)");
                ui.add(
                    egui::TextEdit::multiline(&mut form.prompt)
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );

                ui.separator();
                ui.small(
                    "Optional fields (every/timezone/silent_ack_token/prompt) use defaults when blank.",
                );
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

impl PanelRenderer for HeartbeatPanel {
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
                "Sessions: {}",
                self.config.heartbeat.sessions.len()
            ));
        });
        ui.separator();

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.strong("Defaults");
            ui.add_space(6.0);
            egui::Grid::new("heartbeat-defaults-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Enabled");
                    ui.checkbox(&mut self.defaults_enabled, "");
                    ui.end_row();

                    ui.label("Every");
                    ui.text_edit_singleline(&mut self.defaults_every);
                    ui.end_row();

                    ui.label("Timezone");
                    ui.text_edit_singleline(&mut self.defaults_timezone);
                    ui.end_row();

                    ui.label("Silent Ack Token");
                    ui.text_edit_singleline(&mut self.defaults_silent_ack_token);
                    ui.end_row();
                });

            ui.add_space(6.0);
            ui.label("Prompt");
            ui.add(
                egui::TextEdit::multiline(&mut self.defaults_prompt)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save Defaults").clicked() {
                    self.save_defaults(notifications);
                }
                if ui.button("Reload").clicked() {
                    self.reload(notifications);
                }
            });
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Add Session").clicked() {
                self.open_add_session();
            }
        });

        ui.add_space(8.0);
        if self.config.heartbeat.sessions.is_empty() {
            ui.label("No heartbeat sessions configured.");
        } else {
            egui::Grid::new("heartbeat-session-list-grid")
                .striped(true)
                .num_columns(8)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("Session Key");
                    ui.strong("Channel");
                    ui.strong("Chat ID");
                    ui.strong("Enabled");
                    ui.strong("Every");
                    ui.strong("Timezone");
                    ui.strong("Overrides");
                    ui.strong("Actions");
                    ui.end_row();

                    let keys = self
                        .config
                        .heartbeat
                        .sessions
                        .iter()
                        .map(|item| item.session_key.clone())
                        .collect::<Vec<_>>();

                    for key in keys {
                        let Some(session) = self
                            .config
                            .heartbeat
                            .sessions
                            .iter()
                            .find(|item| item.session_key == key)
                        else {
                            continue;
                        };

                        let resolved_enabled = session
                            .enabled
                            .unwrap_or(self.config.heartbeat.defaults.enabled);
                        let resolved_every = session
                            .every
                            .as_deref()
                            .unwrap_or(&self.config.heartbeat.defaults.every);
                        let resolved_timezone = session
                            .timezone
                            .as_deref()
                            .unwrap_or(&self.config.heartbeat.defaults.timezone);
                        let override_count = usize::from(session.enabled.is_some())
                            + usize::from(session.every.is_some())
                            + usize::from(session.prompt.is_some())
                            + usize::from(session.silent_ack_token.is_some())
                            + usize::from(session.timezone.is_some());

                        ui.label(&session.session_key);
                        ui.label(&session.channel);
                        ui.label(&session.chat_id);
                        ui.label(if resolved_enabled {
                            "enabled"
                        } else {
                            "disabled"
                        });
                        ui.monospace(resolved_every);
                        ui.monospace(resolved_timezone);
                        ui.label(format!("{override_count}/5"));
                        ui.horizontal(|ui| {
                            if ui.button("Edit").clicked() {
                                self.open_edit_session(&key);
                            }
                            if ui.button("Delete").clicked() {
                                self.delete_confirm_key = Some(key.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
        }

        if let Some(session_key) = self.delete_confirm_key.clone() {
            let mut delete_clicked = false;
            let mut cancel_clicked = false;
            egui::Window::new("Delete Heartbeat Session")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Delete heartbeat session '{}' ?", session_key));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            delete_clicked = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_clicked = true;
                        }
                    });
                });

            if delete_clicked {
                self.remove_session(&session_key, notifications);
                self.delete_confirm_key = None;
            }
            if cancel_clicked {
                self.delete_confirm_key = None;
            }
        }

        self.render_form_window(ui, notifications);
    }
}

fn optional_trimmed(input: &str) -> Option<String> {
    let value = input.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_form_adds_new_session() {
        let config = AppConfig::default();
        let mut form = HeartbeatSessionForm::new();
        form.session_key = "stdio:main".to_string();
        form.channel = "stdio".to_string();
        form.chat_id = "main".to_string();
        form.enabled_mode = SessionEnabledMode::Enabled;
        form.every = "10m".to_string();

        let updated = HeartbeatPanel::apply_form(config, &form).expect("should apply");
        assert_eq!(updated.heartbeat.sessions.len(), 1);
        assert_eq!(updated.heartbeat.sessions[0].session_key, "stdio:main");
        assert_eq!(updated.heartbeat.sessions[0].enabled, Some(true));
    }

    #[test]
    fn apply_form_rejects_duplicate_session_key() {
        let mut config = AppConfig::default();
        config.heartbeat.sessions.push(HeartbeatSessionConfig {
            session_key: "stdio:dup".to_string(),
            chat_id: "dup".to_string(),
            channel: "stdio".to_string(),
            enabled: None,
            every: None,
            prompt: None,
            silent_ack_token: None,
            timezone: None,
        });

        let mut form = HeartbeatSessionForm::new();
        form.session_key = "stdio:dup".to_string();
        form.channel = "stdio".to_string();
        form.chat_id = "main".to_string();

        let err = HeartbeatPanel::apply_form(config, &form).expect_err("duplicate should fail");
        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_form_edits_existing_session_and_allows_inherit() {
        let mut config = AppConfig::default();
        config.heartbeat.sessions.push(HeartbeatSessionConfig {
            session_key: "stdio:main".to_string(),
            chat_id: "main".to_string(),
            channel: "stdio".to_string(),
            enabled: Some(true),
            every: Some("10m".to_string()),
            prompt: None,
            silent_ack_token: None,
            timezone: None,
        });

        let mut form = HeartbeatSessionForm::edit(&config.heartbeat.sessions[0]);
        form.enabled_mode = SessionEnabledMode::InheritDefault;
        form.every.clear();

        let updated = HeartbeatPanel::apply_form(config, &form).expect("edit should apply");
        let edited = &updated.heartbeat.sessions[0];
        assert_eq!(edited.enabled, None);
        assert_eq!(edited.every, None);
    }
}
