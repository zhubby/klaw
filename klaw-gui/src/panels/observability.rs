use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::Color32;
use egui_extras::{Size, StripBuilder};
use egui_phosphor::regular;
use klaw_config::{ConfigStore, ObservabilityConfig};

#[derive(Default)]
pub struct ObservabilityPanel {
    store: Option<ConfigStore>,
    config: ObservabilityConfig,
    loaded: bool,
    dirty: bool,
    endpoint_buffer: String,
    service_name_buffer: String,
    service_version_buffer: String,
    prometheus_port_buffer: String,
    prometheus_path_buffer: String,
    audit_output_path_buffer: String,
    sample_rate_buffer: String,
    export_interval_buffer: String,
    local_store_retention_days_buffer: String,
    local_store_flush_interval_buffer: String,
}

impl ObservabilityPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.config = snapshot.config.observability.clone();
                self.store = Some(store);
                self.sync_buffers_from_config();
                self.loaded = true;
                notifications.success("Observability config loaded");
            }
            Err(err) => {
                notifications.error(format!("Failed to load config: {err}"));
            }
        }
    }

    fn sync_buffers_from_config(&mut self) {
        self.endpoint_buffer = self.config.otlp.endpoint.clone();
        self.service_name_buffer = self.config.service_name.clone();
        self.service_version_buffer = self.config.service_version.clone();
        self.prometheus_port_buffer = self.config.prometheus.listen_port.to_string();
        self.prometheus_path_buffer = self.config.prometheus.path.clone();
        self.audit_output_path_buffer = self.config.audit.output_path.clone().unwrap_or_default();
        self.sample_rate_buffer = format!("{:.2}", self.config.traces.sample_rate);
        self.export_interval_buffer = self.config.metrics.export_interval_seconds.to_string();
        self.local_store_retention_days_buffer = self.config.local_store.retention_days.to_string();
        self.local_store_flush_interval_buffer =
            self.config.local_store.flush_interval_seconds.to_string();
    }

    fn parse_config_from_buffers(&self) -> Result<ObservabilityConfig, String> {
        let mut next = self.config.clone();
        next.otlp.endpoint = self.endpoint_buffer.trim().to_string();
        next.service_name = self.service_name_buffer.trim().to_string();
        next.service_version = self.service_version_buffer.trim().to_string();
        next.prometheus.path = self.prometheus_path_buffer.trim().to_string();
        next.audit.output_path = if self.audit_output_path_buffer.trim().is_empty() {
            None
        } else {
            Some(self.audit_output_path_buffer.trim().to_string())
        };

        let prometheus_port = self
            .prometheus_port_buffer
            .trim()
            .parse::<u16>()
            .map_err(|_| "Prometheus listen port must be a valid integer".to_string())?;
        if prometheus_port == 0 {
            return Err("Prometheus listen port must be greater than 0".to_string());
        }
        next.prometheus.listen_port = prometheus_port;

        let sample_rate = self
            .sample_rate_buffer
            .trim()
            .parse::<f64>()
            .map_err(|_| "Trace sample rate must be a valid number".to_string())?;
        if !(0.0..=1.0).contains(&sample_rate) {
            return Err("Trace sample rate must be in range [0.0, 1.0]".to_string());
        }
        next.traces.sample_rate = sample_rate;

        let export_interval_seconds = self
            .export_interval_buffer
            .trim()
            .parse::<u64>()
            .map_err(|_| "Metrics export interval must be a valid integer".to_string())?;
        if export_interval_seconds == 0 {
            return Err("Metrics export interval must be greater than 0".to_string());
        }
        next.metrics.export_interval_seconds = export_interval_seconds;

        let retention_days = self
            .local_store_retention_days_buffer
            .trim()
            .parse::<u16>()
            .map_err(|_| "Local store retention days must be a valid integer".to_string())?;
        if retention_days == 0 {
            return Err("Local store retention days must be greater than 0".to_string());
        }
        next.local_store.retention_days = retention_days;

        let flush_interval_seconds = self
            .local_store_flush_interval_buffer
            .trim()
            .parse::<u64>()
            .map_err(|_| "Local store flush interval must be a valid integer".to_string())?;
        if flush_interval_seconds == 0 {
            return Err("Local store flush interval must be greater than 0".to_string());
        }
        next.local_store.flush_interval_seconds = flush_interval_seconds;

        Ok(next)
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn handle_save(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.clone() else {
            notifications.error("Config store is not available");
            return;
        };
        let parsed = match self.parse_config_from_buffers() {
            Ok(parsed) => parsed,
            Err(err) => {
                notifications.error(format!("Save failed: {err}"));
                return;
            }
        };
        match store.save_observability_config(&parsed) {
            Ok(_) => {
                self.config = parsed;
                self.sync_buffers_from_config();
                self.dirty = false;
                notifications.success("Observability config saved");
            }
            Err(err) => {
                notifications.error(format!("Save failed: {err}"));
            }
        }
    }

    fn handle_reload(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.clone() else {
            notifications.error("Config store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.config = snapshot.config.observability.clone();
                self.sync_buffers_from_config();
                self.dirty = false;
                notifications.success("Observability config reloaded");
            }
            Err(err) => {
                notifications.error(format!("Reload failed: {err}"));
            }
        }
    }

    fn status_indicator(&self) -> (bool, &'static str, &'static str) {
        (self.config.enabled, "Enabled", "Disabled")
    }
}

impl PanelRenderer for ObservabilityPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        const MIN_TOTAL_HEIGHT: f32 = 480.0;

        self.ensure_loaded(notifications);

        let (status_enabled, enabled_label, disabled_label) = self.status_indicator();

        let mut render_strip = |ui: &mut egui::Ui, this: &mut ObservabilityPanel| {
            ui.heading(ctx.tab_title);
            ui.horizontal(|ui| {
                ui.label("Status:");
                render_boolean_status(ui, status_enabled, enabled_label, disabled_label);
                if this.is_dirty() {
                    ui.colored_label(Color32::YELLOW, "(unsaved changes)");
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    this.handle_save(notifications);
                }
                if ui.button("Reload").clicked() {
                    this.handle_reload(notifications);
                }
                ui.label("Note: Changes require restart to take effect.");
            });

            ui.separator();

            StripBuilder::new(ui)
                .size(Size::remainder().at_least(200.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("observability-scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.collapsing("General", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Enabled:");
                                        if ui.checkbox(&mut this.config.enabled, "").changed() {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Service Name:");
                                        if ui
                                            .text_edit_singleline(&mut this.service_name_buffer)
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Service Version:");
                                        if ui
                                            .text_edit_singleline(&mut this.service_version_buffer)
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                });

                                ui.separator();

                                ui.collapsing("Metrics", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Enabled:");
                                        if ui
                                            .checkbox(&mut this.config.metrics.enabled, "")
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Export Interval (seconds):");
                                        if ui
                                            .text_edit_singleline(&mut this.export_interval_buffer)
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                });

                                ui.separator();

                                ui.collapsing("Traces", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Enabled:");
                                        if ui
                                            .checkbox(&mut this.config.traces.enabled, "")
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Sample Rate (0.0-1.0):");
                                        if ui
                                            .text_edit_singleline(&mut this.sample_rate_buffer)
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                });

                                ui.separator();

                                ui.collapsing("OTLP Exporter", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Enabled:");
                                        if ui.checkbox(&mut this.config.otlp.enabled, "").changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Endpoint:");
                                        if ui
                                            .text_edit_singleline(&mut this.endpoint_buffer)
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.label("Headers (from config file):");
                                    if this.config.otlp.headers.is_empty() {
                                        ui.label("  (none)");
                                    } else {
                                        for (key, value) in &this.config.otlp.headers {
                                            ui.label(format!("  {key}: {value}"));
                                        }
                                    }
                                });

                                ui.separator();

                                ui.collapsing("Prometheus Exporter", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Enabled:");
                                        if ui
                                            .checkbox(&mut this.config.prometheus.enabled, "")
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Listen Port:");
                                        if ui
                                            .text_edit_singleline(&mut this.prometheus_port_buffer)
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Path:");
                                        if ui
                                            .text_edit_singleline(&mut this.prometheus_path_buffer)
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                });

                                ui.separator();

                                ui.collapsing("Audit", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Enabled:");
                                        if ui.checkbox(&mut this.config.audit.enabled, "").changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Output Path (optional):");
                                        if ui
                                            .text_edit_singleline(
                                                &mut this.audit_output_path_buffer,
                                            )
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                });

                                ui.separator();

                                ui.collapsing("Local Analysis Store", |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Enabled:");
                                        if ui
                                            .checkbox(&mut this.config.local_store.enabled, "")
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Retention Days:");
                                        if ui
                                            .text_edit_singleline(
                                                &mut this.local_store_retention_days_buffer,
                                            )
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Flush Interval (seconds):");
                                        if ui
                                            .text_edit_singleline(
                                                &mut this.local_store_flush_interval_buffer,
                                            )
                                            .changed()
                                        {
                                            this.mark_dirty();
                                        }
                                    });
                                });
                            });
                    });
                });
        };

        let parent_height = ui.available_height();
        if parent_height < MIN_TOTAL_HEIGHT {
            egui::ScrollArea::vertical()
                .id_salt("observability-outer-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_height(MIN_TOTAL_HEIGHT);
                    render_strip(ui, self);
                });
        } else {
            render_strip(ui, self);
        }
    }
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
