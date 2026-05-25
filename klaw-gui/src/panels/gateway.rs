use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::current_ui_language;
use crate::time_format::format_timestamp_seconds;
use crate::{
    GatewayStatusSnapshot, request_gateway_status, request_restart_gateway,
    request_set_tailscale_mode, request_start_gateway, request_tailscale_host_status,
};
use egui::Color32;
use egui_dock::{AllowedSplits, DockArea, DockState, NodeIndex, Style, SurfaceIndex, TabIndex};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, GatewayConfig, TailscaleMode,
};
use klaw_gateway::{TailscaleHostInfo, TailscaleStatus};
use klaw_ui_kit::{LocaleDomain, Translator, label_with_hint, toggle::toggle};
use std::collections::HashMap;
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

enum PendingGatewayResult {
    Snapshot(Box<Result<GatewayStatusSnapshot, String>>),
    TailscaleHost(Result<TailscaleHostInfo, String>),
}

struct PendingGatewayRequest {
    action: PendingGatewayAction,
    receiver: Receiver<PendingGatewayResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayTab {
    Gateway,
    Tailscale,
}

impl GatewayTab {
    const ALL: [Self; 2] = [Self::Gateway, Self::Tailscale];

    fn title_key(self) -> &'static str {
        match self {
            Self::Gateway => "menu-gateway",
            Self::Tailscale => "gw-ts-heading",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Gateway => regular::PLUGS_CONNECTED,
            Self::Tailscale => regular::NETWORK,
        }
    }

    fn tab_id(self) -> &'static str {
        match self {
            Self::Gateway => "gateway",
            Self::Tailscale => "tailscale",
        }
    }
}

pub struct GatewayPanel {
    status: Option<GatewayStatusSnapshot>,
    load_error: Option<String>,
    loaded: bool,
    tailscale_needs_refresh: bool,
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    config: AppConfig,
    config_form: GatewayConfigForm,
    config_window_open: bool,
    auth_token_visible: bool,
    active_tab: GatewayTab,
    tab_dock_state: DockState<GatewayTab>,
    selected_tailscale_mode: TailscaleMode,
    pending_request: Option<PendingGatewayRequest>,
}

impl Default for GatewayPanel {
    fn default() -> Self {
        Self {
            status: None,
            load_error: None,
            loaded: false,
            tailscale_needs_refresh: true,
            store: None,
            config_path: None,
            config: AppConfig::default(),
            config_form: GatewayConfigForm::default(),
            config_window_open: false,
            auth_token_visible: false,
            active_tab: GatewayTab::Gateway,
            tab_dock_state: Self::tab_dock_state(GatewayTab::Gateway),
            selected_tailscale_mode: TailscaleMode::Off,
            pending_request: None,
        }
    }
}

impl GatewayPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn tab_dock_state(active_tab: GatewayTab) -> DockState<GatewayTab> {
        let mut dock_state = DockState::new(GatewayTab::ALL.to_vec());
        let active_index = GatewayTab::ALL
            .iter()
            .position(|tab| *tab == active_tab)
            .unwrap_or_default();
        dock_state.set_active_tab((
            SurfaceIndex::main(),
            NodeIndex::root(),
            TabIndex(active_index),
        ));
        dock_state
    }

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
        let t = Self::translator();
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
            }
            Err(err) => notifications.error(format!("{}: {err}", t.text("gw-notify-load-failed"))),
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

    fn maybe_queue_tailscale_refresh(&mut self) {
        if !self.tailscale_needs_refresh || self.pending_request.is_some() {
            return;
        }
        self.tailscale_needs_refresh = false;
        self.queue_request(
            PendingGatewayAction::Refresh {
                announce: false,
                tailscale_only: true,
            },
            || PendingGatewayResult::TailscaleHost(request_tailscale_host_status()),
        );
    }

    fn queue_request<F>(&mut self, action: PendingGatewayAction, request: F)
    where
        F: FnOnce() -> PendingGatewayResult + Send + 'static,
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
        let t = Self::translator();
        let Some(pending) = self.pending_request.take() else {
            return;
        };

        match pending.receiver.try_recv() {
            Ok(result) => match result {
                PendingGatewayResult::Snapshot(result) => match *result {
                    Ok(status) => {
                        self.apply_status(status);
                        match pending.action {
                            PendingGatewayAction::Refresh {
                                announce,
                                tailscale_only,
                            } => {
                                if announce {
                                    notifications.success(if tailscale_only {
                                        t.text("gw-tailscale-status-refreshed")
                                    } else {
                                        t.text("gw-status-refreshed")
                                    });
                                }
                            }
                            PendingGatewayAction::Start => {
                                self.tailscale_needs_refresh = true;
                                let message = self
                                    .status
                                    .as_ref()
                                    .and_then(|snapshot| snapshot.info.as_ref())
                                    .map(|info| {
                                        t.text_args(
                                            "gw-notify-started-at",
                                            HashMap::from([("url", info.ws_url.clone())]),
                                        )
                                    })
                                    .unwrap_or_else(|| t.text("gw-notify-started"));
                                notifications.success(message);
                            }
                            PendingGatewayAction::Restart => {
                                self.tailscale_needs_refresh = true;
                                let message = self
                                    .status
                                    .as_ref()
                                    .and_then(|snapshot| snapshot.info.as_ref())
                                    .map(|info| {
                                        t.text_args(
                                            "gw-notify-restarted-at",
                                            HashMap::from([("url", info.ws_url.clone())]),
                                        )
                                    })
                                    .unwrap_or_else(|| t.text("gw-notify-restarted"));
                                notifications.success(message);
                            }
                            PendingGatewayAction::SetTailscaleMode(mode) => {
                                self.tailscale_needs_refresh = true;
                                let mode_str = match mode {
                                    TailscaleMode::Off => t.text("gw-ts-mode-apply-disabled"),
                                    TailscaleMode::Serve => t.text("gw-ts-mode-apply-serve"),
                                    TailscaleMode::Funnel => t.text("gw-ts-mode-apply-funnel"),
                                };
                                notifications.success(t.text_args(
                                    "gw-notify-tailscale-mode-set",
                                    HashMap::from([("mode", mode_str)]),
                                ));
                            }
                        }
                        self.maybe_queue_tailscale_refresh();
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
                                    t.text_args(
                                        "gw-notify-tailscale-refresh-failed",
                                        HashMap::from([("error", err.clone())]),
                                    )
                                } else {
                                    t.text_args(
                                        "gw-notify-load-failed",
                                        HashMap::from([("error", err.clone())]),
                                    )
                                }
                            }
                            PendingGatewayAction::Start => t.text_args(
                                "gw-notify-start-failed",
                                HashMap::from([("error", err.clone())]),
                            ),
                            PendingGatewayAction::Restart => t.text_args(
                                "gw-notify-restart-failed",
                                HashMap::from([("error", err.clone())]),
                            ),
                            PendingGatewayAction::SetTailscaleMode(_) => t.text_args(
                                "gw-notify-tailscale-mode-failed",
                                HashMap::from([("error", err.clone())]),
                            ),
                        });
                        self.load_error = Some(err);
                        self.queue_request(
                            PendingGatewayAction::Refresh {
                                announce: false,
                                tailscale_only: false,
                            },
                            || PendingGatewayResult::Snapshot(Box::new(request_gateway_status())),
                        );
                    }
                },
                PendingGatewayResult::TailscaleHost(result) => match result {
                    Ok(tailscale_host) => {
                        if let Some(status) = self.status.as_mut() {
                            status.tailscale_host = tailscale_host;
                        }
                        if let PendingGatewayAction::Refresh { announce, .. } = pending.action
                            && announce
                        {
                            notifications.success(t.text("gw-tailscale-status-refreshed"));
                        }
                    }
                    Err(err) => {
                        self.tailscale_needs_refresh = true;
                        notifications.error(t.text_args(
                            "gw-notify-tailscale-refresh-failed",
                            HashMap::from([("error", err.clone())]),
                        ));
                        self.load_error = Some(err);
                    }
                },
            },
            Err(TryRecvError::Empty) => {
                self.pending_request = Some(pending);
            }
            Err(TryRecvError::Disconnected) => {
                notifications.error(t.text("gw-notify-worker-closed"));
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
            move || {
                if tailscale_only {
                    PendingGatewayResult::TailscaleHost(request_tailscale_host_status())
                } else {
                    PendingGatewayResult::Snapshot(Box::new(request_gateway_status()))
                }
            },
        );
    }

    fn open_config_window(&mut self) {
        self.config_form = GatewayConfigForm::from_config(&self.config.gateway);
        self.auth_token_visible = false;
        self.config_window_open = true;
    }

    fn save_config(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(store) = self.store.as_ref() else {
            notifications.error(t.text("gw-notify-config-store-unavailable"));
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
                    notifications.success(t.text("gw-notify-config-saved-restart"));
                } else {
                    notifications.success(t.text("gw-notify-config-saved"));
                }
            }
            Err(err) => notifications.error(t.text_args(
                "gw-notify-save-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn reload_config(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(store) = self.store.as_ref() else {
            notifications.error(t.text("gw-notify-config-store-unavailable"));
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                self.refresh(notifications, false, false);
                notifications.success(t.text("gw-notify-config-reloaded"));
            }
            Err(err) => notifications.error(t.text_args(
                "gw-notify-reload-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn start(&mut self, _notifications: &mut NotificationCenter) {
        self.queue_request(PendingGatewayAction::Start, || {
            PendingGatewayResult::Snapshot(Box::new(request_start_gateway()))
        });
    }

    fn restart(&mut self, _notifications: &mut NotificationCenter) {
        self.queue_request(PendingGatewayAction::Restart, || {
            PendingGatewayResult::Snapshot(Box::new(request_restart_gateway()))
        });
    }

    fn set_tailscale_mode(&mut self, mode: TailscaleMode, _notifications: &mut NotificationCenter) {
        self.queue_request(PendingGatewayAction::SetTailscaleMode(mode), move || {
            PendingGatewayResult::Snapshot(Box::new(request_set_tailscale_mode(mode)))
        });
    }

    fn render_config_window(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        let mut open = self.config_window_open;
        egui::Window::new(t.text("gw-cfg-title"))
            .id(egui::Id::new("gateway-config-window"))
            .open(&mut open)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.heading(t.text("gw-cfg-basic"));
                egui::Grid::new("gateway-config-basic-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        label_with_hint(
                            ui,
                            &t.text("gw-cfg-enabled"),
                            &t.text("gw-cfg-enabled-hint"),
                        );
                        ui.add(toggle(&mut self.config_form.enabled));
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("gw-cfg-listen-ip"),
                            &t.text("gw-cfg-listen-ip-hint"),
                        );
                        ui.add_sized(
                            [240.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.config_form.listen_ip),
                        );
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("gw-cfg-listen-port"),
                            &t.text("gw-cfg-listen-port-hint"),
                        );
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [100.0, ui.spacing().interact_size.y],
                                egui::TextEdit::singleline(&mut self.config_form.listen_port),
                            );
                            ui.label(t.text("gw-cfg-port-auto"));
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.heading(t.text("gw-cfg-auth"));
                egui::Grid::new("gateway-config-auth-grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        label_with_hint(
                            ui,
                            &t.text("gw-cfg-auth-enabled"),
                            &t.text("gw-cfg-auth-enabled-hint"),
                        );
                        ui.add(toggle(&mut self.config_form.auth_enabled));
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("gw-cfg-auth-token"),
                            &t.text("gw-cfg-auth-token-hint"),
                        );
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
                                    notifications.error(t.text("gw-notify-auth-token-empty"));
                                } else {
                                    let auth_token = self.config_form.auth_token.clone();
                                    ui.ctx().output_mut(|output| {
                                        output.commands.push(
                                            egui::output::OutputCommand::CopyText(auth_token),
                                        );
                                    });
                                    notifications.success(t.text("gw-notify-auth-token-copied"));
                                }
                            }
                            if ui.button(t.text("gw-btn-generate")).clicked() {
                                self.config_form.auth_token = generate_gateway_auth_token();
                            }
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t.text("gw-btn-reload")).clicked() {
                        self.reload_config(notifications);
                    }
                    if ui.button(t.text("gw-btn-save")).clicked() {
                        self.save_config(notifications);
                    }
                });
            });
        self.config_window_open = open;
    }

    fn render_gateway_tab(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        status: &GatewayStatusSnapshot,
        t: &Translator,
    ) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.pending_request.is_none(),
                    egui::Button::new(t.text_args(
                        "gw-btn-refresh",
                        HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                    )),
                )
                .clicked()
            {
                self.refresh(notifications, true, false);
            }

            if ui
                .button(t.text_args(
                    "gw-btn-config",
                    HashMap::from([("icon", regular::SLIDERS.to_string())]),
                ))
                .clicked()
            {
                self.open_config_window();
            }

            if ui
                .add_enabled(
                    !status.transitioning && !status.running && self.pending_request.is_none(),
                    egui::Button::new(t.text_args(
                        "gw-btn-start",
                        HashMap::from([("icon", regular::PLAY.to_string())]),
                    )),
                )
                .clicked()
            {
                self.start(notifications);
            }

            if ui
                .add_enabled(
                    !status.transitioning && status.running && self.pending_request.is_none(),
                    egui::Button::new(t.text_args(
                        "gw-btn-restart",
                        HashMap::from([("icon", regular::ARROW_COUNTER_CLOCKWISE.to_string())]),
                    )),
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
                ui.label(t.text("gw-status-configured"));
                render_boolean_status(
                    ui,
                    status.configured_enabled,
                    &t.text("gw-status-enabled"),
                    &t.text("gw-status-disabled"),
                );
                ui.end_row();

                ui.label(t.text("gw-status-runtime"));
                let running_label = if status.running {
                    t.text("gw-status-running")
                } else {
                    t.text("gw-status-stopped")
                };
                ui.label(running_label);
                ui.end_row();

                ui.label(t.text("gw-status-transition"));
                let transition_label = if status.transitioning {
                    t.text("gw-status-busy")
                } else {
                    t.text("gw-status-idle")
                };
                ui.label(transition_label);
                ui.end_row();

                ui.label(t.text("gw-status-auth"));
                render_boolean_status(
                    ui,
                    status.auth_configured,
                    &t.text("gw-status-auth-configured"),
                    &t.text("gw-status-auth-not-configured"),
                );
                ui.end_row();

                if let Some(info) = &status.info {
                    ui.label(t.text("gw-status-listen-ip"));
                    ui.label(&info.listen_ip);
                    ui.end_row();

                    ui.label(t.text("gw-status-configured-port"));
                    ui.label(info.configured_port.to_string());
                    ui.end_row();

                    ui.label(t.text("gw-status-actual-port"));
                    ui.label(info.actual_port.to_string());
                    ui.end_row();

                    ui.label(t.text("gw-status-address"));
                    ui.hyperlink(gateway_base_url(&info.ws_url));
                    ui.end_row();

                    ui.label(t.text("gw-status-started-at"));
                    ui.label(format_timestamp_seconds(info.started_at_unix_seconds));
                    ui.end_row();
                }
            });
    }

    fn render_tailscale_tab(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        status: &GatewayStatusSnapshot,
        t: &Translator,
    ) {
        ui.label(t.text("gw-ts-subtitle"));
        ui.add_space(8.0);

        let current_mode = status.tailscale_mode;
        let tailscale_available = tailscale_service_available(status);

        ui.horizontal(|ui| {
            ui.label(t.text("gw-ts-mode"));
            egui::ComboBox::from_id_salt("tailscale-mode")
                .selected_text(mode_display(self.selected_tailscale_mode, t))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.selected_tailscale_mode,
                        TailscaleMode::Off,
                        t.text("gw-ts-mode-off"),
                    );
                    ui.selectable_value(
                        &mut self.selected_tailscale_mode,
                        TailscaleMode::Serve,
                        t.text("gw-ts-mode-serve"),
                    );
                    ui.selectable_value(
                        &mut self.selected_tailscale_mode,
                        TailscaleMode::Funnel,
                        t.text("gw-ts-mode-funnel"),
                    );
                });
            if ui
                .add_enabled(
                    self.pending_request.is_none(),
                    egui::Button::new(t.text_args(
                        "gw-btn-refresh-ts",
                        HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                    )),
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
                .add_enabled(apply_enabled, egui::Button::new(t.text("gw-btn-apply")))
                .clicked()
            {
                self.set_tailscale_mode(self.selected_tailscale_mode, notifications);
            }
        });

        ui.add_space(8.0);
        ui.label(t.text("gw-ts-host-status"));
        egui::Grid::new("gateway-panel-tailscale-host-grid")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label(t.text("gw-ts-host-status-label"));
                render_tailscale_status(ui, &status.tailscale_host.status, t);
                ui.end_row();

                if let Some(version) = &status.tailscale_host.version {
                    ui.label(t.text("gw-ts-host-version"));
                    ui.label(version);
                    ui.end_row();
                }

                if let Some(backend_state) = &status.tailscale_host.backend_state {
                    ui.label(t.text("gw-ts-host-backend-state"));
                    ui.label(backend_state);
                    ui.end_row();
                }

                if let Some(dns_name) = &status.tailscale_host.dns_name {
                    ui.label(t.text("gw-ts-host-dns-name"));
                    ui.label(dns_name);
                    ui.end_row();
                }

                if let Some(url) = &status.tailscale_host.public_url {
                    ui.label(t.text("gw-ts-host-tailnet-url"));
                    ui.hyperlink(url);
                    ui.end_row();
                }

                if let Some(message) = &status.tailscale_host.message {
                    ui.label(t.text("gw-ts-host-message"));
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
                    ui.label(t.text("gw-ts-gateway-exposure"));
                    render_tailscale_status(ui, &ts.status, t);
                    ui.end_row();

                    if let Some(url) = &ts.public_url {
                        ui.label(t.text("gw-ts-gateway-url"));
                        ui.hyperlink(url);
                        ui.end_row();
                    }

                    if let Some(msg) = &ts.message {
                        ui.label(t.text("gw-ts-message"));
                        ui.label(msg);
                        ui.end_row();
                    }
                });
        }

        if status.tailscale_mode == TailscaleMode::Funnel && !status.auth_configured {
            ui.add_space(8.0);
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t.text("gw-ts-funnel-no-auth-warning"),
            );
        }
    }

    fn render_tab_dock(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        status: GatewayStatusSnapshot,
        t: &Translator,
    ) {
        let mut dock_state = std::mem::replace(
            &mut self.tab_dock_state,
            Self::tab_dock_state(self.active_tab),
        );
        let mut style = Style::from_egui(ui.style().as_ref());
        style.tab_bar.show_scroll_bar_on_overflow = false;

        DockArea::new(&mut dock_state)
            .id(egui::Id::new("gateway-panel-dock"))
            .style(style)
            .show_add_buttons(false)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .tab_context_menus(false)
            .draggable_tabs(false)
            .allowed_splits(AllowedSplits::None)
            .show_inside(
                ui,
                &mut GatewayTabViewer {
                    panel: self,
                    notifications,
                    status,
                    translator: t,
                },
            );

        self.tab_dock_state = dock_state;
    }
}

struct GatewayTabViewer<'a> {
    panel: &'a mut GatewayPanel,
    notifications: &'a mut NotificationCenter,
    status: GatewayStatusSnapshot,
    translator: &'a Translator,
}

impl egui_dock::TabViewer for GatewayTabViewer<'_> {
    type Tab = GatewayTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{} {}", tab.icon(), self.translator.text(tab.title_key())).into()
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("gateway-panel-tab", tab.tab_id()))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        self.panel.active_tab = *tab;
        egui::ScrollArea::vertical()
            .id_salt(("gateway-panel-tab-scroll", tab.tab_id()))
            .auto_shrink([false, false])
            .show(ui, |ui| match *tab {
                GatewayTab::Gateway => {
                    self.panel.render_gateway_tab(
                        ui,
                        self.notifications,
                        &self.status,
                        self.translator,
                    );
                }
                GatewayTab::Tailscale => {
                    self.panel.render_tailscale_tab(
                        ui,
                        self.notifications,
                        &self.status,
                        self.translator,
                    );
                }
            });
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        if response.clicked() {
            self.panel.active_tab = *tab;
        }
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }
}

impl PanelRenderer for GatewayPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        self.ensure_loaded(notifications);
        self.poll_pending_request(notifications);

        ui.heading(ctx.tab_title);
        ui.label(t.text("gw-subtitle"));
        ui.separator();

        let Some(status) = self.status.clone() else {
            if let Some(err) = &self.load_error {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    t.text_args(
                        "gw-status-unavailable",
                        HashMap::from([("error", err.clone())]),
                    ),
                );
                ui.add_space(8.0);
                if ui
                    .button(t.text_args(
                        "gw-btn-retry",
                        HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                    ))
                    .clicked()
                {
                    self.refresh(notifications, true, false);
                }
            } else {
                ui.label(t.text("gw-loading"));
            }
            if self.config_window_open {
                self.render_config_window(ui.ctx(), notifications);
            }
            return;
        };

        if status.transitioning || self.pending_request.is_some() {
            ui.ctx().request_repaint_after(GATEWAY_POLL_INTERVAL);
        }

        self.render_tab_dock(ui, notifications, status, &t);

        if self.config_window_open {
            self.render_config_window(ui.ctx(), notifications);
        }
    }
}

fn mode_display(mode: TailscaleMode, t: &Translator) -> String {
    match mode {
        TailscaleMode::Off => t.text("gw-ts-mode-off"),
        TailscaleMode::Serve => t.text("gw-ts-mode-serve"),
        TailscaleMode::Funnel => t.text("gw-ts-mode-funnel"),
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

fn render_tailscale_status(ui: &mut egui::Ui, status: &TailscaleStatus, t: &Translator) {
    match status {
        TailscaleStatus::Connected => {
            ui.colored_label(
                egui::Color32::from_rgb(0, 180, 0),
                t.text("gw-ts-host-connected"),
            );
        }
        TailscaleStatus::Disconnected => {
            ui.label(t.text("gw-ts-host-disconnected"));
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
        let mut panel = GatewayPanel {
            selected_tailscale_mode: TailscaleMode::Serve,
            ..Default::default()
        };

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
