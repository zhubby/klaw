use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::{
    ProviderRuntimeSnapshot, RuntimeRequestHandle, begin_provider_status_request,
    begin_sync_providers_request,
};
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigError, ConfigSnapshot, ConfigStore, ModelProviderConfig};
use klaw_llm::OpenAiWireApi;
use std::time::{Duration, Instant};

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
    stream: bool,
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
            stream: default.stream,
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
            stream: provider.stream,
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
            stream: self.stream,
            env_key: (!env_key.is_empty()).then(|| env_key.to_string()),
            api_key: (!api_key.is_empty()).then(|| api_key.to_string()),
        }
    }
}

#[derive(Default)]
pub struct ProviderPanel {
    store: Option<ConfigStore>,
    config: AppConfig,
    runtime_status: Option<ProviderRuntimeSnapshot>,
    last_runtime_status_at: Option<Instant>,
    runtime_status_request: Option<RuntimeRequestHandle<ProviderRuntimeSnapshot>>,
    sync_request: Option<RuntimeRequestHandle<ProviderRuntimeSnapshot>>,
    sync_success_message: Option<String>,
    sync_failure_message: Option<String>,
    form: Option<ProviderForm>,
    selected_provider: Option<String>,
    delete_confirm: Option<String>,
}

impl ProviderPanel {
    fn refresh_runtime_status(&mut self) {
        if let Some(request) = self.runtime_status_request.as_mut()
            && let Some(result) = request.try_take_result()
        {
            self.runtime_status_request = None;
            if let Ok(snapshot) = result {
                self.runtime_status = Some(snapshot);
            }
        }
        let should_refresh = self
            .last_runtime_status_at
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(2));
        if !should_refresh || self.runtime_status_request.is_some() {
            return;
        }
        self.last_runtime_status_at = Some(Instant::now());
        self.runtime_status_request = Some(begin_provider_status_request());
    }

    fn begin_runtime_sync(
        &mut self,
        success_message: impl Into<String>,
        failure_message: impl Into<String>,
    ) {
        self.sync_request = Some(begin_sync_providers_request());
        self.sync_success_message = Some(success_message.into());
        self.sync_failure_message = Some(failure_message.into());
    }

    fn poll_runtime_sync(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.sync_request.as_mut() else {
            return;
        };
        let Some(result) = request.try_take_result() else {
            return;
        };

        self.sync_request = None;
        match result {
            Ok(snapshot) => {
                self.runtime_status = Some(snapshot);
                if let Some(message) = self.sync_success_message.take() {
                    notifications.success(message);
                }
                self.sync_failure_message = None;
            }
            Err(err) => {
                let prefix = self
                    .sync_failure_message
                    .take()
                    .unwrap_or_else(|| "Failed to sync running runtime".to_string());
                notifications.error(format!("{prefix}: {err}"));
                self.sync_success_message = None;
            }
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
                notifications.success("Provider config loaded from disk");
            }
            Err(err) => {
                notifications.error(format!("Failed to load config: {err}"));
            }
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
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

        let provider_id = provider_id.to_string();
        match store.update_config(|config| {
            config.model_provider = provider_id.clone();
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.begin_runtime_sync(
                    format!(
                        "Set active provider to '{}' and synced running runtime",
                        provider_id
                    ),
                    format!(
                        "Saved active provider to '{}', but failed to sync running runtime",
                        provider_id
                    ),
                );
            }
            Err(err) => notifications.error(format!("Save failed: {err}")),
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
        let Some(form) = self.form.clone() else {
            return;
        };

        match store.update_config(|config| {
            let next_config =
                Self::apply_form(config.clone(), &form).map_err(ConfigError::InvalidConfig)?;
            *config = next_config;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.form = None;
                self.begin_runtime_sync(
                    "Provider configuration saved and runtime synced",
                    "Provider configuration saved, but failed to sync running runtime",
                );
            }
            Err(err) => notifications.error(format!("Save failed: {err}")),
        }
    }

    fn delete_provider(&mut self, provider_id: &str, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };

        let provider_id = provider_id.to_string();
        match store.update_config(|config| {
            let next_config = Self::remove_provider(config.clone(), &provider_id)
                .map_err(ConfigError::InvalidConfig)?;
            *config = next_config;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                if self.selected_provider.as_deref() == Some(provider_id.as_str()) {
                    self.selected_provider = None;
                }
                self.begin_runtime_sync(
                    format!("Deleted provider '{}' and synced runtime", provider_id),
                    format!(
                        "Deleted provider '{}', but failed to sync running runtime",
                        provider_id
                    ),
                );
            }
            Err(err) => notifications.error(format!("Save failed: {err}")),
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

    fn remove_provider(mut config: AppConfig, provider_id: &str) -> Result<AppConfig, String> {
        if !config.model_providers.contains_key(provider_id) {
            return Err(format!("Provider '{provider_id}' not found"));
        }
        if config.model_provider == provider_id {
            return Err(format!(
                "Cannot delete active provider '{provider_id}'. Set another provider active first."
            ));
        }
        if config.memory.embedding.enabled && config.memory.embedding.provider == provider_id {
            return Err(format!(
                "Cannot delete provider '{provider_id}' because memory embedding uses it."
            ));
        }

        config.model_providers.remove(provider_id);
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

                        ui.label("Enable Streaming");
                        ui.checkbox(&mut form.stream, "");
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

    fn render_delete_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(provider_id) = self.delete_confirm.clone() else {
            return;
        };

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Delete Provider")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "Are you sure you want to delete provider '{provider_id}'?"
                    ))
                    .strong(),
                );
                ui.add_space(8.0);
                ui.label(
                    "This removes the provider from config.toml. Active or in-use providers cannot be deleted.",
                );
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
            self.delete_provider(&provider_id, notifications);
            self.delete_confirm = None;
        }
        if cancelled {
            self.delete_confirm = None;
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
        self.refresh_runtime_status();
        self.poll_runtime_sync(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            ui.colored_label(
                egui::Color32::LIGHT_GREEN,
                format!("Config default: {}", self.config.model_provider),
            );
            if let Some(runtime_status) = &self.runtime_status {
                ui.separator();
                ui.colored_label(
                    egui::Color32::from_rgb(0x60, 0xA5, 0xFA),
                    format!("Runtime active: {}", runtime_status.active_provider_id),
                );
            }
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
            let mut delete_provider_id: Option<String> = None;

            let provider_ids = self
                .config
                .model_providers
                .keys()
                .cloned()
                .collect::<Vec<_>>();

            let available_height = ui.available_height();
            let table_width = ui.available_width();
            egui::ScrollArea::both()
                .id_salt("provider-table-scroll")
                .auto_shrink([false, false])
                .max_width(table_width)
                .max_height(available_height)
                .show(ui, |ui| {
                    ui.set_min_width(table_width);
                    TableBuilder::new(ui)
                        .striped(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto().at_least(100.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(180.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(140.0))
                        .column(Column::auto().at_least(70.0))
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
                                ui.strong("Stream");
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
                                let Some(provider) = self.config.model_providers.get(provider_id)
                                else {
                                    return;
                                };

                                let is_selected =
                                    self.selected_provider.as_deref() == Some(provider_id);
                                row.set_selected(is_selected);

                                let is_config_default = provider_id == &self.config.model_provider;
                                let is_runtime_active =
                                    self.runtime_status.as_ref().is_some_and(|status| {
                                        provider_id == &status.active_provider_id
                                    });
                                let auth =
                                    if provider.api_key.as_deref().is_some_and(|v| !v.is_empty()) {
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
                                    ui.horizontal(|ui| {
                                        ui.label(provider_id);
                                        if is_config_default {
                                            ui.label(
                                                RichText::new(regular::CHECK_CIRCLE).color(
                                                    egui::Color32::from_rgb(0x22, 0xC5, 0x5E),
                                                ),
                                            );
                                            ui.label(
                                                RichText::new("config").small().color(
                                                    egui::Color32::from_rgb(0x22, 0xC5, 0x5E),
                                                ),
                                            );
                                        }
                                        if is_runtime_active {
                                            ui.label(
                                                RichText::new(regular::PLAY_CIRCLE).color(
                                                    egui::Color32::from_rgb(0x60, 0xA5, 0xFA),
                                                ),
                                            );
                                            ui.label(
                                                RichText::new("runtime").small().color(
                                                    egui::Color32::from_rgb(0x60, 0xA5, 0xFA),
                                                ),
                                            );
                                        }
                                    });
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
                                    ui.label(if provider.stream { "yes" } else { "no" });
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
                                response.context_menu(|ui| {
                                    if ui
                                        .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                        .clicked()
                                    {
                                        edit_provider_id = Some(provider_id_clone.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled(
                                            !is_config_default,
                                            egui::Button::new(format!(
                                                "{} Set Config Default",
                                                regular::CHECK_CIRCLE
                                            )),
                                        )
                                        .clicked()
                                    {
                                        set_active_provider_id = Some(provider_id_clone.clone());
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
                                        delete_provider_id = Some(provider_id_clone.clone());
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button(format!("{} Copy ID", regular::COPY)).clicked() {
                                        ui.ctx().output_mut(|o| {
                                            o.commands.push(egui::OutputCommand::CopyText(
                                                provider_id.clone(),
                                            ));
                                        });
                                        ui.close();
                                    }
                                });
                            });
                        });
                });

            if let Some(id) = edit_provider_id {
                self.open_edit_provider(&id);
            }
            if let Some(id) = set_active_provider_id {
                self.set_active_provider(&id, notifications);
            }
            if let Some(id) = delete_provider_id {
                self.delete_confirm = Some(id);
            }
        }

        self.render_form_window(ui, notifications);
        self.render_delete_confirm_dialog(ui.ctx(), notifications);
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
    fn apply_form_preserves_stream_flag() {
        let config = AppConfig::default();
        let mut form = ProviderForm::new();
        form.id = "openai-stream".to_string();
        form.base_url = "https://api.openai.com/v1".to_string();
        form.wire_api = "responses".to_string();
        form.default_model = "gpt-4.1-mini".to_string();
        form.stream = true;

        let updated = ProviderPanel::apply_form(config, &form).expect("form should apply");

        assert!(updated.model_providers["openai-stream"].stream);
    }

    #[test]
    fn apply_form_rejects_duplicate_provider_id() {
        let config = AppConfig::default();
        let mut form = ProviderForm::new();
        form.id = "openai".to_string();

        let err = ProviderPanel::apply_form(config, &form).expect_err("duplicate id should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn remove_provider_deletes_non_active_provider() {
        let mut config = AppConfig::default();
        config.model_providers.insert(
            "anthropic".to_string(),
            ModelProviderConfig {
                base_url: "https://api.anthropic.com/v1".to_string(),
                wire_api: "responses".to_string(),
                default_model: "claude-sonnet-4".to_string(),
                ..ModelProviderConfig::default()
            },
        );

        let updated =
            ProviderPanel::remove_provider(config, "anthropic").expect("delete should succeed");

        assert!(!updated.model_providers.contains_key("anthropic"));
        assert!(updated.model_providers.contains_key("openai"));
    }

    #[test]
    fn remove_provider_rejects_active_provider() {
        let config = AppConfig::default();

        let err = ProviderPanel::remove_provider(config, "openai")
            .expect_err("active delete should fail");

        assert!(err.contains("Cannot delete active provider"));
    }

    #[test]
    fn remove_provider_rejects_memory_embedding_provider() {
        let mut config = AppConfig::default();
        config.model_providers.insert(
            "anthropic".to_string(),
            ModelProviderConfig {
                base_url: "https://api.anthropic.com/v1".to_string(),
                wire_api: "responses".to_string(),
                default_model: "claude-sonnet-4".to_string(),
                ..ModelProviderConfig::default()
            },
        );
        config.memory.embedding.enabled = true;
        config.memory.embedding.provider = "anthropic".to_string();

        let err = ProviderPanel::remove_provider(config, "anthropic")
            .expect_err("embedding provider delete should fail");

        assert!(err.contains("memory embedding uses it"));
    }
}
