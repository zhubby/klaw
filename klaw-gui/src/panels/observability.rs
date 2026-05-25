use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::current_ui_language;
use egui::{Color32, RichText};
use egui_dock::{AllowedSplits, DockArea, DockState, NodeIndex, Style, SurfaceIndex, TabIndex};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{ConfigStore, ObservabilityConfig, PriceEntry, PriceTable};
use klaw_ui_kit::{LocaleDomain, Translator, label_with_hint, toggle::toggle};
use std::collections::HashMap;

const PRICE_TABLE_HEIGHT: f32 = 280.0;
const PRICE_ROW_HEIGHT: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservabilitySection {
    General,
    Metrics,
    Traces,
    Otlp,
    Prometheus,
    Audit,
    LocalStore,
    Pricing,
}

impl ObservabilitySection {
    const ALL: [Self; 8] = [
        Self::General,
        Self::Metrics,
        Self::Traces,
        Self::Otlp,
        Self::Prometheus,
        Self::Audit,
        Self::LocalStore,
        Self::Pricing,
    ];

    fn title_key(self) -> &'static str {
        match self {
            Self::General => "obs-section-general",
            Self::Metrics => "obs-section-metrics",
            Self::Traces => "obs-section-traces",
            Self::Otlp => "obs-section-otlp",
            Self::Prometheus => "obs-section-prometheus",
            Self::Audit => "obs-section-audit",
            Self::LocalStore => "obs-section-local-store",
            Self::Pricing => "obs-section-pricing",
        }
    }

    fn tab_id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Metrics => "metrics",
            Self::Traces => "traces",
            Self::Otlp => "otlp",
            Self::Prometheus => "prometheus",
            Self::Audit => "audit",
            Self::LocalStore => "local-store",
            Self::Pricing => "pricing",
        }
    }
}

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

    fn title(&self, t: &Translator) -> String {
        if self.original_provider.is_some() {
            t.text("obs-price-form-title-edit")
        } else {
            t.text("obs-price-form-title-add")
        }
    }

    fn validate(&self, t: &Translator) -> Result<(String, String, PriceEntry), String> {
        let provider = self.provider.trim().to_string();
        if provider.is_empty() {
            return Err(t.text("obs-validation-price-provider-empty"));
        }
        let model = self.model.trim().to_string();
        if model.is_empty() {
            return Err(t.text("obs-validation-price-model-empty"));
        }
        if self.input_rate < 0.0 {
            return Err(t.text("obs-validation-price-input-negative"));
        }
        if self.output_rate < 0.0 {
            return Err(t.text("obs-validation-price-output-negative"));
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

pub struct ObservabilityPanel {
    store: Option<ConfigStore>,
    config: ObservabilityConfig,
    active_section: ObservabilitySection,
    section_dock_state: DockState<ObservabilitySection>,
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

impl Default for ObservabilityPanel {
    fn default() -> Self {
        let active_section = ObservabilitySection::General;
        Self {
            store: None,
            config: ObservabilityConfig::default(),
            active_section,
            section_dock_state: Self::section_dock_state(active_section),
            loaded: false,
            dirty: false,
            endpoint_buffer: String::new(),
            service_name_buffer: String::new(),
            service_version_buffer: String::new(),
            prometheus_port_buffer: String::new(),
            prometheus_path_buffer: String::new(),
            audit_output_path_buffer: String::new(),
            sample_rate_buffer: String::new(),
            export_interval_buffer: String::new(),
            local_store_retention_days_buffer: String::new(),
            local_store_flush_interval_buffer: String::new(),
            price_form: None,
            price_delete_confirm: None,
            selected_price_row: None,
        }
    }
}

impl ObservabilityPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn section_dock_state(active_section: ObservabilitySection) -> DockState<ObservabilitySection> {
        let mut dock_state = DockState::new(ObservabilitySection::ALL.to_vec());
        let active_index = ObservabilitySection::ALL
            .iter()
            .position(|section| *section == active_section)
            .unwrap_or_default();
        dock_state.set_active_tab((
            SurfaceIndex::main(),
            NodeIndex::root(),
            TabIndex(active_index),
        ));
        dock_state
    }

    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        let t = Self::translator();
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.config = snapshot.config.observability.clone();
                self.store = Some(store);
                self.sync_buffers_from_config();
                self.loaded = true;
                notifications.success(t.text("obs-notify-config-loaded"));
            }
            Err(err) => {
                notifications.error(t.text_args(
                    "obs-notify-config-load-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
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

    fn parse_config_from_buffers(&self, t: &Translator) -> Result<ObservabilityConfig, String> {
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
            .map_err(|_| t.text("obs-validation-prometheus-port-invalid"))?;
        if prometheus_port == 0 {
            return Err(t.text("obs-validation-prometheus-port-zero"));
        }
        next.prometheus.listen_port = prometheus_port;

        let sample_rate = self
            .sample_rate_buffer
            .trim()
            .parse::<f64>()
            .map_err(|_| t.text("obs-validation-sample-rate-invalid"))?;
        if !(0.0..=1.0).contains(&sample_rate) {
            return Err(t.text("obs-validation-sample-rate-range"));
        }
        next.traces.sample_rate = sample_rate;

        let export_interval_seconds = self
            .export_interval_buffer
            .trim()
            .parse::<u64>()
            .map_err(|_| t.text("obs-validation-export-interval-invalid"))?;
        if export_interval_seconds == 0 {
            return Err(t.text("obs-validation-export-interval-zero"));
        }
        next.metrics.export_interval_seconds = export_interval_seconds;

        let retention_days = self
            .local_store_retention_days_buffer
            .trim()
            .parse::<u16>()
            .map_err(|_| t.text("obs-validation-retention-days-invalid"))?;
        if retention_days == 0 {
            return Err(t.text("obs-validation-retention-days-zero"));
        }
        next.local_store.retention_days = retention_days;

        let flush_interval_seconds = self
            .local_store_flush_interval_buffer
            .trim()
            .parse::<u64>()
            .map_err(|_| t.text("obs-validation-flush-interval-invalid"))?;
        if flush_interval_seconds == 0 {
            return Err(t.text("obs-validation-flush-interval-zero"));
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
        let t = Self::translator();
        let Some(store) = self.store.clone() else {
            notifications.error(t.text("obs-notify-store-unavailable"));
            return;
        };
        let parsed = match self.parse_config_from_buffers(&t) {
            Ok(parsed) => parsed,
            Err(err) => {
                notifications.error(t.text_args(
                    "obs-notify-save-parse-failed",
                    HashMap::from([("error", err)]),
                ));
                return;
            }
        };
        match store.save_observability_config(&parsed) {
            Ok(_) => {
                self.config = parsed;
                self.sync_buffers_from_config();
                self.dirty = false;
                notifications.success(t.text("obs-notify-config-saved"));
            }
            Err(err) => {
                notifications.error(t.text_args(
                    "obs-notify-save-write-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
            }
        }
    }

    fn handle_reload(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(store) = self.store.clone() else {
            notifications.error(t.text("obs-notify-store-unavailable"));
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.config = snapshot.config.observability.clone();
                self.sync_buffers_from_config();
                self.dirty = false;
                notifications.success(t.text("obs-notify-config-reloaded"));
            }
            Err(err) => {
                notifications.error(t.text_args(
                    "obs-notify-reload-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
            }
        }
    }

    fn status_indicator(&self, t: &Translator) -> (bool, String, String) {
        (
            self.config.enabled,
            t.text("obs-status-enabled"),
            t.text("obs-status-disabled"),
        )
    }

    fn save_price_form(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(form) = self.price_form.take() else {
            return;
        };
        match form.validate(&t) {
            Ok((provider, model, entry)) => {
                if let Some(old_provider) = &form.original_provider
                    && let Some(old_model) = &form.original_model
                    && (old_provider != &provider || old_model != &model)
                    && let Some(models) = self.config.price.get_mut(old_provider)
                {
                    models.remove(old_model.as_str());
                }
                let is_dup = self
                    .config
                    .price
                    .get(&provider)
                    .is_some_and(|m| m.contains_key(&model));
                if is_dup && form.original_provider.is_none() {
                    notifications.error(t.text_args(
                        "obs-notify-price-duplicate",
                        HashMap::from([("provider", provider.clone()), ("model", model.clone())]),
                    ));
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

    fn render_general_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-enabled"),
                &t.text("obs-field-enabled-hint"),
            );
            if ui.add(toggle(&mut self.config.enabled)).changed() {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-service-name"),
                &t.text("obs-field-service-name-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.service_name_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-service-version"),
                &t.text("obs-field-service-version-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.service_version_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
    }

    fn render_metrics_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-enabled"),
                &t.text("obs-field-enabled-hint"),
            );
            if ui.add(toggle(&mut self.config.metrics.enabled)).changed() {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-export-interval"),
                &t.text("obs-field-export-interval-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.export_interval_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
    }

    fn render_traces_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-enabled"),
                &t.text("obs-field-enabled-hint"),
            );
            if ui.add(toggle(&mut self.config.traces.enabled)).changed() {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-sample-rate"),
                &t.text("obs-field-sample-rate-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.sample_rate_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
    }

    fn render_otlp_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-enabled"),
                &t.text("obs-field-enabled-hint"),
            );
            if ui.add(toggle(&mut self.config.otlp.enabled)).changed() {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-endpoint"),
                &t.text("obs-field-endpoint-hint"),
            );
            if ui.text_edit_singleline(&mut self.endpoint_buffer).changed() {
                self.mark_dirty();
            }
        });
        ui.label(t.text("obs-field-headers"));
        if self.config.otlp.headers.is_empty() {
            ui.label(t.text("obs-field-headers-none"));
        } else {
            for (key, value) in &self.config.otlp.headers {
                ui.label(format!("  {key}: {value}"));
            }
        }
    }

    fn render_prometheus_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-enabled"),
                &t.text("obs-field-enabled-hint"),
            );
            if ui
                .add(toggle(&mut self.config.prometheus.enabled))
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-listen-port"),
                &t.text("obs-field-listen-port-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.prometheus_port_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-path"),
                &t.text("obs-field-path-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.prometheus_path_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
    }

    fn render_audit_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-enabled"),
                &t.text("obs-field-enabled-hint"),
            );
            if ui.add(toggle(&mut self.config.audit.enabled)).changed() {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-output-path"),
                &t.text("obs-field-output-path-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.audit_output_path_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
    }

    fn render_local_store_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-enabled"),
                &t.text("obs-field-enabled-hint"),
            );
            if ui
                .add(toggle(&mut self.config.local_store.enabled))
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-retention-days"),
                &t.text("obs-field-retention-days-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.local_store_retention_days_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
        ui.horizontal(|ui| {
            label_with_hint(
                ui,
                &t.text("obs-field-flush-interval"),
                &t.text("obs-field-flush-interval-hint"),
            );
            if ui
                .text_edit_singleline(&mut self.local_store_flush_interval_buffer)
                .changed()
            {
                self.mark_dirty();
            }
        });
    }

    fn render_price_section(&mut self, ui: &mut egui::Ui, t: &Translator) {
        let mut edit_provider = None;
        let mut edit_model = None;
        let mut delete_provider = None;
        let mut delete_model = None;

        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(t.text_args(
                    "obs-price-btn-add",
                    HashMap::from([("icon", regular::PLUS_CIRCLE.to_string())]),
                )))
                .clicked()
            {
                self.price_form = Some(PriceForm::new());
            }
            ui.label(t.text("obs-price-rates-note"));
        });
        ui.add_space(4.0);

        let rows = Self::flattened_price_rows(&self.config.price);

        if rows.is_empty() {
            ui.label(t.text("obs-price-no-entries"));
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
                    ui.strong(t.text("obs-price-col-provider"));
                });
                header.col(|ui| {
                    ui.strong(t.text("obs-price-col-model"));
                });
                header.col(|ui| {
                    ui.strong(t.text("obs-price-col-input-rate"));
                });
                header.col(|ui| {
                    ui.strong(t.text("obs-price-col-output-rate"));
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
                            .button(t.text_args(
                                "obs-price-ctx-edit",
                                HashMap::from([("icon", regular::PENCIL_SIMPLE.to_string())]),
                            ))
                            .clicked()
                        {
                            edit_provider = Some(p.clone());
                            edit_model = Some(m.clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add(egui::Button::new(
                                RichText::new(t.text_args(
                                    "obs-price-ctx-delete",
                                    HashMap::from([("icon", regular::TRASH.to_string())]),
                                ))
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

        if let (Some(provider), Some(model)) = (edit_provider, edit_model)
            && let Some(entry) = self
                .config
                .price
                .get(&provider)
                .and_then(|models| models.get(&model))
        {
            self.price_form = Some(PriceForm::edit(&provider, &model, entry));
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
        let t = Self::translator();
        let providers = self.known_providers();
        let Some(form) = self.price_form.as_mut() else {
            return;
        };
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        let placeholder = t.text("obs-price-form-provider-placeholder");
        let custom_label = t.text("obs-price-form-provider-custom");

        egui::Window::new(form.title(&t))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(400.0);
                egui::Grid::new("price-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t.text("obs-price-form-provider"));
                        egui::ComboBox::from_id_salt("price-form-provider")
                            .selected_text(if form.provider.is_empty() {
                                placeholder.as_str()
                            } else {
                                &form.provider
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        form.provider.is_empty(),
                                        custom_label.as_str(),
                                    )
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

                        ui.label(t.text("obs-price-form-model"));
                        ui.text_edit_singleline(&mut form.model);
                        ui.end_row();

                        ui.label(t.text("obs-price-form-input-rate"));
                        ui.add(
                            egui::DragValue::new(&mut form.input_rate)
                                .speed(0.01)
                                .range(0.0..=f64::MAX)
                                .custom_formatter(|n, _| format!("${n:.2}"))
                                .custom_parser(|s| s.trim_start_matches('$').parse().ok()),
                        );
                        ui.end_row();

                        ui.label(t.text("obs-price-form-output-rate"));
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
                    if ui.button(t.text("obs-price-form-save")).clicked() {
                        save_clicked = true;
                    }
                    if ui.button(t.text("obs-price-form-cancel")).clicked() {
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
        let t = Self::translator();
        let Some((provider, model)) = self.price_delete_confirm.clone() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(t.text("obs-price-delete-title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(360.0);
                ui.label(
                    RichText::new(t.text_args(
                        "obs-price-delete-prompt",
                        HashMap::from([("provider", provider.clone()), ("model", model.clone())]),
                    ))
                    .strong(),
                );
                ui.add_space(8.0);
                ui.label(t.text("obs-price-delete-warning"));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(
                            RichText::new(t.text_args(
                                "obs-price-delete-btn",
                                HashMap::from([("icon", regular::TRASH.to_string())]),
                            ))
                            .color(ui.visuals().warn_fg_color),
                        ))
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button(t.text("obs-price-delete-cancel")).clicked() {
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

    fn render_section_dock(&mut self, ui: &mut egui::Ui, t: &Translator) {
        let mut dock_state = std::mem::replace(
            &mut self.section_dock_state,
            Self::section_dock_state(self.active_section),
        );
        let mut style = Style::from_egui(ui.style().as_ref());
        style.tab_bar.show_scroll_bar_on_overflow = false;

        DockArea::new(&mut dock_state)
            .id(egui::Id::new("observability-section-dock"))
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
                &mut ObservabilitySectionTabViewer {
                    panel: self,
                    translator: t,
                },
            );

        self.section_dock_state = dock_state;
    }
}

struct ObservabilitySectionTabViewer<'a> {
    panel: &'a mut ObservabilityPanel,
    translator: &'a Translator,
}

impl egui_dock::TabViewer for ObservabilitySectionTabViewer<'_> {
    type Tab = ObservabilitySection;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        self.translator.text(tab.title_key()).into()
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("observability-section-tab", tab.tab_id()))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        self.panel.active_section = *tab;
        egui::ScrollArea::vertical()
            .id_salt(("observability-section-scroll", tab.tab_id()))
            .auto_shrink([false, false])
            .show(ui, |ui| match *tab {
                ObservabilitySection::General => {
                    self.panel.render_general_section(ui, self.translator)
                }
                ObservabilitySection::Metrics => {
                    self.panel.render_metrics_section(ui, self.translator)
                }
                ObservabilitySection::Traces => {
                    self.panel.render_traces_section(ui, self.translator)
                }
                ObservabilitySection::Otlp => self.panel.render_otlp_section(ui, self.translator),
                ObservabilitySection::Prometheus => {
                    self.panel.render_prometheus_section(ui, self.translator)
                }
                ObservabilitySection::Audit => self.panel.render_audit_section(ui, self.translator),
                ObservabilitySection::LocalStore => {
                    self.panel.render_local_store_section(ui, self.translator)
                }
                ObservabilitySection::Pricing => {
                    self.panel.render_price_section(ui, self.translator)
                }
            });
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        if response.clicked() {
            self.panel.active_section = *tab;
        }
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
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

        let t = Self::translator();
        let (status_enabled, enabled_label, disabled_label) = self.status_indicator(&t);

        let mut render_strip = |ui: &mut egui::Ui, this: &mut ObservabilityPanel| {
            ui.heading(ctx.tab_title);
            ui.horizontal(|ui| {
                ui.label(t.text("obs-status-label"));
                render_boolean_status(ui, status_enabled, &enabled_label, &disabled_label);
                if this.is_dirty() {
                    ui.colored_label(Color32::YELLOW, t.text("obs-status-unsaved"));
                }
            });
            ui.horizontal(|ui| {
                if ui
                    .button(t.text_args(
                        "obs-btn-save",
                        HashMap::from([("icon", regular::FLOPPY_DISK.to_string())]),
                    ))
                    .clicked()
                {
                    this.handle_save(notifications);
                }
                if ui
                    .button(t.text_args(
                        "obs-btn-reload",
                        HashMap::from([("icon", regular::ARROW_COUNTER_CLOCKWISE.to_string())]),
                    ))
                    .clicked()
                {
                    this.handle_reload(notifications);
                }
                ui.label(t.text("obs-note-restart"));
            });

            ui.separator();

            StripBuilder::new(ui)
                .size(Size::remainder().at_least(200.0))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        this.render_section_dock(ui, &t);
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
