use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_seconds;
use crate::{
    request_gateway_status, request_restart_gateway, request_set_gateway_enabled,
    request_set_tailscale_mode, GatewayStatusSnapshot,
};
use klaw_config::TailscaleMode;
use klaw_gateway::TailscaleStatus;
use std::time::Duration;

const GATEWAY_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct GatewayPanel {
    status: Option<GatewayStatusSnapshot>,
    loaded: bool,
}

impl GatewayPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.refresh(notifications, false);
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

    fn set_enabled(&mut self, enabled: bool, notifications: &mut NotificationCenter) {
        match request_set_gateway_enabled(enabled) {
            Ok(status) => {
                let message = if enabled {
                    status
                        .info
                        .as_ref()
                        .map(|info| format!("Gateway started at {}", info.ws_url))
                        .unwrap_or_else(|| "Gateway started".to_string())
                } else {
                    "Gateway stopped".to_string()
                };
                self.status = Some(status);
                notifications.success(message);
            }
            Err(err) => {
                notifications.error(format!("Failed to update gateway: {err}"));
                self.refresh(notifications, false);
            }
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

        let mut enabled = status.configured_enabled;
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications, true);
            }

            if ui
                .add_enabled(
                    !status.transitioning,
                    egui::Checkbox::new(&mut enabled, "Enabled"),
                )
                .changed()
            {
                self.set_enabled(enabled, notifications);
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
