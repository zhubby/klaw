use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore, EmbeddingConfig};
use klaw_memory::{MemoryError, MemoryStats, SqliteMemoryStatsService};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::thread;
use tokio::runtime::Builder;

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

#[derive(Default)]
pub struct MemoryPanel {
    loaded: bool,
    stats: Option<MemoryStats>,
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<MemoryConfigForm>,
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
                self.stats = Some(stats);
                self.loaded = true;
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
        self.revision = Some(snapshot.revision);
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
        let Some(form) = self.form.as_ref() else {
            return;
        };

        match Self::apply_form(self.config.clone(), form) {
            Ok(next) => match toml::to_string_pretty(&next) {
                Ok(raw) => match store.save_raw_toml(&raw) {
                    Ok(snapshot) => {
                        self.apply_snapshot(snapshot);
                        self.form = None;
                        notifications.success("Memory config saved");
                    }
                    Err(err) => notifications.error(format!("Save failed: {err}")),
                },
                Err(err) => notifications.error(format!("Failed to render config TOML: {err}")),
            },
            Err(err) => notifications.error(err),
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
                ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
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
        });
        ui.separator();

        let Some(stats) = self.stats.as_ref() else {
            ui.label("No memory stats available.");
            return;
        };

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

        ui.separator();
        ui.label("Top Scopes");
        if stats.top_scopes.is_empty() {
            ui.label("No scope data.");
        } else {
            egui::Grid::new("memory-top-scopes-grid")
                .striped(true)
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("Scope");
                    ui.strong("Count");
                    ui.end_row();

                    for scope in &stats.top_scopes {
                        ui.label(&scope.scope);
                        ui.monospace(scope.count.to_string());
                        ui.end_row();
                    }
                });
        }

        self.render_form_window(ui, notifications);
    }
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
}
