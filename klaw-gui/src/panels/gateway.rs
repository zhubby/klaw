use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_seconds;
use crate::{
    request_gateway_status, request_restart_gateway, request_set_tailscale_mode,
    GatewayStatusSnapshot,
};
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore, GatewayConfig, TailscaleMode};
use klaw_gateway::TailscaleStatus;
use std::path::PathBuf;
use std::time::Duration;

const GATEWAY_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
struct GatewayConfigForm {
    enabled: bool,
    listen_ip: String,
    listen_port: String,
    auth_enabled: bool,
    auth_token: String,
    auth_env_key: String,
    webhook_enabled: bool,
    webhook_path: String,
    webhook_max_body_bytes: String,
}

impl Default for GatewayConfigForm {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_ip: "127.0.0.1".to_string(),
            listen_port: "0".to_string(),
            auth_enabled: false,
            auth_token: String::new(),
            auth_env_key: String::new(),
            webhook_enabled: false,
            webhook_path: "/webhook/events".to_string(),
            webhook_max_body_bytes: "262144".to_string(),
        }
    }
}

impl GatewayConfigForm {
    fn from_config(config: &GatewayConfig) -> Self {
        Self {
            enabled: config.enabled,
            listen_ip: config.listen_ip.clone(),
            listen_port: config.listen_port.to_string(),
            auth_enabled: config.auth.enabled,
            auth_token: config.auth.token.clone().unwrap_or_default(),
            auth_env_key: config.auth.env_key.clone().unwrap_or_default(),
            webhook_enabled: config.webhook.enabled,
            webhook_path: config.webhook.path.clone(),
            webhook_max_body_bytes: config.webhook.max_body_bytes.to_string(),
        }
    }

    fn apply_to_config(&self, config: &mut AppConfig) -> Result<(), String> {
        let listen_ip = self.listen_ip.trim();
        if listen_ip.is_empty() {
            return Err("listen IP cannot be empty".to_string());
        }

        let listen_port = self
            .listen_port
            .trim()
            .parse::<u16>()
            .map_err(|_| "listen port must be a valid number (0-65535)".to_string())?;

        let webhook_path = self.webhook_path.trim();
        if webhook_path.is_empty() {
            return Err("webhook path cannot be empty".to_string());
        }

        let webhook_max_body_bytes = self
            .webhook_max_body_bytes
            .trim()
            .parse::<usize>()
            .map_err(|_| "webhook max body bytes must be a valid integer".to_string())?;

        config.gateway.enabled = self.enabled;
        config.gateway.listen_ip = listen_ip.to_string();
        config.gateway.listen_port = listen_port;
        config.gateway.auth.enabled = self.auth_enabled;
        config.gateway.auth.token = if self.auth_token.trim().is_empty() {
            None
        } else {
            Some(self.auth_token.trim().to_string())
        };
        config.gateway.auth.env_key = if self.auth_env_key.trim().is_empty() {
            None
        } else {
            Some(self.auth_env_key.trim().to_string())
        };
        config.gateway.webhook.enabled = self.webhook_enabled;
        config.gateway.webhook.path = webhook_path.to_string();
        config.gateway.webhook.max_body_bytes = webhook_max_body_bytes;

        Ok(())
    }
}

pub struct GatewayPanel {
    status: Option<GatewayStatusSnapshot>,
    loaded: bool,
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    config: AppConfig,
    config_form: GatewayConfigForm,
    config_window_open: bool,
}

impl Default for GatewayPanel {
    fn default() -> Self {
        Self {
            status: None,
            loaded: false,
            store: None,
            config_path: None,
            config: AppConfig::default(),
            config_form: GatewayConfigForm::default(),
            config_window_open: false,
        }
    }
}

impl GatewayPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        self.ensure_store_loaded(notifications);
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.refresh(notifications, false);
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
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.config = snapshot.config;
        self.config_form = GatewayConfigForm::from_config(&self.config.gateway);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter, announce: bool) {
        match request_gateway_status() {
            Ok(status) => {
                self.status = Some(status);
                if announce {
                    notifications.success("Gateway status refreshed");
                }
            }
            Err(err) => notifications.error(format!("Failed to load gateway status: {err}")),
        }
    }

    fn open_config_window(&mut self) {
        self.config_form = GatewayConfigForm::from_config(&self.config.gateway);
        self.config_window_open = true;
    }

    fn save_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };

        let mut next = self.config.clone();
        if let Err(err) = self.config_form.apply_to_config(&mut next) {
            notifications.error(err);
            return;
        }

        match toml::to_string_pretty(&next) {
            Ok(raw) => match store.save_raw_toml(&raw) {
                Ok(snapshot) => {
                    self.apply_snapshot(snapshot);
                    self.config_window_open = false;
                    let running = self.status.as_ref().map(|s| s.running).unwrap_or(false);
                    if running {
                        notifications
                            .success("Gateway config saved. Restart gateway to apply changes.");
                    } else {
                        notifications.success("Gateway config saved");
                    }
                }
                Err(err) => notifications.error(format!("Save failed: {err}")),
            },
            Err(err) => notifications.error(format!("Failed to render config TOML: {err}")),
        }
    }

    fn reload_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success("Config reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn restart(&mut self, notifications: &mut NotificationCenter) {
        match request_restart_gateway() {
            Ok(status) => {
                let message = status
                    .info
                    .as_ref()
                    .map(|info| format!("Gateway restarted at {}", info.ws_url))
                    .unwrap_or_else(|| "Gateway restarted".to_string());
                self.status = Some(status);
                notifications.success(message);
            }
            Err(err) => {
                notifications.error(format!("Failed to restart gateway: {err}"));
                self.refresh(notifications, false);
            }
        }
    }

    fn set_tailscale_mode(&mut self, mode: TailscaleMode, notifications: &mut NotificationCenter) {
        match request_set_tailscale_mode(mode) {
            Ok(status) => {
                let mode_str = match mode {
                    TailscaleMode::Off => "disabled",
                    TailscaleMode::Serve => "serve (tailnet only)",
                    TailscaleMode::Funnel => "funnel (public)",
                };
                self.status = Some(status);
                notifications.success(format!("Tailscale mode set to {}", mode_str));
            }
            Err(err) => {
                notifications.error(format!("Failed to set tailscale mode: {err}"));
                self.refresh(notifications, false);
            }
        }
    }

    fn render_config_window(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let mut open = self.config_window_open;
        egui::Window::new("Gateway Config")
            .id(egui::Id::new("gateway-config-window"))
            .open(&mut open)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.heading("Basic");
                ui.horizontal(|ui| {
                    ui.label("Enabled");
                    ui.checkbox(&mut self.config_form.enabled, "");
                });

                ui.horizontal(|ui| {
                    ui.label("Listen IP");
                    ui.add_sized(
                        [200.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.config_form.listen_ip),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Listen Port");
                    ui.add_sized(
                        [100.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.config_form.listen_port),
                    );
                    ui.label("(0 = auto)");
                });

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Auth");
                ui.horizontal(|ui| {
                    ui.label("Enabled");
                    ui.checkbox(&mut self.config_form.auth_enabled, "");
                });
                ui.horizontal(|ui| {
                    ui.label("Token");
                    ui.add_sized(
                        [280.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.config_form.auth_token).password(true),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Env Key");
                    ui.add_sized(
                        [200.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.config_form.auth_env_key),
                    );
                });

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Webhook");
                ui.horizontal(|ui| {
                    ui.label("Enabled");
                    ui.checkbox(&mut self.config_form.webhook_enabled, "");
                });
                ui.horizontal(|ui| {
                    ui.label("Path");
                    ui.add_sized(
                        [280.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.config_form.webhook_path),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Max Body Bytes");
                    ui.add_sized(
                        [120.0, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.config_form.webhook_max_body_bytes),
                    );
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Reload").clicked() {
                        self.reload_config(notifications);
                    }
                    if ui.button("Save").clicked() {
                        self.save_config(notifications);
                    }
                });
            });
        self.config_window_open = open;
    }
}

impl PanelRenderer for GatewayPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.label("Manage the embedded gateway service used by the GUI runtime.");
        ui.separator();

        let Some(status) = self.status.clone() else {
            ui.label("Loading...");
            return;
        };

        if status.transitioning {
            ui.ctx().request_repaint_after(GATEWAY_POLL_INTERVAL);
        }

        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications, true);
            }

            if ui.button("Config").clicked() {
                self.open_config_window();
            }

            if ui
                .add_enabled(
                    !status.transitioning && status.running,
                    egui::Button::new("Restart"),
                )
                .clicked()
            {
                self.restart(notifications);
            }
        });

        ui.add_space(8.0);
        egui::Grid::new("gateway-panel-status-grid")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Configured");
                ui.label(if status.configured_enabled {
                    "enabled"
                } else {
                    "disabled"
                });
                ui.end_row();

                ui.label("Runtime");
                ui.label(if status.running { "running" } else { "stopped" });
                ui.end_row();

                ui.label("Transition");
                ui.label(if status.transitioning { "busy" } else { "idle" });
                ui.end_row();

                ui.label("Auth");
                ui.label(if status.auth_configured {
                    "configured"
                } else {
                    "not configured"
                });
                ui.end_row();

                if let Some(info) = &status.info {
                    ui.label("Listen IP");
                    ui.label(&info.listen_ip);
                    ui.end_row();

                    ui.label("Configured Port");
                    ui.label(info.configured_port.to_string());
                    ui.end_row();

                    ui.label("Actual Port");
                    ui.label(info.actual_port.to_string());
                    ui.end_row();

                    ui.label("Address");
                    ui.hyperlink(gateway_base_url(&info.ws_url));
                    ui.end_row();

                    ui.label("Started At");
                    ui.label(format_timestamp_seconds(info.started_at_unix_seconds));
                    ui.end_row();
                }

                if let Some(err) = &status.last_error {
                    ui.label("Last Error");
                    ui.colored_label(ui.visuals().error_fg_color, err);
                    ui.end_row();
                }
            });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);
        ui.heading("Tailscale");
        ui.label(
            "Expose the gateway via Tailscale Serve (tailnet only) or Funnel (public internet).",
        );
        ui.add_space(8.0);

        let current_mode = status.tailscale_mode;
        let mut selected_mode = current_mode;

        ui.horizontal(|ui| {
            ui.label("Mode");
            egui::ComboBox::from_id_salt("tailscale-mode")
                .selected_text(mode_display(current_mode))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut selected_mode, TailscaleMode::Off, "Off");
                    ui.selectable_value(
                        &mut selected_mode,
                        TailscaleMode::Serve,
                        "Serve (tailnet)",
                    );
                    ui.selectable_value(
                        &mut selected_mode,
                        TailscaleMode::Funnel,
                        "Funnel (public)",
                    );
                });
        });

        if selected_mode != current_mode {
            if selected_mode == TailscaleMode::Funnel && !status.auth_configured {
                notifications.error(
                    "Funnel mode requires authentication. Please configure gateway.auth first.",
                );
            } else if ui.button("Apply").clicked() {
                self.set_tailscale_mode(selected_mode, notifications);
            }
        }

        if let Some(info) = &status.info {
            if let Some(ts) = &info.tailscale {
                ui.add_space(8.0);
                egui::Grid::new("gateway-panel-tailscale-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Status");
                        match &ts.status {
                            TailscaleStatus::Connected => {
                                ui.colored_label(egui::Color32::from_rgb(0, 180, 0), "Connected");
                            }
                            TailscaleStatus::Disconnected => {
                                ui.label("Disconnected");
                            }
                            TailscaleStatus::Error(msg) => {
                                ui.colored_label(ui.visuals().error_fg_color, msg);
                            }
                        }
                        ui.end_row();

                        if let Some(url) = &ts.public_url {
                            ui.label("Public URL");
                            ui.hyperlink(url);
                            ui.end_row();
                        }

                        if let Some(msg) = &ts.message {
                            ui.label("Message");
                            ui.label(msg);
                            ui.end_row();
                        }
                    });
            }
        }

        if status.tailscale_mode == TailscaleMode::Funnel && !status.auth_configured {
            ui.add_space(8.0);
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "⚠️ Funnel exposes your gateway publicly. Configure gateway.auth to protect it.",
            );
        }

        if self.config_window_open {
            self.render_config_window(ui.ctx(), notifications);
        }
    }
}

fn mode_display(mode: TailscaleMode) -> &'static str {
    match mode {
        TailscaleMode::Off => "Off",
        TailscaleMode::Serve => "Serve (tailnet)",
        TailscaleMode::Funnel => "Funnel (public)",
    }
}

fn gateway_base_url(ws_url: &str) -> String {
    ws_url
        .strip_suffix("/ws/chat")
        .unwrap_or(ws_url)
        .to_string()
}
