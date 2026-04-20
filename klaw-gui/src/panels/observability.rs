use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::{Color32, RichText};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{ConfigStore, ObservabilityConfig, PriceEntry, PriceTable};

const PRICE_TABLE_HEIGHT: f32 = 280.0;
const PRICE_ROW_HEIGHT: f32 = 24.0;

#[derive(Debug, Clone)]
struct PriceForm {
    original_provider: Option<String>,
    original_model: Option<String>,
    provider: String,
    model: String,
    input_rate: f64,
    output_rate: f64,
}

impl PriceForm {
    fn new() -> Self {
        Self {
            original_provider: None,
            original_model: None,
            provider: String::new(),
            model: String::new(),
            input_rate: 0.0,
            output_rate: 0.0,
        }
    }

    fn edit(provider: &str, model: &str, entry: &PriceEntry) -> Self {
        Self {
            original_provider: Some(provider.to_string()),
            original_model: Some(model.to_string()),
            provider: provider.to_string(),
            model: model.to_string(),
            input_rate: entry.input_rate,
            output_rate: entry.output_rate,
        }
    }

    fn title(&self) -> &'static str {
        if self.original_provider.is_some() {
            "Edit Price Entry"
        } else {
            "Add Price Entry"
        }
    }

    fn validate(&self) -> Result<(String, String, PriceEntry), String> {
        let provider = self.provider.trim().to_string();
        if provider.is_empty() {
            return Err("Provider must not be empty".to_string());
        }
        let model = self.model.trim().to_string();
        if model.is_empty() {
            return Err("Model must not be empty".to_string());
        }
        if self.input_rate < 0.0 {
            return Err("Input rate must not be negative".to_string());
        }
        if self.output_rate < 0.0 {
            return Err("Output rate must not be negative".to_string());
        }
        Ok((
            provider,
            model,
            PriceEntry {
                input_rate: self.input_rate,
                output_rate: self.output_rate,
            },
        ))
    }
}

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
    price_form: Option<PriceForm>,
    price_delete_confirm: Option<(String, String)>,
    selected_price_row: Option<(String, String)>,
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

    fn save_price_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.price_form.take() else {
            return;
        };
        match form.validate() {
            Ok((provider, model, entry)) => {
                if form.original_provider.is_some() {
                    if let Some(old_provider) = &form.original_provider {
                        if let Some(old_model) = &form.original_model {
                            if old_provider != &provider || old_model != &model {
                                if let Some(models) = self.config.price.get_mut(old_provider) {
                                    models.remove(old_model.as_str());
                                }
                            }
                        }
                    }
                }
                let is_dup = self
                    .config
                    .price
                    .get(&provider)
                    .is_some_and(|m| m.contains_key(&model));
                if is_dup && form.original_provider.is_none() {
                    notifications
                        .error(format!("Price entry for {provider}/{model} already exists"));
                    self.price_form = Some(form);
                    return;
                }
                self.config
                    .price
                    .entry(provider)
                    .or_default()
                    .insert(model, entry);
                self.mark_dirty();
            }
            Err(err) => {
                notifications.error(err);
                self.price_form = Some(form);
            }
        }
    }

    fn delete_price_entry(&mut self, provider: &str, model: &str) {
        if let Some(models) = self.config.price.get_mut(provider) {
            models.remove(model);
            if models.is_empty() {
                self.config.price.remove(provider);
            }
        }
        self.mark_dirty();
    }

    fn flattened_price_rows(price: &PriceTable) -> Vec<(String, String, PriceEntry)> {
        let mut rows: Vec<(String, String, PriceEntry)> = Vec::new();
        for (provider, models) in price {
            for (model, entry) in models {
                rows.push((provider.clone(), model.clone(), entry.clone()));
            }
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        rows
    }

    fn known_providers(&self) -> Vec<String> {
        let mut providers: Vec<String> = self
            .store
            .as_ref()
            .map(|store| {
                store
                    .snapshot()
                    .config
                    .model_providers
                    .keys()
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        providers.sort();
        providers
    }

    fn render_price_section(&mut self, ui: &mut egui::Ui) {
        let mut edit_provider = None;
        let mut edit_model = None;
        let mut delete_provider = None;
        let mut delete_model = None;

        ui.collapsing("Model Pricing", |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(format!(
                        "{} Add Price Entry",
                        regular::PLUS_CIRCLE
                    )))
                    .clicked()
                {
                    self.price_form = Some(PriceForm::new());
                }
                ui.label("Rates are per 1M tokens in USD");
            });
            ui.add_space(4.0);

            let rows = Self::flattened_price_rows(&self.config.price);

            if rows.is_empty() {
                ui.label("No price entries configured. Click Add to create one.");
                return;
            }

            let selected_row = self.selected_price_row.clone();

            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(100.0))
                .column(Column::auto().at_least(140.0))
                .column(Column::auto().at_least(96.0))
                .column(Column::auto().at_least(96.0))
                .min_scrolled_height(PRICE_TABLE_HEIGHT)
                .max_scroll_height(PRICE_TABLE_HEIGHT)
                .sense(egui::Sense::click())
                .header(PRICE_ROW_HEIGHT, |mut header| {
                    header.col(|ui| {
                        ui.strong("Provider");
                    });
                    header.col(|ui| {
                        ui.strong("Model");
                    });
                    header.col(|ui| {
                        ui.strong("Input Rate");
                    });
                    header.col(|ui| {
                        ui.strong("Output Rate");
                    });
                })
                .body(|body| {
                    body.rows(PRICE_ROW_HEIGHT, rows.len(), |mut row| {
                        let idx = row.index();
                        let (provider, model, entry) = &rows[idx];
                        let is_selected =
                            selected_row.as_ref() == Some(&(provider.clone(), model.clone()));

                        row.set_selected(is_selected);
                        row.col(|ui| {
                            ui.label(provider);
                        });
                        row.col(|ui| {
                            ui.label(model);
                        });
                        row.col(|ui| {
                            ui.label(format!("${:.2}", entry.input_rate));
                        });
                        row.col(|ui| {
                            ui.label(format!("${:.2}", entry.output_rate));
                        });

                        let response = row.response();
                        if response.clicked() {
                            self.selected_price_row = if is_selected {
                                None
                            } else {
                                Some((provider.clone(), model.clone()))
                            };
                        }

                        let p = provider.clone();
                        let m = model.clone();
                        response.context_menu(|ui: &mut egui::Ui| {
                            if ui
                                .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                .clicked()
                            {
                                edit_provider = Some(p.clone());
                                edit_model = Some(m.clone());
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
                                delete_provider = Some(p.clone());
                                delete_model = Some(m.clone());
                                ui.close();
                            }
                        });
                    });
                });
        });

        if let (Some(provider), Some(model)) = (edit_provider, edit_model) {
            if let Some(entry) = self
                .config
                .price
                .get(&provider)
                .and_then(|models| models.get(&model))
            {
                self.price_form = Some(PriceForm::edit(&provider, &model, entry));
            }
        }
        if let (Some(provider), Some(model)) = (delete_provider, delete_model) {
            self.price_delete_confirm = Some((provider, model));
        }
    }

    fn render_price_form_window(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        let providers = self.known_providers();
        let Some(form) = self.price_form.as_mut() else {
            return;
        };
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new(form.title())
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(400.0);
                egui::Grid::new("price-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Provider");
                        egui::ComboBox::from_id_salt("price-form-provider")
                            .selected_text(if form.provider.is_empty() {
                                "Select or type"
                            } else {
                                &form.provider
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(form.provider.is_empty(), "(custom)")
                                    .clicked()
                                {
                                    form.provider.clear();
                                }
                                for p in &providers {
                                    if ui.selectable_label(form.provider == *p, p).clicked() {
                                        form.provider = p.clone();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Model");
                        ui.text_edit_singleline(&mut form.model);
                        ui.end_row();

                        ui.label("Input Rate ($/1M tokens)");
                        ui.add(
                            egui::DragValue::new(&mut form.input_rate)
                                .speed(0.01)
                                .range(0.0..=f64::MAX)
                                .custom_formatter(|n, _| format!("${n:.2}"))
                                .custom_parser(|s| s.trim_start_matches('$').parse().ok()),
                        );
                        ui.end_row();

                        ui.label("Output Rate ($/1M tokens)");
                        ui.add(
                            egui::DragValue::new(&mut form.output_rate)
                                .speed(0.01)
                                .range(0.0..=f64::MAX)
                                .custom_formatter(|n, _| format!("${n:.2}"))
                                .custom_parser(|s| s.trim_start_matches('$').parse().ok()),
                        );
                        ui.end_row();
                    });
                ui.add_space(6.0);
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
            self.save_price_form(notifications);
        }
        if cancel_clicked {
            self.price_form = None;
        }
    }

    fn render_price_delete_confirm(&mut self, ui: &mut egui::Ui) {
        let Some((provider, model)) = self.price_delete_confirm.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Delete Price Entry")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(360.0);
                ui.label(
                    RichText::new(format!("Delete price entry for {provider}/{model}?")).strong(),
                );
                ui.add_space(8.0);
                ui.label("This removes the pricing rule from config.toml.");
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
            self.delete_price_entry(&provider, &model);
            self.price_delete_confirm = None;
        }
        if cancelled {
            self.price_delete_confirm = None;
        }
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

                                ui.separator();

                                this.render_price_section(ui);
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

        self.render_price_form_window(ui, notifications);
        self.render_price_delete_confirm(ui);
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
