use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::{
    KnowledgeSyncRequestHandle, KnowledgeSyncTaskMessage, RuntimeRequestHandle,
    begin_knowledge_entry_request, begin_knowledge_status_request, begin_reload_knowledge_request,
    begin_search_knowledge_request, begin_sync_knowledge_index_request,
};
use crate::settings::current_ui_language;
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, KnowledgeConfig, KnowledgeModelsConfig,
    KnowledgeRetrievalConfig, ObsidianKnowledgeConfig,
};
use klaw_knowledge::{
    KnowledgeEntry, KnowledgeHit, KnowledgeRuntimeSnapshot, KnowledgeRuntimeState,
    KnowledgeSyncProgress, KnowledgeSyncProgressStage,
};
use klaw_model::{ModelCapability, ModelService, ModelSummary};
use klaw_ui_kit::{LocaleDomain, Translator, label_with_hint, toggle::toggle};
use std::collections::HashMap;
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
    auto_index: bool,
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
            auto_index: config.obsidian.auto_index,
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

    fn to_config(&self, t: &Translator) -> Result<KnowledgeConfig, String> {
        let provider = self.provider.trim();
        if provider != "obsidian" {
            return Err(t.text("kn-validation-provider-obsidian"));
        }
        let vault_path = self.vault_path.trim();
        if self.enabled && vault_path.is_empty() {
            return Err(t.text("kn-validation-vault-required"));
        }
        let max_excerpt_length = parse_usize(&self.max_excerpt_length, "max_excerpt_length", t)?;
        let top_k = parse_usize(&self.top_k, "top_k", t)?;
        let rerank_candidates = parse_usize(&self.rerank_candidates, "rerank_candidates", t)?;
        let graph_hops = self
            .graph_hops
            .trim()
            .parse::<usize>()
            .map_err(|_| t.text("kn-validation-graph-hops"))?;
        let temporal_decay = self
            .temporal_decay
            .trim()
            .parse::<f32>()
            .map_err(|_| t.text("kn-validation-temporal-decay"))?;

        Ok(KnowledgeConfig {
            enabled: self.enabled,
            provider: provider.to_string(),
            obsidian: ObsidianKnowledgeConfig {
                vault_path: (!vault_path.is_empty()).then(|| vault_path.to_string()),
                auto_index: self.auto_index,
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
    status: Option<KnowledgeRuntimeSnapshot>,
    status_request: Option<RuntimeRequestHandle<KnowledgeRuntimeSnapshot>>,
    sync_request: Option<KnowledgeSyncRequestHandle>,
    sync_progress: Option<KnowledgeSyncProgress>,
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
            sync_progress: None,
            search_query: String::new(),
            search_limit: "10".to_string(),
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
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        let t = Self::translator();
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                self.refresh_status();
                self.refresh_model_options();
            }
            Err(err) => notifications.error(t.text_args(
                "kn-notify-config-load-failed",
                HashMap::from([("error", err.to_string())]),
            )),
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

    fn open_config_form(&mut self) {
        self.form = Some(KnowledgeConfigForm::from_config(&self.config.knowledge));
        self.refresh_model_options();
    }

    fn begin_sync(&mut self, notifications: &mut NotificationCenter) {
        if self.sync_request.is_some() {
            return;
        }
        let t = Self::translator();
        self.sync_request = Some(begin_sync_knowledge_index_request());
        self.sync_progress = None;
        notifications.info(t.text("kn-notify-syncing"));
    }

    fn begin_search(&mut self, notifications: &mut NotificationCenter) {
        if self.search_request.is_some() {
            return;
        }
        let t = Self::translator();
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            notifications.error(t.text("kn-notify-search-query-required"));
            return;
        }
        let limit = match parse_usize(&self.search_limit, "limit", &t) {
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
        let t = Self::translator();
        if let Some(mut request) = self.status_request.take() {
            match request.try_take_result() {
                Some(Ok(status)) => {
                    let keep_polling = matches!(
                        status.state,
                        KnowledgeRuntimeState::Loading | KnowledgeRuntimeState::Syncing
                    );
                    self.status = Some(status);
                    if keep_polling {
                        self.refresh_status();
                    }
                }
                Some(Err(err)) => notifications.error(t.text_args(
                    "kn-notify-status-failed",
                    HashMap::from([("error", err.to_string())]),
                )),
                None => self.status_request = Some(request),
            }
        }
        if let Some(mut request) = self.sync_request.take() {
            let mut completed = false;
            while let Some(message) = request.try_take_message() {
                match message {
                    KnowledgeSyncTaskMessage::Progress(progress) => {
                        self.sync_progress = Some(progress);
                    }
                    KnowledgeSyncTaskMessage::Completed(Ok(result)) => {
                        self.status = Some(KnowledgeRuntimeSnapshot {
                            state: KnowledgeRuntimeState::Ready,
                            status: Some(result.status),
                            error: None,
                        });
                        self.sync_progress = None;
                        completed = true;
                        notifications.success(t.text_args(
                            "kn-notify-sync-complete",
                            HashMap::from([
                                ("notes", result.indexed_notes.to_string()),
                                ("chunks", result.embedded_chunks.to_string()),
                            ]),
                        ));
                    }
                    KnowledgeSyncTaskMessage::Completed(Err(err)) => {
                        self.sync_progress = None;
                        completed = true;
                        notifications.error(t.text_args(
                            "kn-notify-sync-failed",
                            HashMap::from([("error", err.to_string())]),
                        ));
                    }
                }
            }
            if !completed && request.is_pending() {
                self.sync_request = Some(request);
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
                Some(Err(err)) => notifications.error(t.text_args(
                    "kn-notify-search-failed",
                    HashMap::from([("error", err.to_string())]),
                )),
                None => self.search_request = Some(request),
            }
        }
        if let Some(mut request) = self.entry_request.take() {
            match request.try_take_result() {
                Some(Ok(entry)) => self.selected_entry = entry,
                Some(Err(err)) => notifications.error(t.text_args(
                    "kn-notify-entry-failed",
                    HashMap::from([("error", err.to_string())]),
                )),
                None => self.entry_request = Some(request),
            }
        }
        if let Some(receiver) = self.model_options_request.take() {
            match receiver.try_recv() {
                Ok(Ok(mut options)) => {
                    options.sort_by(|left, right| left.model_id.cmp(&right.model_id));
                    self.model_options = options;
                }
                Ok(Err(err)) => notifications.error(t.text_args(
                    "kn-notify-models-failed",
                    HashMap::from([("error", err.to_string())]),
                )),
                Err(mpsc::TryRecvError::Empty) => self.model_options_request = Some(receiver),
                Err(mpsc::TryRecvError::Disconnected) => {
                    notifications.error(t.text("kn-notify-models-disconnected"));
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
        let t = Self::translator();
        let Some(store) = self.store.as_ref() else {
            notifications.error(t.text("kn-notify-store-unavailable"));
            return;
        };
        let Some(form) = self.form.clone() else {
            return;
        };
        match store.update_config(|config| {
            config.knowledge = form.to_config(&t).map_err(ConfigError::InvalidConfig)?;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.form = None;
                self.status_request = Some(begin_reload_knowledge_request());
                self.refresh_model_options();
                notifications.success(t.text("kn-notify-config-saved"));
            }
            Err(err) => notifications.error(t.text_args(
                "kn-notify-save-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn render_status(&self, ui: &mut egui::Ui) {
        let t = Self::translator();
        let snapshot = self.status.as_ref();
        let status = snapshot.and_then(|snapshot| snapshot.status.as_ref());
        ui.horizontal_wrapped(|ui| {
            status_chip(
                ui,
                t.text("kn-status-runtime"),
                snapshot
                    .map(|snapshot| runtime_state_label(snapshot.state, &t))
                    .unwrap_or_else(|| t.text("kn-state-unknown")),
            );
            status_chip(
                ui,
                t.text("kn-status-state"),
                status
                    .map(|status| {
                        if status.enabled {
                            t.text("kn-state-enabled")
                        } else {
                            t.text("kn-state-disabled-label")
                        }
                    })
                    .unwrap_or_else(|| t.text("kn-state-unknown")),
            );
            status_chip(
                ui,
                t.text("kn-status-provider"),
                status
                    .map(|status| status.provider.clone())
                    .unwrap_or_else(|| self.config.knowledge.provider.clone()),
            );
            status_chip(
                ui,
                t.text("kn-status-entries"),
                status
                    .map(|status| status.entry_count.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            status_chip(
                ui,
                t.text("kn-status-chunks"),
                status
                    .map(|status| status.chunk_count.to_string())
                    .unwrap_or_else(|| "-".to_string()),
            );
            status_chip(
                ui,
                t.text("kn-status-vectors"),
                status
                    .map(|status| format!("{}/{}", status.embedded_chunk_count, status.chunk_count))
                    .unwrap_or_else(|| "-".to_string()),
            );
        });
        if let Some(error) = snapshot.and_then(|snapshot| snapshot.error.as_deref()) {
            ui.colored_label(Color32::from_rgb(220, 80, 80), error);
        }
        ui.add_space(4.0);
        let vault = status
            .and_then(|status| status.vault_path.as_deref())
            .or(self.config.knowledge.obsidian.vault_path.as_deref());
        let vault_label = match vault {
            Some(path) => t.text_args(
                "kn-vault-label",
                HashMap::from([("path", path.to_string())]),
            ),
            None => t.text("kn-vault-not-configured"),
        };
        ui.small(vault_label);
    }

    fn render_search(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let search_enabled =
            self.search_request.is_none() && self.knowledge_runtime_accepts_search();
        ui.horizontal(|ui| {
            ui.label(t.text("kn-search-query"));
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.search_query)
                    .desired_width(360.0)
                    .hint_text(t.text("kn-search-hint")),
            );
            ui.label(t.text("kn-search-limit"));
            ui.add(egui::TextEdit::singleline(&mut self.search_limit).desired_width(60.0));
            let enter =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if ui
                .add_enabled(search_enabled, egui::Button::new(t.text("kn-btn-search")))
                .clicked()
                || (enter && search_enabled)
            {
                self.begin_search(notifications);
            }
        });
        if !self.knowledge_runtime_accepts_search() {
            ui.small(t.text("kn-search-not-ready"));
        }
        ui.add_space(8.0);

        ui.columns(2, |columns| {
            self.render_hits(&mut columns[0]);
            self.render_entry(&mut columns[1]);
        });
    }

    fn knowledge_runtime_accepts_search(&self) -> bool {
        self.status.as_ref().is_some_and(|snapshot| {
            matches!(
                snapshot.state,
                KnowledgeRuntimeState::Ready | KnowledgeRuntimeState::Syncing
            )
        })
    }

    fn render_hits(&mut self, ui: &mut egui::Ui) {
        let t = Self::translator();
        ui.strong(t.text_args(
            "kn-results-heading",
            HashMap::from([("count", self.hits.len().to_string())]),
        ));
        ui.add_space(4.0);
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("knowledge_hits")
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
                            ui.strong(t.text("kn-col-title"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("kn-col-score"));
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
        let t = Self::translator();
        ui.strong(t.text("kn-preview-heading"));
        ui.add_space(4.0);
        if self.entry_request.is_some() {
            ui.add(egui::Spinner::new());
            return;
        }
        let Some(selected_id) = self.selected_id.as_deref() else {
            ui.label(t.text("kn-preview-empty"));
            return;
        };
        let Some(entry) = self.selected_entry.as_ref() else {
            ui.label(t.text_args(
                "kn-preview-not-loaded",
                HashMap::from([("id", selected_id.to_string())]),
            ));
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.monospace(&entry.id);
            if !entry.tags.is_empty() {
                ui.label(t.text_args(
                    "kn-preview-tags",
                    HashMap::from([("tags", entry.tags.join(", "))]),
                ));
            }
        });
        ui.small(t.text_args(
            "kn-preview-uri",
            HashMap::from([("uri", entry.uri.clone())]),
        ));
        ui.add_space(6.0);
        let mut content = entry.content.clone();
        egui::ScrollArea::vertical()
            .id_salt("knowledge_entry_preview")
            .max_height(ui.available_height())
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut content)
                        .desired_width(f32::INFINITY)
                        .desired_rows(18)
                        .interactive(false),
                );
            });
    }

    fn render_sync_progress_window(&self, ui: &mut egui::Ui) {
        if self.sync_request.is_none() {
            return;
        }
        let t = Self::translator();
        egui::Window::new(t.text("kn-sync-title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .show(ui.ctx(), |ui| {
                let progress_height = ui.spacing().interact_size.y * 0.5;
                let Some(progress) = self.sync_progress.as_ref() else {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(t.text("kn-sync-preparing"));
                    });
                    return;
                };
                ui.strong(sync_stage_label(progress.stage, &t));
                if let Some(current_item) = progress.current_item.as_deref() {
                    ui.label(t.text_args(
                        "kn-sync-current",
                        HashMap::from([("item", current_item.to_string())]),
                    ));
                }
                let text = match progress.total {
                    Some(total) if total > 0 => t.text_args(
                        "kn-sync-progress",
                        HashMap::from([
                            ("completed", progress.completed.min(total).to_string()),
                            ("total", total.to_string()),
                        ]),
                    ),
                    _ => t.text_args(
                        "kn-sync-processed",
                        HashMap::from([("count", progress.completed.to_string())]),
                    ),
                };
                if let Some(total) = progress.total.filter(|total| *total > 0) {
                    let fraction = progress.completed as f32 / total as f32;
                    ui.add(
                        egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                            .text(text)
                            .desired_height(progress_height),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label(text);
                    });
                }
            });
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        let mut refresh_models_clicked = false;
        let model_options = self.model_options.clone();
        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new(t.text("kn-form-title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .show(ui.ctx(), |ui| {
                ui.small(status_label(self.config_path.as_deref(), &t));
                ui.separator();
                egui::Grid::new("knowledge-config-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        label_with_hint(
                            ui,
                            &t.text("kn-form-enabled"),
                            &t.text("kn-form-enabled-hint"),
                        );
                        ui.add(toggle(&mut form.enabled));
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-provider"),
                            &t.text("kn-form-provider-hint"),
                        );
                        provider_combo(ui, &mut form.provider, &t);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-vault-path"),
                            &t.text("kn-form-vault-path-hint"),
                        );
                        ui.text_edit_singleline(&mut form.vault_path);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-auto-index"),
                            &t.text("kn-form-auto-index-hint"),
                        );
                        ui.vertical(|ui| {
                            ui.add(toggle(&mut form.auto_index));
                            ui.small(t.text("kn-form-auto-index-note"));
                        });
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-max-excerpt"),
                            &t.text("kn-form-max-excerpt-hint"),
                        );
                        ui.text_edit_singleline(&mut form.max_excerpt_length);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-exclude-folders"),
                            &t.text("kn-form-exclude-folders-hint"),
                        );
                        ui.text_edit_singleline(&mut form.exclude_folders);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-top-k"),
                            &t.text("kn-form-top-k-hint"),
                        );
                        ui.text_edit_singleline(&mut form.top_k);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-rerank-candidates"),
                            &t.text("kn-form-rerank-candidates-hint"),
                        );
                        ui.text_edit_singleline(&mut form.rerank_candidates);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-graph-hops"),
                            &t.text("kn-form-graph-hops-hint"),
                        );
                        ui.text_edit_singleline(&mut form.graph_hops);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-temporal-decay"),
                            &t.text("kn-form-temporal-decay-hint"),
                        );
                        ui.text_edit_singleline(&mut form.temporal_decay);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-embedding-model"),
                            &t.text("kn-form-embedding-model-hint"),
                        );
                        model_combo(
                            ui,
                            "knowledge-config-embedding-model",
                            &mut form.embedding_model_id,
                            &model_options,
                            ModelCapability::Embedding,
                            &t,
                        );
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-orchestrator-model"),
                            &t.text("kn-form-orchestrator-model-hint"),
                        );
                        model_combo(
                            ui,
                            "knowledge-config-orchestrator-model",
                            &mut form.orchestrator_model_id,
                            &model_options,
                            ModelCapability::Orchestrator,
                            &t,
                        );
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("kn-form-reranker-model"),
                            &t.text("kn-form-reranker-model-hint"),
                        );
                        model_combo(
                            ui,
                            "knowledge-config-reranker-model",
                            &mut form.reranker_model_id,
                            &model_options,
                            ModelCapability::Rerank,
                            &t,
                        );
                        ui.end_row();
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.model_options_request.is_none(),
                            egui::Button::new(t.text("kn-form-refresh-models")),
                        )
                        .clicked()
                    {
                        refresh_models_clicked = true;
                    }
                    if self.model_options_request.is_some() {
                        ui.add(egui::Spinner::new());
                        ui.small(t.text("kn-form-models-loading"));
                    }
                });
                if self.model_options_request.is_none() && model_options.is_empty() {
                    ui.small(t.text("kn-form-models-empty"));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t.text("kn-form-save")).clicked() {
                        save_clicked = true;
                    }
                    if ui.button(t.text("kn-form-cancel")).clicked() {
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
        if refresh_models_clicked {
            self.refresh_model_options();
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
        let t = Self::translator();
        self.ensure_store_loaded(notifications);
        self.poll_requests(notifications);
        if self.has_pending_request() {
            ui.ctx().request_repaint_after(POLL_INTERVAL);
        }

        ui.heading(ctx.tab_title);
        ui.label(t.text("kn-subtitle"));
        ui.horizontal(|ui| {
            if ui
                .button(t.text_args(
                    "kn-btn-refresh",
                    HashMap::from([("icon", regular::ARROW_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.refresh_status();
            }
            if ui
                .add_enabled(
                    self.sync_request.is_none(),
                    egui::Button::new(t.text_args(
                        "kn-btn-sync",
                        HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                    )),
                )
                .clicked()
            {
                self.begin_sync(notifications);
            }
            if ui
                .button(t.text_args(
                    "kn-btn-config",
                    HashMap::from([("icon", regular::GEAR.to_string())]),
                ))
                .clicked()
            {
                self.open_config_form();
            }
            if self.has_pending_request() {
                ui.add(egui::Spinner::new());
            }
        });
        ui.separator();

        self.render_status(ui);
        ui.separator();
        self.render_search(ui, notifications);
        self.render_sync_progress_window(ui);
        self.render_form_window(ui, notifications);
    }
}

fn parse_usize(value: &str, field: &str, t: &Translator) -> Result<usize, String> {
    match value.trim().parse::<usize>() {
        Ok(value) if value > 0 => Ok(value),
        _ => Err(t.text_args(
            "kn-validation-positive-integer",
            HashMap::from([("field", field.to_string())]),
        )),
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

fn status_label(path: Option<&Path>, t: &Translator) -> String {
    match path {
        Some(path) => t.text_args(
            "kn-path-label",
            HashMap::from([("path", path.display().to_string())]),
        ),
        None => t.text("kn-path-not-loaded"),
    }
}

fn status_chip(ui: &mut egui::Ui, label: String, value: String) {
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

fn sync_stage_label(stage: KnowledgeSyncProgressStage, t: &Translator) -> String {
    match stage {
        KnowledgeSyncProgressStage::IndexingNotes => t.text("kn-sync-stage-indexing"),
        KnowledgeSyncProgressStage::EmbeddingChunks => t.text("kn-sync-stage-embedding"),
    }
}

fn runtime_state_label(state: KnowledgeRuntimeState, t: &Translator) -> String {
    match state {
        KnowledgeRuntimeState::Disabled => t.text("kn-state-disabled"),
        KnowledgeRuntimeState::Unconfigured => t.text("kn-state-unconfigured"),
        KnowledgeRuntimeState::Loading => t.text("kn-state-loading"),
        KnowledgeRuntimeState::Ready => t.text("kn-state-ready"),
        KnowledgeRuntimeState::Syncing => t.text("kn-state-syncing"),
        KnowledgeRuntimeState::Error => t.text("kn-state-error"),
    }
}

fn provider_combo(ui: &mut egui::Ui, provider: &mut String, t: &Translator) {
    let selected_text = if provider.trim().is_empty() {
        "obsidian".to_string()
    } else {
        provider.clone()
    };
    egui::ComboBox::from_id_salt("knowledge-config-provider")
        .selected_text(selected_text)
        .width(360.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(provider.trim() == "obsidian", "obsidian")
                .clicked()
            {
                *provider = "obsidian".to_string();
                ui.close();
            }
            let current = provider.trim();
            if !current.is_empty() && current != "obsidian" {
                ui.separator();
                ui.label(t.text_args(
                    "kn-provider-unsupported",
                    HashMap::from([("name", current.to_string())]),
                ));
            }
        });
}

fn model_combo(
    ui: &mut egui::Ui,
    id: &'static str,
    selected: &mut String,
    models: &[ModelSummary],
    preferred_capability: ModelCapability,
    t: &Translator,
) {
    let selected_text = if selected.trim().is_empty() {
        t.text("kn-model-not-configured")
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
                .selectable_label(
                    selected.trim().is_empty(),
                    t.text("kn-model-not-configured"),
                )
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
                    .selectable_label(
                        true,
                        t.text_args("kn-model-not-installed", HashMap::from([("name", current)])),
                    )
                    .clicked()
                {
                    ui.close();
                }
            }
            for model in ordered {
                let is_selected = selected.trim() == model.model_id;
                let label = model_option_label(&model, preferred_capability, t);
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

fn model_option_label(
    model: &ModelSummary,
    preferred_capability: ModelCapability,
    t: &Translator,
) -> String {
    if model.capabilities.contains(&preferred_capability) {
        return t.text_args(
            "kn-model-with-capability",
            HashMap::from([
                ("name", model.model_id.clone()),
                ("capability", capability_label(preferred_capability, t)),
            ]),
        );
    }
    if model.capabilities.is_empty() {
        return t.text_args(
            "kn-model-capability-unknown",
            HashMap::from([("name", model.model_id.clone())]),
        );
    }
    let capabilities = model
        .capabilities
        .iter()
        .map(|capability| capability_label(*capability, t))
        .collect::<Vec<_>>()
        .join(", ");
    t.text_args(
        "kn-model-with-capabilities",
        HashMap::from([
            ("name", model.model_id.clone()),
            ("capabilities", capabilities),
        ]),
    )
}

fn capability_label(capability: ModelCapability, t: &Translator) -> String {
    match capability {
        ModelCapability::Embedding => t.text("kn-capability-embedding"),
        ModelCapability::Rerank => t.text("kn-capability-rerank"),
        ModelCapability::Chat => t.text("kn-capability-chat"),
        ModelCapability::Orchestrator => t.text("kn-capability-orchestrator"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_ui_kit::UiLanguage;

    #[test]
    fn knowledge_form_does_not_touch_tool_config() {
        let t = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        let mut config = AppConfig::default();
        config.tools.knowledge.enabled = true;
        config.tools.knowledge.search_limit = 9;
        let before = config.tools.knowledge.clone();
        let form = KnowledgeConfigForm {
            enabled: true,
            provider: "obsidian".to_string(),
            vault_path: "/tmp/vault".to_string(),
            auto_index: true,
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

        config.knowledge = form.to_config(&t).expect("form should be valid");

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
        assert!(config.knowledge.obsidian.auto_index);
    }

    #[test]
    fn model_capability_rank_prefers_matching_models_then_unknown() {
        let matching = ModelSummary {
            model_id: "a".to_string(),
            repo_id: "repo/a".to_string(),
            revision: "main".to_string(),
            default_gguf_model_file: None,
            capabilities: vec![ModelCapability::Embedding],
            size_bytes: 1,
            installed_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let unknown = ModelSummary {
            model_id: "b".to_string(),
            repo_id: "repo/b".to_string(),
            revision: "main".to_string(),
            default_gguf_model_file: None,
            capabilities: vec![],
            size_bytes: 1,
            installed_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let other = ModelSummary {
            model_id: "c".to_string(),
            repo_id: "repo/c".to_string(),
            revision: "main".to_string(),
            default_gguf_model_file: None,
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

    #[test]
    fn opening_config_form_refreshes_model_options() {
        let mut panel = KnowledgePanel::default();
        panel.config.models.root_dir = Some(
            std::env::temp_dir()
                .join(format!("klaw-knowledge-models-{}", uuid::Uuid::new_v4()))
                .display()
                .to_string(),
        );

        panel.open_config_form();

        assert!(panel.form.is_some());
        assert!(panel.model_options_request.is_some());
    }
}
