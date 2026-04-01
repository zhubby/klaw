use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigError, ConfigSnapshot, ConfigStore, EmbeddingConfig};
use klaw_memory::{MemoryError, MemoryRecord, MemoryStats, SqliteMemoryStatsService};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::thread;
use tokio::runtime::Builder;

const TOP_SCOPES_TABLE_MAX_HEIGHT: f32 = 240.0;
const SCOPE_DETAIL_WINDOW_WIDTH: f32 = 960.0;
const SCOPE_DETAIL_WINDOW_HEIGHT: f32 = 540.0;
const SCOPE_DETAIL_WINDOW_MARGIN: f32 = 48.0;
const SCOPE_DETAIL_MIN_WIDTH: f32 = 480.0;
const SCOPE_DETAIL_MIN_HEIGHT: f32 = 320.0;
const SCOPE_DETAIL_TABLE_MIN_WIDTH: f32 = 1360.0;

#[derive(Debug, Clone)]
struct MemoryConfigForm {
    enabled: bool,
    provider: String,
    model: String,
}

impl MemoryConfigForm {
    fn from_config(config: &AppConfig) -> Self {
        let provider =
            Self::resolve_provider(config, Some(config.memory.embedding.provider.as_str()));
        let model = if config.memory.embedding.model.trim().is_empty() {
            Self::provider_default_model(config, &provider)
        } else {
            config.memory.embedding.model.trim().to_string()
        };

        Self {
            enabled: config.memory.embedding.enabled,
            provider,
            model,
        }
    }

    fn resolve_provider(config: &AppConfig, preferred: Option<&str>) -> String {
        let preferred = preferred.unwrap_or_default().trim();
        if !preferred.is_empty() && config.model_providers.contains_key(preferred) {
            return preferred.to_string();
        }

        let active = config.model_provider.trim();
        if !active.is_empty() && config.model_providers.contains_key(active) {
            return active.to_string();
        }

        config
            .model_providers
            .keys()
            .next()
            .cloned()
            .unwrap_or_default()
    }

    fn provider_default_model(config: &AppConfig, provider: &str) -> String {
        config
            .model_providers
            .get(provider)
            .map(|provider| provider.default_model.trim().to_string())
            .filter(|model| !model.is_empty())
            .unwrap_or_default()
    }

    fn set_provider(&mut self, config: &AppConfig, provider: String) {
        self.provider = provider;
        self.model = Self::provider_default_model(config, &self.provider);
    }
}

#[derive(Debug, Clone)]
struct ScopeDetailWindow {
    scope: String,
    records: Vec<MemoryRecord>,
}

#[derive(Default)]
pub struct MemoryPanel {
    loaded: bool,
    stats: Option<MemoryStats>,
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    config: AppConfig,
    form: Option<MemoryConfigForm>,
    stats_window_open: bool,
    selected_scope: Option<String>,
    scope_detail: Option<ScopeDetailWindow>,
}

impl MemoryPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        match run_memory_task(|service| async move { service.collect(8).await }) {
            Ok(stats) => {
                let selected_scope = self.selected_scope.clone();
                self.stats = Some(stats);
                self.loaded = true;
                if let Some(scope) = selected_scope
                    && self.stats.as_ref().is_none_or(|stats| {
                        !stats.top_scopes.iter().any(|item| item.scope == scope)
                    })
                {
                    self.selected_scope = None;
                    self.scope_detail = None;
                }
            }
            Err(err) => notifications.error(format!("Failed to load memory stats: {err}")),
        }
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
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Path: {}", path.display()),
            None => "Path: (not loaded)".to_string(),
        }
    }

    fn available_provider_ids(&self) -> Vec<String> {
        self.config.model_providers.keys().cloned().collect()
    }

    fn open_config_form(&mut self) {
        self.form = Some(MemoryConfigForm::from_config(&self.config));
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        let Some(form) = self.form.clone() else {
            return;
        };

        match store.update_config(|config| {
            let next =
                Self::apply_form(config.clone(), &form).map_err(ConfigError::InvalidConfig)?;
            *config = next;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.form = None;
                notifications.success("Memory config saved");
            }
            Err(err) => notifications.error(format!("Save failed: {err}")),
        }
    }

    fn apply_form(mut config: AppConfig, form: &MemoryConfigForm) -> Result<AppConfig, String> {
        let provider = form.provider.trim();
        if provider.is_empty() {
            return Err("Provider cannot be empty".to_string());
        }
        if !config.model_providers.contains_key(provider) {
            return Err(format!("Provider '{provider}' is not available"));
        }

        let model = form.model.trim();
        if model.is_empty() {
            return Err("Model cannot be empty".to_string());
        }

        config.memory.embedding = EmbeddingConfig {
            enabled: form.enabled,
            provider: provider.to_string(),
            model: model.to_string(),
        };
        Ok(config)
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let provider_ids = self.available_provider_ids();
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new("Memory Config")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(420.0);
                ui.label(Self::status_label(self.config_path.as_deref()));
                ui.separator();

                egui::Grid::new("memory-config-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Enabled");
                        ui.checkbox(&mut form.enabled, "");
                        ui.end_row();

                        ui.label("Provider");
                        egui::ComboBox::from_id_salt("memory-config-provider")
                            .selected_text(if form.provider.is_empty() {
                                "Select provider"
                            } else {
                                form.provider.as_str()
                            })
                            .show_ui(ui, |ui| {
                                for provider_id in &provider_ids {
                                    let is_selected = form.provider == *provider_id;
                                    if ui.selectable_label(is_selected, provider_id).clicked() {
                                        form.set_provider(&self.config, provider_id.clone());
                                        ui.close();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Model");
                        ui.text_edit_singleline(&mut form.model);
                        ui.end_row();
                    });

                if provider_ids.is_empty() {
                    ui.colored_label(
                        ui.style().visuals.warn_fg_color,
                        "No providers are configured in config.toml.",
                    );
                }

                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!provider_ids.is_empty(), egui::Button::new("Save"))
                        .clicked()
                    {
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

    fn open_scope_detail(&mut self, scope: &str, notifications: &mut NotificationCenter) {
        let scope = scope.to_string();
        match run_memory_task({
            let scope = scope.clone();
            move |service| async move { service.list_scope_records(&scope).await }
        }) {
            Ok(records) => {
                self.selected_scope = Some(scope.clone());
                self.scope_detail = Some(ScopeDetailWindow { scope, records });
            }
            Err(err) => notifications.error(format!("Failed to load scope detail: {err}")),
        }
    }

    fn render_scope_detail_window(&mut self, ctx: &egui::Context) {
        let Some(detail) = self.scope_detail.clone() else {
            return;
        };

        let mut open = true;
        let window_size = clamp_scope_detail_window_size(ctx.content_rect().size());
        egui::Window::new(format!("Scope Detail: {}", detail.scope))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .fixed_size(window_size)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(window_size.x);
                ui.label(format!("Scope: {}", detail.scope));
                ui.label(format!("Records: {}", detail.records.len()));
                ui.separator();

                egui::ScrollArea::both()
                    .id_salt(("memory-scope-detail", detail.scope.as_str()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(SCOPE_DETAIL_TABLE_MIN_WIDTH);
                        TableBuilder::new(ui)
                            .striped(true)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(Column::auto().at_least(220.0))
                            .column(Column::auto().at_least(70.0))
                            .column(Column::remainder().at_least(340.0))
                            .column(Column::remainder().at_least(360.0))
                            .column(Column::auto().at_least(170.0))
                            .column(Column::auto().at_least(170.0))
                            .header(22.0, |mut header| {
                                header.col(|ui| {
                                    ui.strong("ID");
                                });
                                header.col(|ui| {
                                    ui.strong("Pinned");
                                });
                                header.col(|ui| {
                                    ui.strong("Content");
                                });
                                header.col(|ui| {
                                    ui.strong("Metadata");
                                });
                                header.col(|ui| {
                                    ui.strong("Created At");
                                });
                                header.col(|ui| {
                                    ui.strong("Updated At");
                                });
                            })
                            .body(|body| {
                                body.rows(24.0, detail.records.len(), |mut row| {
                                    let record = &detail.records[row.index()];
                                    let metadata = serde_json::to_string(&record.metadata)
                                        .unwrap_or_else(|_| "<invalid metadata>".to_string());

                                    row.col(|ui| {
                                        ui.monospace(&record.id);
                                    });
                                    row.col(|ui| {
                                        ui.monospace(if record.pinned { "yes" } else { "no" });
                                    });
                                    row.col(|ui| {
                                        ui.label(&record.content);
                                    });
                                    row.col(|ui| {
                                        ui.monospace(metadata);
                                    });
                                    row.col(|ui| {
                                        ui.monospace(format_timestamp_millis(record.created_at_ms));
                                    });
                                    row.col(|ui| {
                                        ui.monospace(format_timestamp_millis(record.updated_at_ms));
                                    });
                                });
                            });
                    });
            });

        if !open {
            self.scope_detail = None;
        }
    }

    fn render_stats_window(&mut self, ctx: &egui::Context) {
        let Some(stats) = self.stats.clone() else {
            return;
        };

        let mut open = self.stats_window_open;
        egui::Window::new("Memory Info")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(480.0);
                render_memory_stats_grid(ui, &stats);
            });
        self.stats_window_open = open;
    }
}

impl PanelRenderer for MemoryPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);
        self.ensure_store_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications);
            }
            if ui.button("Config").clicked() {
                self.open_config_form();
            }
            if ui.button(format!("{} Info", regular::INFO)).clicked() {
                self.stats_window_open = true;
            }
        });
        ui.separator();

        let Some(stats) = self.stats.clone() else {
            ui.label("No memory stats available.");
            return;
        };
        ui.label("Top Scopes");
        if stats.top_scopes.is_empty() {
            ui.label("No scope data.");
        } else {
            let selected_scope = self.selected_scope.clone();
            let mut open_detail_scope = None;

            ui.horizontal(|ui| {
                let detail_enabled = selected_scope.is_some();
                let selected_label = selected_scope
                    .as_deref()
                    .map(|scope| format!("Selected: {scope}"))
                    .unwrap_or_else(|| "Selected: -".to_string());
                ui.label(selected_label);
                if ui
                    .add_enabled(
                        detail_enabled,
                        egui::Button::new(format!("{} Detail", regular::FILE_TEXT)),
                    )
                    .clicked()
                {
                    open_detail_scope = selected_scope.clone();
                }
            });

            let table_width = ui.available_width();
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .max_width(table_width)
                .max_height(TOP_SCOPES_TABLE_MAX_HEIGHT)
                .show(ui, |ui| {
                    ui.set_min_width(table_width);
                    TableBuilder::new(ui)
                        .striped(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::remainder().at_least(320.0))
                        .column(Column::auto().at_least(80.0))
                        .min_scrolled_height(0.0)
                        .max_scroll_height(TOP_SCOPES_TABLE_MAX_HEIGHT)
                        .sense(egui::Sense::click())
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("Scope");
                            });
                            header.col(|ui| {
                                ui.strong("Count");
                            });
                        })
                        .body(|body| {
                            body.rows(22.0, stats.top_scopes.len(), |mut row| {
                                let scope = &stats.top_scopes[row.index()];
                                let is_selected =
                                    self.selected_scope.as_deref() == Some(scope.scope.as_str());

                                row.set_selected(is_selected);
                                row.col(|ui| {
                                    ui.label(&scope.scope);
                                });
                                row.col(|ui| {
                                    ui.monospace(scope.count.to_string());
                                });

                                let response = row.response();
                                if response.clicked()
                                    || (response.secondary_clicked() && !is_selected)
                                {
                                    self.selected_scope = Some(scope.scope.clone());
                                }
                                if response.double_clicked() {
                                    self.selected_scope = Some(scope.scope.clone());
                                    open_detail_scope = Some(scope.scope.clone());
                                }

                                let scope_name = scope.scope.clone();
                                response.context_menu(|ui| {
                                    if ui
                                        .button(format!("{} Detail", regular::FILE_TEXT))
                                        .clicked()
                                    {
                                        self.selected_scope = Some(scope_name.clone());
                                        open_detail_scope = Some(scope_name.clone());
                                        ui.close();
                                    }
                                });
                            });
                        });
                });

            if let Some(scope) = open_detail_scope {
                self.open_scope_detail(&scope, notifications);
            }
        }

        self.render_form_window(ui, notifications);
        self.render_scope_detail_window(ui.ctx());
        if self.stats_window_open {
            self.render_stats_window(ui.ctx());
        }
    }
}

fn render_memory_stats_grid(ui: &mut egui::Ui, stats: &MemoryStats) {
    egui::Grid::new("memory-stats-grid")
        .num_columns(2)
        .spacing([14.0, 8.0])
        .show(ui, |ui| {
            ui.label("Total Records");
            ui.monospace(stats.total_records.to_string());
            ui.end_row();

            ui.label("Pinned Records");
            ui.monospace(stats.pinned_records.to_string());
            ui.end_row();

            ui.label("Embedded Records");
            ui.monospace(stats.embedded_records.to_string());
            ui.end_row();

            ui.label("Distinct Scopes");
            ui.monospace(stats.distinct_scopes.to_string());
            ui.end_row();

            ui.label("Updated Last 24h");
            ui.monospace(stats.updated_last_24h.to_string());
            ui.end_row();

            ui.label("Updated Last 7d");
            ui.monospace(stats.updated_last_7d.to_string());
            ui.end_row();

            ui.label("FTS Enabled");
            ui.monospace(if stats.fts_enabled { "yes" } else { "no" });
            ui.end_row();

            ui.label("Vector Index Enabled");
            ui.monospace(if stats.vector_index_enabled {
                "yes"
            } else {
                "no"
            });
            ui.end_row();

            ui.label("Avg Content Length");
            ui.monospace(
                stats
                    .avg_content_len
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "-".to_string()),
            );
            ui.end_row();

            ui.label("Created Min");
            ui.monospace(
                stats
                    .created_min_ms
                    .map(format_timestamp_millis)
                    .unwrap_or_else(|| "-".to_string()),
            );
            ui.end_row();

            ui.label("Created Max");
            ui.monospace(
                stats
                    .created_max_ms
                    .map(format_timestamp_millis)
                    .unwrap_or_else(|| "-".to_string()),
            );
            ui.end_row();

            ui.label("Updated Max");
            ui.monospace(
                stats
                    .updated_max_ms
                    .map(format_timestamp_millis)
                    .unwrap_or_else(|| "-".to_string()),
            );
            ui.end_row();
        });
}

fn clamp_scope_detail_window_size(available: egui::Vec2) -> egui::Vec2 {
    let max_width = (available.x - SCOPE_DETAIL_WINDOW_MARGIN).max(SCOPE_DETAIL_MIN_WIDTH);
    let max_height = (available.y - SCOPE_DETAIL_WINDOW_MARGIN).max(SCOPE_DETAIL_MIN_HEIGHT);
    egui::vec2(
        SCOPE_DETAIL_WINDOW_WIDTH.min(max_width),
        SCOPE_DETAIL_WINDOW_HEIGHT.min(max_height),
    )
}

fn run_memory_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SqliteMemoryStatsService) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, MemoryError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let service = SqliteMemoryStatsService::open_default()
                .await
                .map_err(|err| format!("failed to open memory stats service: {err}"))?;
            op(service)
                .await
                .map_err(|err| format!("memory stats operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("memory stats operation thread panicked".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::ModelProviderConfig;
    use std::collections::BTreeMap;

    fn test_config() -> AppConfig {
        let mut model_providers = BTreeMap::new();
        model_providers.insert(
            "openai".to_string(),
            ModelProviderConfig {
                default_model: "gpt-4.1-mini".to_string(),
                ..ModelProviderConfig::default()
            },
        );
        model_providers.insert(
            "anthropic".to_string(),
            ModelProviderConfig {
                default_model: "claude-3-7-sonnet-latest".to_string(),
                ..ModelProviderConfig::default()
            },
        );

        AppConfig {
            model_provider: "openai".to_string(),
            model_providers,
            ..AppConfig::default()
        }
    }

    #[test]
    fn config_form_uses_existing_memory_values() {
        let mut config = test_config();
        config.memory.embedding.enabled = true;
        config.memory.embedding.provider = "anthropic".to_string();
        config.memory.embedding.model = "custom-embed".to_string();

        let form = MemoryConfigForm::from_config(&config);

        assert!(form.enabled);
        assert_eq!(form.provider, "anthropic");
        assert_eq!(form.model, "custom-embed");
    }

    #[test]
    fn config_form_falls_back_to_active_provider_and_default_model() {
        let mut config = test_config();
        config.memory.embedding.enabled = false;
        config.memory.embedding.provider = "missing".to_string();
        config.memory.embedding.model.clear();

        let form = MemoryConfigForm::from_config(&config);

        assert!(!form.enabled);
        assert_eq!(form.provider, "openai");
        assert_eq!(form.model, "gpt-4.1-mini");
    }

    #[test]
    fn selecting_provider_updates_model_to_provider_default() {
        let config = test_config();
        let mut form = MemoryConfigForm {
            enabled: false,
            provider: "openai".to_string(),
            model: "custom".to_string(),
        };

        form.set_provider(&config, "anthropic".to_string());

        assert_eq!(form.provider, "anthropic");
        assert_eq!(form.model, "claude-3-7-sonnet-latest");
    }

    #[test]
    fn apply_form_updates_memory_embedding_config() {
        let config = test_config();
        let form = MemoryConfigForm {
            enabled: true,
            provider: "anthropic".to_string(),
            model: "text-embedding-custom".to_string(),
        };

        let updated = MemoryPanel::apply_form(config, &form).expect("form should apply");

        assert!(updated.memory.embedding.enabled);
        assert_eq!(updated.memory.embedding.provider, "anthropic");
        assert_eq!(updated.memory.embedding.model, "text-embedding-custom");
    }

    #[test]
    fn apply_form_rejects_unknown_provider() {
        let config = test_config();
        let form = MemoryConfigForm {
            enabled: false,
            provider: "missing".to_string(),
            model: "text-embedding-3-small".to_string(),
        };

        let err = MemoryPanel::apply_form(config, &form).expect_err("provider should be rejected");

        assert!(err.contains("not available"));
    }

    #[test]
    fn scope_detail_window_size_clamps_to_available_space() {
        let size = clamp_scope_detail_window_size(egui::vec2(720.0, 420.0));

        assert_eq!(size, egui::vec2(672.0, 372.0));
    }

    #[test]
    fn scope_detail_window_size_uses_default_when_space_allows() {
        let size = clamp_scope_detail_window_size(egui::vec2(1600.0, 900.0));

        assert_eq!(
            size,
            egui::vec2(SCOPE_DETAIL_WINDOW_WIDTH, SCOPE_DETAIL_WINDOW_HEIGHT)
        );
    }
}
