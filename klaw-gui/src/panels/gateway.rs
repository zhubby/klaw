use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_seconds;
use crate::{
    request_gateway_status, request_restart_gateway, request_set_gateway_enabled,
    GatewayStatusSnapshot,
};
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

                    ui.label("WebSocket");
                    ui.hyperlink(&info.ws_url);
                    ui.end_row();

                    ui.label("Health");
                    ui.hyperlink(&info.health_url);
                    ui.end_row();

                    ui.label("Metrics");
                    ui.hyperlink(&info.metrics_url);
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
    }
}
