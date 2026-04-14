use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_seconds;
use crate::{
    GatewayStatusSnapshot, request_gateway_status, request_restart_gateway,
    request_set_tailscale_mode, request_start_gateway,
};
use egui::Color32;
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, GatewayConfig, TailscaleMode,
};
use klaw_gateway::TailscaleStatus;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;
use uuid::Uuid;

const GATEWAY_POLL_INTERVAL: Duration = Duration::from_millis(250);

fn generate_gateway_auth_token() -> String {
    format!("sk_{}", Uuid::new_v4().simple())
}

#[derive(Debug, Clone)]
struct GatewayConfigForm {
    enabled: bool,
    listen_ip: String,
    listen_port: String,
    auth_enabled: bool,
    auth_token: String,
    auth_env_key: String,
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

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum PendingGatewayAction {
    Refresh {
        announce: bool,
        tailscale_only: bool,
    },
    Start,
    Restart,
    SetTailscaleMode(TailscaleMode),
}

struct PendingGatewayRequest {
    action: PendingGatewayAction,
    receiver: Receiver<Result<GatewayStatusSnapshot, String>>,
}

pub struct GatewayPanel {
    status: Option<GatewayStatusSnapshot>,
    load_error: Option<String>,
    loaded: bool,
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    config: AppConfig,
    config_form: GatewayConfigForm,
    config_window_open: bool,
    auth_token_visible: bool,
    selected_tailscale_mode: TailscaleMode,
    pending_request: Option<PendingGatewayRequest>,
}

impl Default for GatewayPanel {
    fn default() -> Self {
        Self {
            status: None,
            load_error: None,
            loaded: false,
            store: None,
            config_path: None,
            config: AppConfig::default(),
            config_form: GatewayConfigForm::default(),
            config_window_open: false,
            auth_token_visible: false,
            selected_tailscale_mode: TailscaleMode::Off,
            pending_request: None,
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
        self.refresh(notifications, false, false);
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
        self.selected_tailscale_mode = self.config.gateway.tailscale.mode;
    }

    fn apply_status(&mut self, status: GatewayStatusSnapshot) {
        self.selected_tailscale_mode = status.tailscale_mode;
        self.load_error = None;
        self.status = Some(status);
    }

    fn queue_request<F>(&mut self, action: PendingGatewayAction, request: F)
    where
        F: FnOnce() -> Result<GatewayStatusSnapshot, String> + Send + 'static,
    {
        if self.pending_request.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(request());
        });
        self.pending_request = Some(PendingGatewayRequest {
            action,
            receiver: rx,
        });
    }

    fn poll_pending_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(pending) = self.pending_request.take() else {
            return;
        };

        match pending.receiver.try_recv() {
            Ok(result) => match result {
                Ok(status) => {
                    self.apply_status(status);
                    match pending.action {
                        PendingGatewayAction::Refresh {
                            announce,
                            tailscale_only,
                        } => {
                            if announce {
                                notifications.success(if tailscale_only {
                                    "Tailscale status refreshed"
                                } else {
                                    "Gateway status refreshed"
                                });
                            }
                        }
                        PendingGatewayAction::Start => {
                            let message = self
                                .status
                                .as_ref()
                                .and_then(|snapshot| snapshot.info.as_ref())
                                .map(|info| format!("Gateway started at {}", info.ws_url))
                                .unwrap_or_else(|| "Gateway started".to_string());
                            notifications.success(message);
                        }
                        PendingGatewayAction::Restart => {
                            let message = self
                                .status
                                .as_ref()
                                .and_then(|snapshot| snapshot.info.as_ref())
                                .map(|info| format!("Gateway restarted at {}", info.ws_url))
                                .unwrap_or_else(|| "Gateway restarted".to_string());
                            notifications.success(message);
                        }
                        PendingGatewayAction::SetTailscaleMode(mode) => {
                            let mode_str = match mode {
                                TailscaleMode::Off => "disabled",
                                TailscaleMode::Serve => "serve (tailnet only)",
                                TailscaleMode::Funnel => "funnel (public)",
                            };
                            notifications.success(format!("Tailscale mode set to {}", mode_str));
                        }
                    }
                }
                Err(err) => {
                    if matches!(pending.action, PendingGatewayAction::SetTailscaleMode(_)) {
                        self.selected_tailscale_mode = self
                            .status
                            .as_ref()
                            .map(|status| status.tailscale_mode)
                            .unwrap_or(self.config.gateway.tailscale.mode);
                    }
                    notifications.error(match pending.action {
                        PendingGatewayAction::Refresh { tailscale_only, .. } => {
                            if tailscale_only {
                                format!("Failed to refresh tailscale status: {err}")
                            } else {
                                format!("Failed to load gateway status: {err}")
                            }
                        }
                        PendingGatewayAction::Start => {
                            format!("Failed to start gateway: {err}")
                        }
                        PendingGatewayAction::Restart => {
                            format!("Failed to restart gateway: {err}")
                        }
                        PendingGatewayAction::SetTailscaleMode(_) => {
                            format!("Failed to set tailscale mode: {err}")
                        }
                    });
                    self.load_error = Some(err);
                    self.queue_request(
                        PendingGatewayAction::Refresh {
                            announce: false,
                            tailscale_only: false,
                        },
                        request_gateway_status,
                    );
                }
            },
            Err(TryRecvError::Empty) => {
                self.pending_request = Some(pending);
            }
            Err(TryRecvError::Disconnected) => {
                notifications.error("Gateway request worker closed unexpectedly");
            }
        }
    }

    fn refresh(
        &mut self,
        _notifications: &mut NotificationCenter,
        announce: bool,
        tailscale_only: bool,
    ) {
        self.queue_request(
            PendingGatewayAction::Refresh {
                announce,
                tailscale_only,
            },
            request_gateway_status,
        );
    }

    fn open_config_window(&mut self) {
        self.config_form = GatewayConfigForm::from_config(&self.config.gateway);
        self.auth_token_visible = false;
        self.config_window_open = true;
    }

    fn save_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };

        let config_form = self.config_form.clone();
        match store.update_config(|config| {
            config_form
                .apply_to_config(config)
                .map_err(ConfigError::InvalidConfig)?;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.refresh(notifications, false, false);
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
                self.refresh(notifications, false, false);
                notifications.success("Config reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn start(&mut self, _notifications: &mut NotificationCenter) {
        self.queue_request(PendingGatewayAction::Start, request_start_gateway);
    }

    fn restart(&mut self, _notifications: &mut NotificationCenter) {
        self.queue_request(PendingGatewayAction::Restart, request_restart_gateway);
    }

    fn set_tailscale_mode(&mut self, mode: TailscaleMode, _notifications: &mut NotificationCenter) {
        self.queue_request(PendingGatewayAction::SetTailscaleMode(mode), move || {
            request_set_tailscale_mode(mode)
        });
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
                egui::Grid::new("gateway-config-basic-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Enabled");
                        ui.checkbox(&mut self.config_form.enabled, "");
                        ui.end_row();

                        ui.label("Listen IP");
                        ui.add_sized(
                            [240.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.config_form.listen_ip),
                        );
                        ui.end_row();

                        ui.label("Listen Port");
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [100.0, ui.spacing().interact_size.y],
                                egui::TextEdit::singleline(&mut self.config_form.listen_port),
                            );
                            ui.label("(0 = auto)");
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.heading("Auth");
                egui::Grid::new("gateway-config-auth-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Enabled");
                        ui.checkbox(&mut self.config_form.auth_enabled, "");
                        ui.end_row();

                        ui.label("Token");
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [280.0, ui.spacing().interact_size.y],
                                egui::TextEdit::singleline(&mut self.config_form.auth_token)
                                    .password(!self.auth_token_visible),
                            );
                            let toggle_icon = if self.auth_token_visible {
                                regular::EYE_SLASH
                            } else {
                                regular::EYE
                            };
                            if ui.button(toggle_icon).clicked() {
                                self.auth_token_visible = !self.auth_token_visible;
                            }
                            if ui.button(regular::COPY).clicked() {
                                if self.config_form.auth_token.is_empty() {
                                    notifications.error("Gateway auth token is empty");
                                } else {
                                    let auth_token = self.config_form.auth_token.clone();
                                    ui.ctx().output_mut(|output| {
                                        output.commands.push(
                                            egui::output::OutputCommand::CopyText(auth_token),
                                        );
                                    });
                                    notifications.success("Gateway auth token copied");
                                }
                            }
                            if ui.button("Generate").clicked() {
                                self.config_form.auth_token = generate_gateway_auth_token();
                            }
                        });
                        ui.end_row();

                        ui.label("Env Key");
                        ui.add_sized(
                            [240.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.config_form.auth_env_key),
                        );
                        ui.end_row();
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
        self.poll_pending_request(notifications);
        egui::ScrollArea::vertical()
            .id_salt("gateway-panel-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading(ctx.tab_title);
                ui.label("Manage the embedded gateway service used by the GUI runtime.");
                ui.separator();

                let Some(status) = self.status.clone() else {
                    if let Some(err) = &self.load_error {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("Gateway status unavailable: {err}"),
                        );
                        ui.add_space(8.0);
                        if ui.button("Retry").clicked() {
                            self.refresh(notifications, true, false);
                        }
                    } else {
                        ui.label("Loading...");
                    }
                    return;
                };

                if status.transitioning {
                    ui.ctx().request_repaint_after(GATEWAY_POLL_INTERVAL);
                }
                if self.pending_request.is_some() {
                    ui.ctx().request_repaint_after(GATEWAY_POLL_INTERVAL);
                }

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.pending_request.is_none(), egui::Button::new("Refresh"))
                        .clicked()
                    {
                        self.refresh(notifications, true, false);
                    }

                    if ui.button("Config").clicked() {
                        self.open_config_window();
                    }

                    if ui
                        .add_enabled(
                            !status.transitioning
                                && !status.running
                                && self.pending_request.is_none(),
                            egui::Button::new("Start"),
                        )
                        .clicked()
                    {
                        self.start(notifications);
                    }

                    if ui
                        .add_enabled(
                            !status.transitioning
                                && status.running
                                && self.pending_request.is_none(),
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
                        render_boolean_status(ui, status.configured_enabled, "Enabled", "Disabled");
                        ui.end_row();

                        ui.label("Runtime");
                        ui.label(if status.running { "running" } else { "stopped" });
                        ui.end_row();

                        ui.label("Transition");
                        ui.label(if status.transitioning { "busy" } else { "idle" });
                        ui.end_row();

                        ui.label("Auth");
                        render_boolean_status(
                            ui,
                            status.auth_configured,
                            "Configured",
                            "Not Configured",
                        );
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
                let tailscale_available = tailscale_service_available(&status);

                ui.horizontal(|ui| {
                    ui.label("Mode");
                    egui::ComboBox::from_id_salt("tailscale-mode")
                        .selected_text(mode_display(self.selected_tailscale_mode))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.selected_tailscale_mode,
                                TailscaleMode::Off,
                                "Off",
                            );
                            ui.selectable_value(
                                &mut self.selected_tailscale_mode,
                                TailscaleMode::Serve,
                                "Serve (tailnet)",
                            );
                            ui.selectable_value(
                                &mut self.selected_tailscale_mode,
                                TailscaleMode::Funnel,
                                "Funnel (public)",
                            );
                        });
                    if ui
                        .add_enabled(
                            self.pending_request.is_none(),
                            egui::Button::new("Refresh Tailscale"),
                        )
                        .clicked()
                    {
                        self.refresh(notifications, true, true);
                    }
                    let apply_enabled = self.selected_tailscale_mode != current_mode
                        && tailscale_available
                        && !status.transitioning
                        && self.pending_request.is_none();
                    if ui
                        .add_enabled(apply_enabled, egui::Button::new("Apply"))
                        .clicked()
                    {
                        self.set_tailscale_mode(self.selected_tailscale_mode, notifications);
                    }
                });

                ui.add_space(8.0);
                ui.label("Host Status");
                egui::Grid::new("gateway-panel-tailscale-host-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Status");
                        render_tailscale_status(ui, &status.tailscale_host.status);
                        ui.end_row();

                        if let Some(version) = &status.tailscale_host.version {
                            ui.label("Version");
                            ui.label(version);
                            ui.end_row();
                        }

                        if let Some(backend_state) = &status.tailscale_host.backend_state {
                            ui.label("Backend State");
                            ui.label(backend_state);
                            ui.end_row();
                        }

                        if let Some(dns_name) = &status.tailscale_host.dns_name {
                            ui.label("DNS Name");
                            ui.label(dns_name);
                            ui.end_row();
                        }

                        if let Some(url) = &status.tailscale_host.public_url {
                            ui.label("Tailnet URL");
                            ui.hyperlink(url);
                            ui.end_row();
                        }

                        if let Some(message) = &status.tailscale_host.message {
                            ui.label("Host Message");
                            ui.label(message);
                            ui.end_row();
                        }
                    });

                if let Some(info) = &status.info
                    && let Some(ts) = &info.tailscale
                {
                    ui.add_space(8.0);
                    egui::Grid::new("gateway-panel-tailscale-grid")
                        .num_columns(2)
                        .spacing([16.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Gateway Exposure");
                            render_tailscale_status(ui, &ts.status);
                            ui.end_row();

                            if let Some(url) = &ts.public_url {
                                ui.label("Gateway URL");
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

                if status.tailscale_mode == TailscaleMode::Funnel && !status.auth_configured {
                    ui.add_space(8.0);
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "⚠️ Funnel exposes your gateway publicly. Configure gateway.auth to protect it.",
                    );
                }
            });

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

fn render_boolean_status(
    ui: &mut egui::Ui,
    enabled: bool,
    enabled_label: &str,
    disabled_label: &str,
) {
    let (icon, color, label) = if enabled {
        (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            enabled_label,
        )
    } else {
        (
            regular::X_CIRCLE,
            ui.visuals().error_fg_color,
            disabled_label,
        )
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, icon);
        ui.colored_label(color, label);
    });
}

fn render_tailscale_status(ui: &mut egui::Ui, status: &TailscaleStatus) {
    match status {
        TailscaleStatus::Connected => {
            ui.colored_label(egui::Color32::from_rgb(0, 180, 0), "Connected");
        }
        TailscaleStatus::Disconnected => {
            ui.label("Disconnected");
        }
        TailscaleStatus::Error(message) => {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
    }
}

fn tailscale_service_available(status: &GatewayStatusSnapshot) -> bool {
    matches!(status.tailscale_host.status, TailscaleStatus::Connected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_snapshot_syncs_selected_tailscale_mode() {
        let mut panel = GatewayPanel::default();
        let mut config = AppConfig::default();
        config.gateway.tailscale.mode = TailscaleMode::Serve;

        panel.apply_snapshot(ConfigSnapshot {
            path: PathBuf::from("/tmp/klaw-config.toml"),
            config,
            raw_toml: String::new(),
            revision: 1,
        });

        assert_eq!(panel.selected_tailscale_mode, TailscaleMode::Serve);
    }

    #[test]
    fn apply_status_syncs_selected_tailscale_mode() {
        let mut panel = GatewayPanel::default();
        panel.selected_tailscale_mode = TailscaleMode::Serve;

        panel.apply_status(GatewayStatusSnapshot {
            tailscale_mode: TailscaleMode::Funnel,
            ..GatewayStatusSnapshot::default()
        });

        assert_eq!(panel.selected_tailscale_mode, TailscaleMode::Funnel);
    }

    #[test]
    fn tailscale_apply_requires_connected_host() {
        let mut status = GatewayStatusSnapshot::default();
        status.tailscale_host.status = TailscaleStatus::Disconnected;
        assert!(!tailscale_service_available(&status));

        status.tailscale_host.status = TailscaleStatus::Connected;
        assert!(tailscale_service_available(&status));
    }

    #[test]
    fn tailscale_error_does_not_count_as_available() {
        let mut status = GatewayStatusSnapshot::default();
        status.tailscale_host.status = TailscaleStatus::Error("unavailable".to_string());

        assert!(!tailscale_service_available(&status));
    }

    #[test]
    fn generated_gateway_auth_token_uses_expected_prefix() {
        let token = generate_gateway_auth_token();
        assert!(token.starts_with("sk_"));
        assert_eq!(token.len(), 35);
    }

    #[test]
    fn config_form_persists_generated_auth_token() {
        let token = generate_gateway_auth_token();
        let form = GatewayConfigForm {
            auth_token: token.clone(),
            ..GatewayConfigForm::default()
        };
        let mut config = AppConfig::default();

        form.apply_to_config(&mut config)
            .expect("config apply should succeed");

        assert_eq!(config.gateway.auth.token.as_deref(), Some(token.as_str()));
    }
}
