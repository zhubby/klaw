use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui_extras::{Column, TableBuilder};
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore, ModelProviderConfig};
use klaw_llm::OpenAiWireApi;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct ProviderForm {
    original_id: Option<String>,
    id: String,
    name: String,
    base_url: String,
    wire_api: String,
    default_model: String,
    tokenizer_path: String,
    proxy: bool,
    env_key: String,
    api_key: String,
    set_as_active: bool,
}

impl ProviderForm {
    fn new() -> Self {
        let default = ModelProviderConfig::default();
        Self {
            original_id: None,
            id: String::new(),
            name: default.name.unwrap_or_default(),
            base_url: default.base_url,
            wire_api: default.wire_api,
            default_model: default.default_model,
            tokenizer_path: default.tokenizer_path.unwrap_or_default(),
            proxy: default.proxy,
            env_key: default.env_key.unwrap_or_default(),
            api_key: default.api_key.unwrap_or_default(),
            set_as_active: false,
        }
    }

    fn edit(id: &str, provider: &ModelProviderConfig, active_provider: &str) -> Self {
        Self {
            original_id: Some(id.to_string()),
            id: id.to_string(),
            name: provider.name.clone().unwrap_or_default(),
            base_url: provider.base_url.clone(),
            wire_api: provider.wire_api.clone(),
            default_model: provider.default_model.clone(),
            tokenizer_path: provider.tokenizer_path.clone().unwrap_or_default(),
            proxy: provider.proxy,
            env_key: provider.env_key.clone().unwrap_or_default(),
            api_key: provider.api_key.clone().unwrap_or_default(),
            set_as_active: id == active_provider,
        }
    }

    fn title(&self) -> &'static str {
        if self.original_id.is_some() {
            "Edit Provider"
        } else {
            "Add Provider"
        }
    }

    fn normalized_id(&self) -> String {
        self.id.trim().to_string()
    }

    fn to_config(&self) -> ModelProviderConfig {
        let name = self.name.trim();
        let env_key = self.env_key.trim();
        let api_key = self.api_key.trim();
        let tokenizer_path = self.tokenizer_path.trim();
        ModelProviderConfig {
            name: (!name.is_empty()).then(|| name.to_string()),
            base_url: self.base_url.trim().to_string(),
            wire_api: self.wire_api.trim().to_string(),
            default_model: self.default_model.trim().to_string(),
            tokenizer_path: (!tokenizer_path.is_empty()).then(|| tokenizer_path.to_string()),
            proxy: self.proxy,
            env_key: (!env_key.is_empty()).then(|| env_key.to_string()),
            api_key: (!api_key.is_empty()).then(|| api_key.to_string()),
        }
    }
}

#[derive(Default)]
pub struct ProviderPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<ProviderForm>,
    selected_provider: Option<String>,
}

impl ProviderPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                notifications.success("Provider config loaded from disk");
            }
            Err(err) => {
                notifications.error(format!("Failed to load config: {err}"));
            }
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

    fn reload(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn set_active_provider(&mut self, provider_id: &str, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        if !self.config.model_providers.contains_key(provider_id) {
            notifications.error(format!("Provider '{provider_id}' not found"));
            return;
        }
        if self.config.model_provider == provider_id {
            notifications.info(format!("Provider '{provider_id}' is already active"));
            return;
        }

        let mut next = self.config.clone();
        next.model_provider = provider_id.to_string();
        match toml::to_string_pretty(&next) {
            Ok(raw) => match store.save_raw_toml(&raw) {
                Ok(snapshot) => {
                    self.apply_snapshot(snapshot);
                    notifications.success(format!("Set active provider to '{provider_id}'"));
                }
                Err(err) => notifications.error(format!("Save failed: {err}")),
            },
            Err(err) => notifications.error(format!("Failed to render config TOML: {err}")),
        }
    }

    fn open_add_provider(&mut self) {
        let mut form = ProviderForm::new();
        form.set_as_active = self.config.model_providers.is_empty();
        self.form = Some(form);
    }

    fn open_edit_provider(&mut self, provider_id: &str) {
        let Some(provider) = self.config.model_providers.get(provider_id) else {
            return;
        };
        self.form = Some(ProviderForm::edit(
            provider_id,
            provider,
            &self.config.model_provider,
        ));
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
            Ok(next_config) => match toml::to_string_pretty(&next_config) {
                Ok(raw) => match store.save_raw_toml(&raw) {
                    Ok(snapshot) => {
                        self.apply_snapshot(snapshot);
                        self.form = None;
                        notifications.success("Provider configuration saved");
                    }
                    Err(err) => notifications.error(format!("Save failed: {err}")),
                },
                Err(err) => notifications.error(format!("Failed to render config TOML: {err}")),
            },
            Err(err) => notifications.error(err),
        }
    }

    fn apply_form(mut config: AppConfig, form: &ProviderForm) -> Result<AppConfig, String> {
        let provider_id = form.normalized_id();
        if provider_id.is_empty() {
            return Err("Provider ID cannot be empty".to_string());
        }

        let provider = form.to_config();
        if provider.base_url.is_empty() {
            return Err("Base URL cannot be empty".to_string());
        }
        if provider.wire_api.is_empty() {
            return Err("Wire API cannot be empty".to_string());
        }
        if provider.default_model.is_empty() {
            return Err("Default model cannot be empty".to_string());
        }

        if let Some(original_id) = form.original_id.as_ref() {
            if original_id != &provider_id {
                if config.model_providers.contains_key(&provider_id) {
                    return Err(format!(
                        "Provider ID '{provider_id}' already exists, choose another ID"
                    ));
                }
                config.model_providers.remove(original_id);
                if config.model_provider == *original_id {
                    config.model_provider = provider_id.clone();
                }
                if config.memory.embedding.provider == *original_id {
                    config.memory.embedding.provider = provider_id.clone();
                }
            }
        } else if config.model_providers.contains_key(&provider_id) {
            return Err(format!(
                "Provider ID '{provider_id}' already exists, choose another ID"
            ));
        }

        config.model_providers.insert(provider_id.clone(), provider);

        if form.set_as_active || config.model_provider == provider_id {
            config.model_provider = provider_id;
        }

        Ok(config)
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new(form.title())
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(460.0);
                ui.label("Provider configuration is persisted to config.toml.");
                ui.separator();
                egui::Grid::new("provider-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Provider ID");
                        ui.text_edit_singleline(&mut form.id);
                        ui.end_row();

                        ui.label("Display Name");
                        ui.text_edit_singleline(&mut form.name);
                        ui.end_row();

                        ui.label("Base URL");
                        ui.text_edit_singleline(&mut form.base_url);
                        ui.end_row();

                        ui.label("Wire API");
                        egui::ComboBox::from_id_salt("wire_api")
                            .selected_text(&form.wire_api)
                            .show_ui(ui, |ui| {
                                for option in OpenAiWireApi::VARIANTS {
                                    ui.selectable_value(
                                        &mut form.wire_api,
                                        option.to_string(),
                                        option,
                                    );
                                }
                            });
                        ui.end_row();

                        ui.label("Default Model");
                        ui.text_edit_singleline(&mut form.default_model);
                        ui.end_row();

                        ui.label("Tokenizer Path");
                        ui.text_edit_singleline(&mut form.tokenizer_path);
                        ui.end_row();

                        ui.label("Use System Proxy");
                        ui.checkbox(&mut form.proxy, "");
                        ui.end_row();

                        ui.label("Env Key");
                        ui.text_edit_singleline(&mut form.env_key);
                        ui.end_row();

                        ui.label("API Key");
                        ui.text_edit_singleline(&mut form.api_key);
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.checkbox(&mut form.set_as_active, "Set as active model provider");

                ui.separator();
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
            self.save_form(notifications);
        }
        if cancel_clicked {
            self.form = None;
        }
    }
}

impl PanelRenderer for ProviderPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.horizontal(|ui| {
            ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
            ui.colored_label(
                egui::Color32::LIGHT_GREEN,
                format!("Active provider: {}", self.config.model_provider),
            );
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Add Provider").clicked() {
                self.open_add_provider();
            }
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });

        ui.add_space(8.0);

        if self.config.model_providers.is_empty() {
            ui.label("No providers configured.");
        } else {
            let mut edit_provider_id: Option<String> = None;
            let mut set_active_provider_id: Option<String> = None;

            let provider_ids = self
                .config
                .model_providers
                .keys()
                .cloned()
                .collect::<Vec<_>>();

            let available_height = ui.available_height();
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(100.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::auto().at_least(180.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::auto().at_least(140.0))
                .column(Column::auto().at_least(100.0))
                .column(Column::remainder().at_least(80.0))
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height)
                .sense(egui::Sense::click())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("ID");
                    });
                    header.col(|ui| {
                        ui.strong("Name");
                    });
                    header.col(|ui| {
                        ui.strong("Base URL");
                    });
                    header.col(|ui| {
                        ui.strong("Wire API");
                    });
                    header.col(|ui| {
                        ui.strong("Default Model");
                    });
                    header.col(|ui| {
                        ui.strong("Tokenizer");
                    });
                    header.col(|ui| {
                        ui.strong("Auth");
                    });
                })
                .body(|body| {
                    body.rows(20.0, provider_ids.len(), |mut row| {
                        let idx = row.index();
                        let provider_id = &provider_ids[idx];
                        let Some(provider) = self.config.model_providers.get(provider_id) else {
                            return;
                        };

                        let is_selected = self.selected_provider.as_deref() == Some(provider_id);
                        row.set_selected(is_selected);

                        let mut id_label = provider_id.clone();
                        if provider_id == &self.config.model_provider {
                            id_label.push_str(" (active)");
                        }

                        let auth = if provider.api_key.as_deref().is_some_and(|v| !v.is_empty()) {
                            "api_key".to_string()
                        } else {
                            provider
                                .env_key
                                .as_deref()
                                .filter(|v| !v.is_empty())
                                .map(|v| format!("env:{v}"))
                                .unwrap_or_else(|| "none".to_string())
                        };

                        row.col(|ui| {
                            ui.label(&id_label);
                        });
                        row.col(|ui| {
                            ui.label(provider.name.as_deref().unwrap_or("-"));
                        });
                        row.col(|ui| {
                            ui.label(&provider.base_url);
                        });
                        row.col(|ui| {
                            ui.label(&provider.wire_api);
                        });
                        row.col(|ui| {
                            ui.label(&provider.default_model);
                        });
                        row.col(|ui| {
                            ui.label(provider.tokenizer_path.as_deref().unwrap_or("-"));
                        });
                        row.col(|ui| {
                            ui.label(auth);
                        });

                        let response = row.response();

                        if response.clicked() {
                            self.selected_provider = if is_selected {
                                None
                            } else {
                                Some(provider_id.clone())
                            };
                        }

                        let provider_id_clone = provider_id.clone();
                        let is_current_active = provider_id == &self.config.model_provider;
                        response.context_menu(|ui| {
                            if ui.button("Edit").clicked() {
                                edit_provider_id = Some(provider_id_clone.clone());
                                ui.close();
                            }
                            if ui
                                .add_enabled(!is_current_active, egui::Button::new("Set Active"))
                                .clicked()
                            {
                                set_active_provider_id = Some(provider_id_clone);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Copy ID").clicked() {
                                ui.ctx().output_mut(|o| {
                                    o.commands
                                        .push(egui::OutputCommand::CopyText(provider_id.clone()));
                                });
                                ui.close();
                            }
                        });
                    });
                });

            if let Some(id) = edit_provider_id {
                self.open_edit_provider(&id);
            }
            if let Some(id) = set_active_provider_id {
                self.set_active_provider(&id, notifications);
            }
        }

        self.render_form_window(ui, notifications);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_form_add_provider_sets_active_when_requested() {
        let config = AppConfig::default();
        let mut form = ProviderForm::new();
        form.id = "anthropic".to_string();
        form.name = "Anthropic".to_string();
        form.base_url = "https://api.anthropic.com/v1".to_string();
        form.wire_api = "responses".to_string();
        form.default_model = "claude-3-7-sonnet-latest".to_string();
        form.env_key = "ANTHROPIC_API_KEY".to_string();
        form.set_as_active = true;

        let updated = ProviderPanel::apply_form(config, &form).expect("form should apply");

        assert_eq!(updated.model_provider, "anthropic");
        assert!(updated.model_providers.contains_key("anthropic"));
        assert_eq!(
            updated.model_providers["anthropic"].env_key.as_deref(),
            Some("ANTHROPIC_API_KEY")
        );
    }

    #[test]
    fn apply_form_rename_provider_updates_references() {
        let mut config = AppConfig::default();
        config.memory.embedding.provider = "openai".to_string();

        let source = config
            .model_providers
            .get("openai")
            .expect("default provider should exist")
            .clone();
        let mut form = ProviderForm::edit("openai", &source, &config.model_provider);
        form.id = "openai-main".to_string();

        let updated = ProviderPanel::apply_form(config, &form).expect("form should apply");

        assert_eq!(updated.model_provider, "openai-main");
        assert_eq!(updated.memory.embedding.provider, "openai-main");
        assert!(updated.model_providers.contains_key("openai-main"));
        assert!(!updated.model_providers.contains_key("openai"));
    }

    #[test]
    fn apply_form_rejects_duplicate_provider_id() {
        let config = AppConfig::default();
        let mut form = ProviderForm::new();
        form.id = "openai".to_string();

        let err = ProviderPanel::apply_form(config, &form).expect_err("duplicate id should fail");

        assert!(err.contains("already exists"));
    }
}
