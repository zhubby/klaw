use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::{
    RuntimeRequestHandle, begin_knowledge_entry_request, begin_knowledge_status_request,
    begin_search_knowledge_request, begin_sync_knowledge_index_request,
};
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, KnowledgeConfig, KnowledgeModelsConfig,
    KnowledgeRetrievalConfig, ObsidianKnowledgeConfig,
};
use klaw_knowledge::{KnowledgeEntry, KnowledgeHit, KnowledgeStatus, KnowledgeSyncResult};
use klaw_model::{ModelCapability, ModelService, ModelSummary};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone)]
struct KnowledgeConfigForm {
    enabled: bool,
    provider: String,
    vault_path: String,
    index_on_startup: bool,
    max_excerpt_length: String,
    exclude_folders: String,
    top_k: String,
    rerank_candidates: String,
    graph_hops: String,
    temporal_decay: String,
    embedding_model_id: String,
    orchestrator_model_id: String,
    reranker_model_id: String,
}

impl KnowledgeConfigForm {
    fn from_config(config: &KnowledgeConfig) -> Self {
        Self {
            enabled: config.enabled,
            provider: config.provider.clone(),
            vault_path: config.obsidian.vault_path.clone().unwrap_or_default(),
            index_on_startup: config.obsidian.index_on_startup,
            max_excerpt_length: config.obsidian.max_excerpt_length.to_string(),
            exclude_folders: config.obsidian.exclude_folders.join(", "),
            top_k: config.retrieval.top_k.to_string(),
            rerank_candidates: config.retrieval.rerank_candidates.to_string(),
            graph_hops: config.retrieval.graph_hops.to_string(),
            temporal_decay: config.retrieval.temporal_decay.to_string(),
            embedding_model_id: config.models.embedding_model_id.clone().unwrap_or_default(),
            orchestrator_model_id: config
                .models
                .orchestrator_model_id
                .clone()
                .unwrap_or_default(),
            reranker_model_id: config.models.reranker_model_id.clone().unwrap_or_default(),
        }
    }

    fn to_config(&self) -> Result<KnowledgeConfig, String> {
        let provider = self.provider.trim();
        if provider != "obsidian" {
            return Err("knowledge.provider must be obsidian".to_string());
        }
        let vault_path = self.vault_path.trim();
        if self.enabled && vault_path.is_empty() {
            return Err("knowledge.obsidian.vault_path is required when enabled".to_string());
        }
        let max_excerpt_length = parse_usize(&self.max_excerpt_length, "max_excerpt_length")?;
        let top_k = parse_usize(&self.top_k, "top_k")?;
        let rerank_candidates = parse_usize(&self.rerank_candidates, "rerank_candidates")?;
        let graph_hops = self
            .graph_hops
            .trim()
            .parse::<usize>()
            .map_err(|_| "graph_hops must be a non-negative integer".to_string())?;
        let temporal_decay = self
            .temporal_decay
            .trim()
            .parse::<f32>()
            .map_err(|_| "temporal_decay must be a number".to_string())?;

        Ok(KnowledgeConfig {
            enabled: self.enabled,
            provider: provider.to_string(),
            obsidian: ObsidianKnowledgeConfig {
                vault_path: (!vault_path.is_empty()).then(|| vault_path.to_string()),
                index_on_startup: self.index_on_startup,
                max_excerpt_length,
                exclude_folders: split_csv(&self.exclude_folders),
            },
            retrieval: KnowledgeRetrievalConfig {
                top_k,
                rerank_candidates,
                graph_hops,
                temporal_decay,
            },
            models: KnowledgeModelsConfig {
                embedding_provider: "local".to_string(),
                embedding_model_id: optional_string(&self.embedding_model_id),
                orchestrator_model_id: optional_string(&self.orchestrator_model_id),
                reranker_model_id: optional_string(&self.reranker_model_id),
            },
        })
    }
}

pub struct KnowledgePanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    config: AppConfig,
    form: Option<KnowledgeConfigForm>,
    status: Option<KnowledgeStatus>,
    status_request: Option<RuntimeRequestHandle<KnowledgeStatus>>,
    sync_request: Option<RuntimeRequestHandle<KnowledgeSyncResult>>,
    search_query: String,
    search_limit: String,
    search_request: Option<RuntimeRequestHandle<Vec<KnowledgeHit>>>,
    hits: Vec<KnowledgeHit>,
    selected_id: Option<String>,
    entry_request: Option<RuntimeRequestHandle<Option<KnowledgeEntry>>>,
    selected_entry: Option<KnowledgeEntry>,
    model_options: Vec<ModelSummary>,
    model_options_request: Option<Receiver<Result<Vec<ModelSummary>, String>>>,
}

impl Default for KnowledgePanel {
    fn default() -> Self {
        Self {
            store: None,
            config_path: None,
            config: AppConfig::default(),
            form: None,
            status: None,
            status_request: None,
            sync_request: None,
            search_query: String::new(),
            search_limit: "5".to_string(),
            search_request: None,
            hits: Vec::new(),
            selected_id: None,
            entry_request: None,
            selected_entry: None,
            model_options: Vec::new(),
            model_options_request: None,
        }
    }
}

impl KnowledgePanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                self.refresh_status();
                self.refresh_model_options();
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.search_limit = snapshot.config.knowledge.retrieval.top_k.max(1).to_string();
        self.config = snapshot.config;
    }

    fn refresh_status(&mut self) {
        if self.status_request.is_none() {
            self.status_request = Some(begin_knowledge_status_request());
        }
    }

    fn refresh_model_options(&mut self) {
        if self.model_options_request.is_some() {
            return;
        }
        let config = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.model_options_request = Some(rx);
        thread::spawn(move || {
            let result = ModelService::open_default(&config)
                .and_then(|service| service.list_installed())
                .map_err(|err| err.to_string());
            let _ = tx.send(result);
        });
    }

    fn begin_sync(&mut self, notifications: &mut NotificationCenter) {
        if self.sync_request.is_some() {
            return;
        }
        self.sync_request = Some(begin_sync_knowledge_index_request());
        notifications.info("Syncing knowledge index and vectors...");
    }

    fn begin_search(&mut self, notifications: &mut NotificationCenter) {
        if self.search_request.is_some() {
            return;
        }
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            notifications.error("Knowledge search requires a query");
            return;
        }
        let limit = match parse_usize(&self.search_limit, "limit") {
            Ok(limit) => limit,
            Err(err) => {
                notifications.error(err);
                return;
            }
        };
        self.search_request = Some(begin_search_knowledge_request(query, limit));
    }

    fn begin_entry_load(&mut self, id: String) {
        if self.selected_id.as_deref() == Some(id.as_str()) && self.entry_request.is_some() {
            return;
        }
        self.selected_id = Some(id.clone());
        self.selected_entry = None;
        self.entry_request = Some(begin_knowledge_entry_request(id));
    }

    fn poll_requests(&mut self, notifications: &mut NotificationCenter) {
        if let Some(mut request) = self.status_request.take() {
            match request.try_take_result() {
                Some(Ok(status)) => self.status = Some(status),
                Some(Err(err)) => notifications.error(format!("Knowledge status failed: {err}")),
                None => self.status_request = Some(request),
            }
        }
        if let Some(mut request) = self.sync_request.take() {
            match request.try_take_result() {
                Some(Ok(result)) => {
                    self.status = Some(result.status);
                    notifications.success(format!(
                        "Knowledge sync complete: {} notes indexed, {} chunks embedded",
                        result.indexed_notes, result.embedded_chunks
                    ));
                }
                Some(Err(err)) => notifications.error(format!("Knowledge sync failed: {err}")),
                None => self.sync_request = Some(request),
            }
        }
        if let Some(mut request) = self.search_request.take() {
            match request.try_take_result() {
                Some(Ok(hits)) => {
                    self.hits = hits;
                    if let Some(first) = self.hits.first() {
                        self.begin_entry_load(first.id.clone());
                    } else {
                        self.selected_id = None;
                        self.selected_entry = None;
                    }
                }
                Some(Err(err)) => notifications.error(format!("Knowledge search failed: {err}")),
                None => self.search_request = Some(request),
            }
        }
        if let Some(mut request) = self.entry_request.take() {
            match request.try_take_result() {
                Some(Ok(entry)) => self.selected_entry = entry,
                Some(Err(err)) => notifications.error(format!("Knowledge entry failed: {err}")),
                None => self.entry_request = Some(request),
            }
        }
        if let Some(receiver) = self.model_options_request.take() {
            match receiver.try_recv() {
                Ok(Ok(mut options)) => {
                    options.sort_by(|left, right| left.model_id.cmp(&right.model_id));
                    self.model_options = options;
                }
                Ok(Err(err)) => notifications.error(format!("Model list failed: {err}")),
                Err(mpsc::TryRecvError::Empty) => self.model_options_request = Some(receiver),
                Err(mpsc::TryRecvError::Disconnected) => {
                    notifications.error("Model list worker closed unexpectedly");
                }
            }
        }
    }

    fn has_pending_request(&self) -> bool {
        self.status_request.is_some()
            || self.sync_request.is_some()
            || self.search_request.is_some()
            || self.entry_request.is_some()
            || self.model_options_request.is_some()
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
            config.knowledge = form.to_config().map_err(ConfigError::InvalidConfig)?;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.form = None;
                self.refresh_status();
                self.refresh_model_options();
                notifications.success("Knowledge config saved");
            }
            Err(err) => notifications.error(format!("Save failed: {err}")),
        }
    }

    fn render_status(&self, ui: &mut egui::Ui) {
        let status = self.status.as_ref();
        ui.horizontal_wrapped(|ui| {
            status_chip(
                ui,
                "State",
                status
                    .map(|status| {
                        if status.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    })
                    .unwrap_or("unknown")
                    .to_string(),
            );
            status_chip(
                ui,
                "Provider",
                status
                    .map(|status| status.provider.clone())
                    .unwrap_or_else(|| self.config.knowledge.provider.clone()),
            );
            status_chip(
                ui,
                "Entries",
                status
                    .map(|status| status.entry_count.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            status_chip(
                ui,
                "Chunks",
                status
                    .map(|status| status.chunk_count.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            status_chip(
                ui,
                "Vectors",
                status
                    .map(|status| format!("{}/{}", status.embedded_chunk_count, status.chunk_count))
                    .unwrap_or_else(|| "-".to_string()),
            );
        });
        ui.add_space(4.0);
        let vault = status
            .and_then(|status| status.vault_path.as_deref())
            .or(self.config.knowledge.obsidian.vault_path.as_deref())
            .unwrap_or("(not configured)");
        ui.small(format!("Vault: {vault}"));
    }

    fn render_search(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        ui.horizontal(|ui| {
            ui.label("Query");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .desired_width(360.0)
                    .hint_text("Search notes"),
            );
            ui.label("Limit");
            ui.add(egui::TextEdit::singleline(&mut self.search_limit).desired_width(60.0));
            let enter =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if ui
                .add_enabled(self.search_request.is_none(), egui::Button::new("Search"))
                .clicked()
                || enter
            {
                self.begin_search(notifications);
            }
        });
        ui.add_space(8.0);

        ui.columns(2, |columns| {
            self.render_hits(&mut columns[0]);
            self.render_entry(&mut columns[1]);
        });
    }

    fn render_hits(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!("Results ({})", self.hits.len()));
        ui.add_space(4.0);
        let mut selected = None;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 8.0)
            .show(ui, |ui| {
                let row_height = ui.spacing().interact_size.y;
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::remainder().at_least(160.0))
                    .column(Column::auto().at_least(52.0))
                    .header(row_height, |mut header| {
                        header.col(|ui| {
                            ui.strong("Title");
                        });
                        header.col(|ui| {
                            ui.strong("Score");
                        });
                    })
                    .body(|mut body| {
                        for hit in &self.hits {
                            let is_selected = self.selected_id.as_deref() == Some(hit.id.as_str());
                            body.row(row_height, |mut row| {
                                row.col(|ui| {
                                    let label = if hit.title.trim().is_empty() {
                                        hit.id.as_str()
                                    } else {
                                        hit.title.as_str()
                                    };
                                    if ui.selectable_label(is_selected, label).clicked() {
                                        selected = Some(hit.id.clone());
                                    }
                                });
                                row.col(|ui| {
                                    ui.monospace(format!("{:.3}", hit.score));
                                });
                            });
                        }
                    });
            });
        if let Some(id) = selected {
            self.begin_entry_load(id);
        }
    }

    fn render_entry(&self, ui: &mut egui::Ui) {
        ui.strong("Preview");
        ui.add_space(4.0);
        if self.entry_request.is_some() {
            ui.add(egui::Spinner::new());
            return;
        }
        let Some(selected_id) = self.selected_id.as_deref() else {
            ui.label("Select a result to inspect it.");
            return;
        };
        let Some(entry) = self.selected_entry.as_ref() else {
            ui.label(format!("No entry loaded for {selected_id}."));
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.monospace(&entry.id);
            if !entry.tags.is_empty() {
                ui.label(format!("tags: {}", entry.tags.join(", ")));
            }
        });
        ui.small(format!("URI: {}", entry.uri));
        ui.add_space(6.0);
        let mut content = entry.content.clone();
        ui.add(
            egui::TextEdit::multiline(&mut content)
                .desired_width(f32::INFINITY)
                .desired_rows(18)
                .interactive(false),
        );
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        let model_options = self.model_options.clone();
        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new("Knowledge Config")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ui.ctx(), |ui| {
                ui.small(status_label(self.config_path.as_deref()));
                ui.separator();
                egui::Grid::new("knowledge-config-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Enabled");
                        ui.checkbox(&mut form.enabled, "");
                        ui.end_row();

                        ui.label("Provider");
                        ui.text_edit_singleline(&mut form.provider);
                        ui.end_row();

                        ui.label("Vault path");
                        ui.text_edit_singleline(&mut form.vault_path);
                        ui.end_row();

                        ui.label("Index on startup");
                        ui.checkbox(&mut form.index_on_startup, "");
                        ui.end_row();

                        ui.label("Max excerpt length");
                        ui.text_edit_singleline(&mut form.max_excerpt_length);
                        ui.end_row();

                        ui.label("Exclude folders");
                        ui.text_edit_singleline(&mut form.exclude_folders);
                        ui.end_row();

                        ui.label("Top K");
                        ui.text_edit_singleline(&mut form.top_k);
                        ui.end_row();

                        ui.label("Rerank candidates");
                        ui.text_edit_singleline(&mut form.rerank_candidates);
                        ui.end_row();

                        ui.label("Graph hops");
                        ui.text_edit_singleline(&mut form.graph_hops);
                        ui.end_row();

                        ui.label("Temporal decay");
                        ui.text_edit_singleline(&mut form.temporal_decay);
                        ui.end_row();

                        ui.label("Embedding model id");
                        model_combo(
                            ui,
                            "knowledge-config-embedding-model",
                            &mut form.embedding_model_id,
                            &model_options,
                            ModelCapability::Embedding,
                        );
                        ui.end_row();

                        ui.label("Orchestrator model id");
                        model_combo(
                            ui,
                            "knowledge-config-orchestrator-model",
                            &mut form.orchestrator_model_id,
                            &model_options,
                            ModelCapability::Orchestrator,
                        );
                        ui.end_row();

                        ui.label("Reranker model id");
                        model_combo(
                            ui,
                            "knowledge-config-reranker-model",
                            &mut form.reranker_model_id,
                            &model_options,
                            ModelCapability::Rerank,
                        );
                        ui.end_row();
                    });
                if self.model_options_request.is_some() {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.small("Loading installed models...");
                    });
                } else if model_options.is_empty() {
                    ui.small("No installed local models were found.");
                }
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

impl PanelRenderer for KnowledgePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        self.poll_requests(notifications);
        if self.has_pending_request() {
            ui.ctx().request_repaint_after(POLL_INTERVAL);
        }

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Refresh", regular::ARROW_CLOCKWISE))
                .clicked()
            {
                self.refresh_status();
            }
            if ui
                .add_enabled(
                    self.sync_request.is_none(),
                    egui::Button::new(format!(
                        "{} Sync Index & Vectors",
                        regular::ARROWS_CLOCKWISE
                    )),
                )
                .clicked()
            {
                self.begin_sync(notifications);
            }
            if ui.button(format!("{} Config", regular::GEAR)).clicked() {
                self.form = Some(KnowledgeConfigForm::from_config(&self.config.knowledge));
            }
            if self.has_pending_request() {
                ui.add(egui::Spinner::new());
            }
        });
        ui.separator();

        self.render_status(ui);
        ui.separator();
        self.render_search(ui, notifications);
        self.render_form_window(ui, notifications);
    }
}

fn parse_usize(value: &str, label: &str) -> Result<usize, String> {
    match value.trim().parse::<usize>() {
        Ok(value) if value > 0 => Ok(value),
        _ => Err(format!("{label} must be a positive integer")),
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn status_label(path: Option<&Path>) -> String {
    match path {
        Some(path) => format!("Path: {}", path.display()),
        None => "Path: (not loaded)".to_string(),
    }
}

fn status_chip(ui: &mut egui::Ui, label: &str, value: String) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.small(RichText::new(label).color(Color32::GRAY));
                ui.monospace(value);
            });
        });
}

fn model_combo(
    ui: &mut egui::Ui,
    id: &'static str,
    selected: &mut String,
    models: &[ModelSummary],
    preferred_capability: ModelCapability,
) {
    let selected_text = if selected.trim().is_empty() {
        "Not configured".to_string()
    } else {
        selected.clone()
    };
    let mut ordered = models.to_vec();
    ordered.sort_by(|left, right| {
        let left_rank = model_capability_rank(left, preferred_capability);
        let right_rank = model_capability_rank(right, preferred_capability);
        left_rank
            .cmp(&right_rank)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });

    egui::ComboBox::from_id_salt(id)
        .selected_text(selected_text)
        .width(360.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(selected.trim().is_empty(), "Not configured")
                .clicked()
            {
                selected.clear();
                ui.close();
            }
            let current_is_installed = selected.trim().is_empty()
                || models.iter().any(|model| model.model_id == selected.trim());
            if !current_is_installed {
                let current = selected.trim().to_string();
                if ui
                    .selectable_label(true, format!("{current} (not installed)"))
                    .clicked()
                {
                    ui.close();
                }
            }
            for model in ordered {
                let is_selected = selected.trim() == model.model_id;
                let label = model_option_label(&model, preferred_capability);
                if ui.selectable_label(is_selected, label).clicked() {
                    *selected = model.model_id;
                    ui.close();
                }
            }
        });
}

fn model_capability_rank(model: &ModelSummary, preferred_capability: ModelCapability) -> u8 {
    if model.capabilities.contains(&preferred_capability) {
        0
    } else if model.capabilities.is_empty() {
        1
    } else {
        2
    }
}

fn model_option_label(model: &ModelSummary, preferred_capability: ModelCapability) -> String {
    if model.capabilities.contains(&preferred_capability) {
        return format!(
            "{} ({})",
            model.model_id,
            capability_label(preferred_capability)
        );
    }
    if model.capabilities.is_empty() {
        return format!("{} (capability unknown)", model.model_id);
    }
    let capabilities = model
        .capabilities
        .iter()
        .map(|capability| capability_label(*capability))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} ({capabilities})", model.model_id)
}

fn capability_label(capability: ModelCapability) -> &'static str {
    match capability {
        ModelCapability::Embedding => "embedding",
        ModelCapability::Rerank => "rerank",
        ModelCapability::Chat => "chat",
        ModelCapability::Orchestrator => "orchestrator",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_form_does_not_touch_tool_config() {
        let mut config = AppConfig::default();
        config.tools.knowledge.enabled = true;
        config.tools.knowledge.search_limit = 9;
        let before = config.tools.knowledge.clone();
        let form = KnowledgeConfigForm {
            enabled: true,
            provider: "obsidian".to_string(),
            vault_path: "/tmp/vault".to_string(),
            index_on_startup: false,
            max_excerpt_length: "500".to_string(),
            exclude_folders: ".obsidian, templates".to_string(),
            top_k: "7".to_string(),
            rerank_candidates: "21".to_string(),
            graph_hops: "1".to_string(),
            temporal_decay: "0.8".to_string(),
            embedding_model_id: "embed".to_string(),
            orchestrator_model_id: String::new(),
            reranker_model_id: "rerank".to_string(),
        };

        config.knowledge = form.to_config().expect("form should be valid");

        assert_eq!(config.tools.knowledge.enabled, before.enabled);
        assert_eq!(config.tools.knowledge.search_limit, before.search_limit);
        assert_eq!(
            config.knowledge.obsidian.vault_path.as_deref(),
            Some("/tmp/vault")
        );
        assert_eq!(
            config.knowledge.obsidian.exclude_folders,
            vec![".obsidian".to_string(), "templates".to_string()]
        );
    }

    #[test]
    fn model_capability_rank_prefers_matching_models_then_unknown() {
        let matching = ModelSummary {
            model_id: "a".to_string(),
            repo_id: "repo/a".to_string(),
            revision: "main".to_string(),
            capabilities: vec![ModelCapability::Embedding],
            size_bytes: 1,
            installed_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let unknown = ModelSummary {
            model_id: "b".to_string(),
            repo_id: "repo/b".to_string(),
            revision: "main".to_string(),
            capabilities: vec![],
            size_bytes: 1,
            installed_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let other = ModelSummary {
            model_id: "c".to_string(),
            repo_id: "repo/c".to_string(),
            revision: "main".to_string(),
            capabilities: vec![ModelCapability::Rerank],
            size_bytes: 1,
            installed_at: "2026-04-25T00:00:00Z".to_string(),
        };

        assert_eq!(
            model_capability_rank(&matching, ModelCapability::Embedding),
            0
        );
        assert_eq!(
            model_capability_rank(&unknown, ModelCapability::Embedding),
            1
        );
        assert_eq!(model_capability_rank(&other, ModelCapability::Embedding), 2);
    }
}
