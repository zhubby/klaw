use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigError, ConfigSnapshot, ConfigStore, EmbeddingConfig};
use klaw_memory::{
    LongTermMemoryKind, LongTermMemoryPromptOptions, LongTermMemoryStatus, MemoryError,
    MemoryRecord, MemoryStats, SqliteMemoryStatsService, read_long_term_kind,
    read_long_term_status, read_long_term_topic, render_long_term_memory_section,
};
use klaw_storage::{ChatRecord, SessionStorage, open_default_store};
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration as StdDuration;
use time::{Duration, OffsetDateTime};
use tokio::runtime::Builder;

const POLL_INTERVAL: StdDuration = StdDuration::from_millis(150);
const LONG_TERM_TABLE_HEIGHT: f32 = 320.0;
const SESSION_RESULTS_HEIGHT: f32 = 280.0;
const DIAGNOSTICS_SCOPES_HEIGHT: f32 = 220.0;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MemoryTab {
    #[default]
    LongTerm,
    SessionSearch,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StatusFilter {
    #[default]
    Active,
    Superseded,
    Archived,
    Rejected,
    All,
}

impl StatusFilter {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Superseded => "Superseded",
            Self::Archived => "Archived",
            Self::Rejected => "Rejected",
            Self::All => "All",
        }
    }

    fn matches(self, status: LongTermMemoryStatus) -> bool {
        match self {
            Self::Active => status == LongTermMemoryStatus::Active,
            Self::Superseded => status == LongTermMemoryStatus::Superseded,
            Self::Archived => status == LongTermMemoryStatus::Archived,
            Self::Rejected => status == LongTermMemoryStatus::Rejected,
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum KindFilter {
    #[default]
    All,
    Identity,
    Preference,
    ProjectRule,
    Workflow,
    Fact,
    Constraint,
}

impl KindFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All kinds",
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::ProjectRule => "project_rule",
            Self::Workflow => "workflow",
            Self::Fact => "fact",
            Self::Constraint => "constraint",
        }
    }

    fn matches(self, kind: LongTermMemoryKind) -> bool {
        match self {
            Self::All => true,
            Self::Identity => kind == LongTermMemoryKind::Identity,
            Self::Preference => kind == LongTermMemoryKind::Preference,
            Self::ProjectRule => kind == LongTermMemoryKind::ProjectRule,
            Self::Workflow => kind == LongTermMemoryKind::Workflow,
            Self::Fact => kind == LongTermMemoryKind::Fact,
            Self::Constraint => kind == LongTermMemoryKind::Constraint,
        }
    }
}

#[derive(Debug, Clone)]
struct SessionSearchForm {
    session_key: String,
    query: String,
    within_days: String,
    limit: String,
}

impl Default for SessionSearchForm {
    fn default() -> Self {
        Self {
            session_key: String::new(),
            query: String::new(),
            within_days: "3".to_string(),
            limit: "8".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct MemoryOverview {
    stats: MemoryStats,
    long_term_records: Vec<MemoryRecord>,
    prompt_preview: Option<String>,
    session_key_options: Vec<String>,
}

#[derive(Debug)]
struct PendingOverviewLoad {
    receiver: Receiver<Result<MemoryOverview, String>>,
}

#[derive(Debug)]
struct PendingSessionSearch {
    receiver: Receiver<Result<SessionSearchOutput, String>>,
}

#[derive(Debug, Clone)]
struct SessionSearchHit {
    session_key: String,
    ts_ms: i64,
    role: String,
    content: String,
    score: f64,
}

#[derive(Debug, Clone)]
struct SessionSearchOutput {
    input_session_key: String,
    base_session_key: String,
    session_keys: Vec<String>,
    within_days: i64,
    limit: usize,
    hits: Vec<SessionSearchHit>,
}

pub struct MemoryPanel {
    loaded: bool,
    loading: bool,
    refresh_queued: bool,
    overview: Option<MemoryOverview>,
    load_request: Option<PendingOverviewLoad>,
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    config: AppConfig,
    form: Option<MemoryConfigForm>,
    stats_window_open: bool,
    tab: MemoryTab,
    status_filter: StatusFilter,
    kind_filter: KindFilter,
    topic_filter: String,
    selected_long_term_id: Option<String>,
    session_form: SessionSearchForm,
    session_search_request: Option<PendingSessionSearch>,
    session_search_loading: bool,
    session_search_result: Option<SessionSearchOutput>,
}

impl Default for MemoryPanel {
    fn default() -> Self {
        Self {
            loaded: false,
            loading: false,
            refresh_queued: false,
            overview: None,
            load_request: None,
            store: None,
            config_path: None,
            config: AppConfig::default(),
            form: None,
            stats_window_open: false,
            tab: MemoryTab::LongTerm,
            status_filter: StatusFilter::Active,
            kind_filter: KindFilter::All,
            topic_filter: String::new(),
            selected_long_term_id: None,
            session_form: SessionSearchForm::default(),
            session_search_request: None,
            session_search_loading: false,
            session_search_result: None,
        }
    }
}

impl MemoryPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded || self.load_request.is_some() {
            return;
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let _ = notifications;
        if self.load_request.is_some() {
            self.refresh_queued = true;
            return;
        }
        self.loading = true;
        self.load_request = Some(PendingOverviewLoad {
            receiver: spawn_memory_task(|service| async move {
                let stats = service.collect(8).await?;
                let long_term_records = service.list_scope_records("long_term").await?;
                let session_store = open_default_store().await.map_err(MemoryError::Storage)?;
                let sessions = session_store
                    .list_sessions(
                        Some(1000),
                        0,
                        None,
                        None,
                        None,
                        None,
                        klaw_storage::SessionSortOrder::UpdatedAtDesc,
                    )
                    .await
                    .map_err(MemoryError::Storage)?;
                let session_key_options = aggregate_session_key_options(&sessions);
                let prompt_preview = render_long_term_memory_section(
                    &long_term_records,
                    &LongTermMemoryPromptOptions {
                        max_items: 12,
                        max_chars: 1200,
                        max_item_chars: 240,
                    },
                )
                .map(|content| format!("## Memory\n\n{content}"));
                Ok(MemoryOverview {
                    stats,
                    long_term_records,
                    prompt_preview,
                    session_key_options,
                })
            }),
        });
    }

    fn poll_load_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.load_request.take() else {
            return;
        };

        match request.receiver.try_recv() {
            Ok(result) => match result {
                Ok(overview) => {
                    if self.session_form.session_key.trim().is_empty()
                        || !overview
                            .session_key_options
                            .iter()
                            .any(|option| option == &self.session_form.session_key)
                    {
                        self.session_form.session_key = overview
                            .session_key_options
                            .first()
                            .cloned()
                            .unwrap_or_default();
                    }
                    self.overview = Some(overview);
                    self.loaded = true;
                    self.loading = false;
                    if self.refresh_queued {
                        self.refresh_queued = false;
                        self.refresh(notifications);
                    }
                }
                Err(err) => {
                    self.loading = false;
                    notifications.error(format!("Failed to load memory panel: {err}"));
                    if self.refresh_queued {
                        self.refresh_queued = false;
                        self.refresh(notifications);
                    }
                }
            },
            Err(TryRecvError::Empty) => {
                self.load_request = Some(request);
            }
            Err(TryRecvError::Disconnected) => {
                self.loading = false;
                notifications.error("Memory panel loader closed unexpectedly");
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

        egui::Window::new("Long-term Memory Embedding Config")
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
                        ui.label("Embedding enabled");
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

                ui.add_space(6.0);
                ui.small(
                    "This config controls long-term memory embedding and indexing. Tool-level search settings stay under the Tool panel.",
                );

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

    fn begin_session_search(&mut self, notifications: &mut NotificationCenter) {
        if self.session_search_request.is_some() {
            return;
        }

        let session_key = self.session_form.session_key.trim().to_string();
        let query = self.session_form.query.trim().to_string();
        if session_key.is_empty() {
            notifications.error("Session search requires a session key");
            return;
        }
        if query.is_empty() {
            notifications.error("Session search requires a query");
            return;
        }

        let within_days = match self.session_form.within_days.trim().parse::<i64>() {
            Ok(value) if value > 0 => value,
            _ => {
                notifications.error("within_days must be a positive integer");
                return;
            }
        };
        let limit = match self.session_form.limit.trim().parse::<usize>() {
            Ok(value) if value > 0 => value,
            _ => {
                notifications.error("limit must be a positive integer");
                return;
            }
        };

        self.session_search_loading = true;
        self.session_search_request = Some(PendingSessionSearch {
            receiver: spawn_session_search_task(session_key, query, within_days, limit),
        });
    }

    fn poll_session_search_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.session_search_request.take() else {
            return;
        };

        match request.receiver.try_recv() {
            Ok(result) => match result {
                Ok(output) => {
                    self.session_search_loading = false;
                    self.session_search_result = Some(output);
                }
                Err(err) => {
                    self.session_search_loading = false;
                    notifications.error(format!("Session search failed: {err}"));
                }
            },
            Err(TryRecvError::Empty) => {
                self.session_search_request = Some(request);
            }
            Err(TryRecvError::Disconnected) => {
                self.session_search_loading = false;
                notifications.error("Session search task disconnected unexpectedly");
            }
        }
    }

    fn render_summary_cards(&self, ui: &mut egui::Ui, overview: &MemoryOverview) {
        let active = count_records_with_status(
            &overview.long_term_records,
            Some(LongTermMemoryStatus::Active),
        );
        let superseded = count_records_with_status(
            &overview.long_term_records,
            Some(LongTermMemoryStatus::Superseded),
        );
        let archived = count_records_with_status(
            &overview.long_term_records,
            Some(LongTermMemoryStatus::Archived),
        );
        let prompt_lines = overview
            .prompt_preview
            .as_ref()
            .map(|preview| preview.lines().count().saturating_sub(2))
            .unwrap_or_default();

        ui.horizontal_wrapped(|ui| {
            summary_chip(ui, "Active long-term", active.to_string());
            summary_chip(ui, "Superseded", superseded.to_string());
            summary_chip(ui, "Archived", archived.to_string());
            summary_chip(ui, "Prompt lines", prompt_lines.to_string());
            summary_chip(
                ui,
                "Session search",
                if self.config.tools.memory.enabled {
                    format!("enabled ({})", self.config.tools.memory.search_limit)
                } else {
                    "disabled".to_string()
                },
            );
        });
    }

    fn render_tab_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, MemoryTab::LongTerm, "Long-term");
            ui.selectable_value(&mut self.tab, MemoryTab::SessionSearch, "Session Search");
            ui.selectable_value(&mut self.tab, MemoryTab::Diagnostics, "Diagnostics");
        });
    }

    fn render_long_term_tab(&mut self, ui: &mut egui::Ui, overview: &MemoryOverview) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("memory-status-filter")
                .selected_text(self.status_filter.label())
                .show_ui(ui, |ui| {
                    for filter in [
                        StatusFilter::Active,
                        StatusFilter::Superseded,
                        StatusFilter::Archived,
                        StatusFilter::Rejected,
                        StatusFilter::All,
                    ] {
                        ui.selectable_value(&mut self.status_filter, filter, filter.label());
                    }
                });

            egui::ComboBox::from_id_salt("memory-kind-filter")
                .selected_text(self.kind_filter.label())
                .show_ui(ui, |ui| {
                    for filter in [
                        KindFilter::All,
                        KindFilter::Identity,
                        KindFilter::Preference,
                        KindFilter::ProjectRule,
                        KindFilter::Workflow,
                        KindFilter::Fact,
                        KindFilter::Constraint,
                    ] {
                        ui.selectable_value(&mut self.kind_filter, filter, filter.label());
                    }
                });

            ui.label("Topic");
            ui.add(
                egui::TextEdit::singleline(&mut self.topic_filter)
                    .desired_width(180.0)
                    .hint_text("reply_language"),
            );
        });
        ui.add_space(8.0);

        let filtered = filter_long_term_records(
            &overview.long_term_records,
            self.status_filter,
            self.kind_filter,
            &self.topic_filter,
        );
        if self
            .selected_long_term_id
            .as_deref()
            .is_none_or(|selected| !filtered.iter().any(|record| record.id == selected))
        {
            self.selected_long_term_id = filtered.first().map(|record| record.id.clone());
        }
        ui.label(format!("Records: {}", filtered.len()));

        egui::ScrollArea::vertical()
            .max_height(LONG_TERM_TABLE_HEIGHT)
            .show(ui, |ui| {
                let row_height = ui.spacing().interact_size.y;
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .sense(egui::Sense::click())
                    .column(Column::auto().at_least(90.0))
                    .column(Column::auto().at_least(110.0))
                    .column(Column::auto().at_least(120.0))
                    .column(Column::auto().at_least(60.0))
                    .column(Column::remainder().at_least(180.0))
                    .column(Column::auto().at_least(170.0))
                    .header(row_height, |mut header| {
                        header.col(|ui| {
                            ui.strong("Status");
                        });
                        header.col(|ui| {
                            ui.strong("Kind");
                        });
                        header.col(|ui| {
                            ui.strong("Topic");
                        });
                        header.col(|ui| {
                            ui.strong("Pin");
                        });
                        header.col(|ui| {
                            ui.strong("Governance");
                        });
                        header.col(|ui| {
                            ui.strong("Updated");
                        });
                    })
                    .body(|body| {
                        body.rows(row_height, filtered.len(), |mut row| {
                            let record = filtered[row.index()];
                            let is_selected =
                                self.selected_long_term_id.as_deref() == Some(record.id.as_str());
                            row.set_selected(is_selected);
                            row.col(|ui| {
                                ui.monospace(status_label(record));
                            });
                            row.col(|ui| {
                                ui.monospace(kind_label(record));
                            });
                            row.col(|ui| {
                                ui.label(
                                    read_long_term_topic(record).unwrap_or_else(|| "-".to_string()),
                                );
                            });
                            row.col(|ui| {
                                ui.monospace(if record.pinned { "yes" } else { "no" });
                            });
                            row.col(|ui| {
                                ui.small(governance_summary(record));
                            });
                            row.col(|ui| {
                                ui.monospace(format_timestamp_millis(record.updated_at_ms));
                            });

                            let response = row.response();
                            if response.clicked() {
                                self.selected_long_term_id = Some(record.id.clone());
                            }
                        });
                    });
            });

        ui.add_space(10.0);
        ui.separator();
        ui.strong("Content");
        match selected_long_term_record(&filtered, self.selected_long_term_id.as_deref()) {
            Some(record) => {
                ui.small(format!(
                    "ID: {} | kind: {} | status: {} | topic: {} | updated: {}",
                    record.id,
                    kind_label(record),
                    status_label(record),
                    read_long_term_topic(record).unwrap_or_else(|| "-".to_string()),
                    format_timestamp_millis(record.updated_at_ms)
                ));
                let governance = governance_summary(record);
                if governance != "-" {
                    ui.small(format!("Governance: {governance}"));
                }
                ui.add_space(6.0);
                let mut content = record.content.clone();
                ui.add(
                    egui::TextEdit::multiline(&mut content)
                        .desired_width(f32::INFINITY)
                        .desired_rows(10)
                        .interactive(false),
                );
            }
            None => {
                ui.label("No long-term memory matches the current filters.");
            }
        }
    }

    fn render_session_search_tab(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        overview: &MemoryOverview,
    ) {
        ui.label("Search recent session memory over existing session/chat history.");
        ui.add_space(6.0);
        egui::Grid::new("memory-session-search-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Session key");
                egui::ComboBox::from_id_salt("memory-session-key")
                    .selected_text(if self.session_form.session_key.trim().is_empty() {
                        "Select session key"
                    } else {
                        self.session_form.session_key.as_str()
                    })
                    .width(320.0)
                    .show_ui(ui, |ui| {
                        for option in &overview.session_key_options {
                            ui.selectable_value(
                                &mut self.session_form.session_key,
                                option.clone(),
                                option,
                            );
                        }
                    });
                ui.end_row();

                ui.label("Query");
                ui.add(
                    egui::TextEdit::singleline(&mut self.session_form.query)
                        .desired_width(320.0)
                        .hint_text("deploy rollback"),
                );
                ui.end_row();

                ui.label("Within days");
                ui.add(
                    egui::TextEdit::singleline(&mut self.session_form.within_days)
                        .desired_width(80.0),
                );
                ui.end_row();

                ui.label("Limit");
                ui.add(
                    egui::TextEdit::singleline(&mut self.session_form.limit).desired_width(80.0),
                );
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Search", regular::MAGNIFYING_GLASS))
                .clicked()
            {
                self.begin_session_search(notifications);
            }
            if self.session_search_loading {
                ui.add(egui::Spinner::new());
                ui.small("Searching...");
            }
        });
        ui.add_space(8.0);

        let Some(result) = self.session_search_result.clone() else {
            ui.small(
                "Run a session search to inspect the resolved base session and matching history.",
            );
            return;
        };

        ui.label(format!("Input session: {}", result.input_session_key));
        ui.label(format!(
            "Resolved base session: {}",
            result.base_session_key
        ));
        ui.label(format!(
            "Resolved sessions: {}",
            result.session_keys.join(", ")
        ));
        ui.label(format!(
            "Window: {} day(s), limit {}",
            result.within_days, result.limit
        ));
        ui.add_space(6.0);

        if result.hits.is_empty() {
            ui.small("No matching session messages found for this query and window.");
            return;
        }

        egui::ScrollArea::vertical()
            .max_height(SESSION_RESULTS_HEIGHT)
            .show(ui, |ui| {
                let row_height = ui.spacing().interact_size.y;
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(160.0))
                    .column(Column::auto().at_least(90.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::remainder().at_least(360.0))
                    .column(Column::auto().at_least(80.0))
                    .header(row_height, |mut header| {
                        header.col(|ui| {
                            ui.strong("Session");
                        });
                        header.col(|ui| {
                            ui.strong("Time");
                        });
                        header.col(|ui| {
                            ui.strong("Role");
                        });
                        header.col(|ui| {
                            ui.strong("Content");
                        });
                        header.col(|ui| {
                            ui.strong("Score");
                        });
                    })
                    .body(|body| {
                        body.rows(row_height, result.hits.len(), |mut row| {
                            let hit = &result.hits[row.index()];
                            row.col(|ui| {
                                ui.monospace(&hit.session_key);
                            });
                            row.col(|ui| {
                                ui.monospace(format_timestamp_millis(hit.ts_ms));
                            });
                            row.col(|ui| {
                                ui.monospace(&hit.role);
                            });
                            row.col(|ui| {
                                ui.label(&hit.content);
                            });
                            row.col(|ui| {
                                ui.monospace(format!("{:.2}", hit.score));
                            });
                        });
                    });
            });
    }

    fn render_diagnostics_tab(&mut self, ui: &mut egui::Ui, overview: &MemoryOverview) {
        render_memory_stats_grid(ui, &overview.stats);
        ui.add_space(10.0);
        ui.separator();
        ui.strong("Top Scopes");

        if overview.stats.top_scopes.is_empty() {
            ui.small("No scope data.");
            return;
        }

        egui::ScrollArea::vertical()
            .max_height(DIAGNOSTICS_SCOPES_HEIGHT)
            .show(ui, |ui| {
                let row_height = ui.spacing().interact_size.y;
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::remainder().at_least(320.0))
                    .column(Column::auto().at_least(80.0))
                    .header(row_height, |mut header| {
                        header.col(|ui| {
                            ui.strong("Scope");
                        });
                        header.col(|ui| {
                            ui.strong("Count");
                        });
                    })
                    .body(|body| {
                        body.rows(row_height, overview.stats.top_scopes.len(), |mut row| {
                            let scope = &overview.stats.top_scopes[row.index()];
                            row.col(|ui| {
                                ui.label(&scope.scope);
                            });
                            row.col(|ui| {
                                ui.monospace(scope.count.to_string());
                            });
                        });
                    });
            });
    }

    fn render_stats_window(&mut self, ctx: &egui::Context, overview: &MemoryOverview) {
        let mut open = self.stats_window_open;
        egui::Window::new("Memory Info")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(480.0);
                render_memory_stats_grid(ui, &overview.stats);
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
        self.ensure_store_loaded(notifications);
        self.ensure_loaded(notifications);
        self.poll_load_request(notifications);
        self.poll_session_search_request(notifications);
        if self.load_request.is_some() || self.session_search_request.is_some() {
            ui.ctx().request_repaint_after(POLL_INTERVAL);
        }

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
            if self.loading {
                ui.add(egui::Spinner::new());
                ui.small("Loading...");
            }
        });
        ui.separator();

        let Some(overview) = self.overview.clone() else {
            ui.label("No memory data available yet.");
            self.render_form_window(ui, notifications);
            return;
        };

        self.render_summary_cards(ui, &overview);
        ui.add_space(8.0);
        self.render_tab_selector(ui);
        ui.separator();

        match self.tab {
            MemoryTab::LongTerm => self.render_long_term_tab(ui, &overview),
            MemoryTab::SessionSearch => {
                self.render_session_search_tab(ui, notifications, &overview)
            }
            MemoryTab::Diagnostics => self.render_diagnostics_tab(ui, &overview),
        }

        self.render_form_window(ui, notifications);
        if self.stats_window_open {
            self.render_stats_window(ui.ctx(), &overview);
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

fn filter_long_term_records<'a>(
    records: &'a [MemoryRecord],
    status_filter: StatusFilter,
    kind_filter: KindFilter,
    topic_filter: &str,
) -> Vec<&'a MemoryRecord> {
    let topic_filter = topic_filter.trim().to_ascii_lowercase();
    records
        .iter()
        .filter(|record| {
            let status = read_long_term_status(record).unwrap_or(LongTermMemoryStatus::Active);
            let kind = read_long_term_kind(record).unwrap_or(LongTermMemoryKind::Fact);
            let topic = read_long_term_topic(record).unwrap_or_default();
            status_filter.matches(status)
                && kind_filter.matches(kind)
                && (topic_filter.is_empty()
                    || topic.to_ascii_lowercase().contains(topic_filter.as_str()))
        })
        .collect()
}

fn selected_long_term_record<'a>(
    records: &'a [&'a MemoryRecord],
    selected_id: Option<&str>,
) -> Option<&'a MemoryRecord> {
    selected_id
        .and_then(|selected_id| {
            records
                .iter()
                .copied()
                .find(|record| record.id == selected_id)
        })
        .or_else(|| records.first().copied())
}

fn aggregate_session_key_options(sessions: &[klaw_storage::SessionIndex]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut options = Vec::new();
    for session in sessions {
        if seen.insert(session.session_key.clone()) {
            options.push(session.session_key.clone());
        }
        if let Some(active_session_key) = session.active_session_key.as_ref()
            && !active_session_key.trim().is_empty()
            && seen.insert(active_session_key.clone())
        {
            options.push(active_session_key.clone());
        }
    }
    options
}

fn count_records_with_status(
    records: &[MemoryRecord],
    status: Option<LongTermMemoryStatus>,
) -> usize {
    records
        .iter()
        .filter(|record| {
            let current = read_long_term_status(record).unwrap_or(LongTermMemoryStatus::Active);
            status.is_none_or(|expected| expected == current)
        })
        .count()
}

fn summary_chip(ui: &mut egui::Ui, label: &str, value: String) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.small(label);
            ui.strong(value);
        });
    });
}

fn kind_label(record: &MemoryRecord) -> &'static str {
    read_long_term_kind(record)
        .unwrap_or(LongTermMemoryKind::Fact)
        .as_str()
}

fn status_label(record: &MemoryRecord) -> &'static str {
    read_long_term_status(record)
        .unwrap_or(LongTermMemoryStatus::Active)
        .as_str()
}

fn governance_summary(record: &MemoryRecord) -> String {
    let supersedes = read_string_list_field(&record.metadata, "supersedes");
    let superseded_by = read_string_field(&record.metadata, "superseded_by");
    match (supersedes.is_empty(), superseded_by) {
        (false, Some(superseded_by)) => {
            format!(
                "supersedes: {}; superseded_by: {superseded_by}",
                supersedes.join(", ")
            )
        }
        (false, None) => format!("supersedes: {}", supersedes.join(", ")),
        (true, Some(superseded_by)) => format!("superseded_by: {superseded_by}"),
        (true, None) => "-".to_string(),
    }
}

fn read_string_list_field(metadata: &Value, field: &str) -> Vec<String> {
    match metadata.get(field) {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn read_string_field(metadata: &Value, field: &str) -> Option<String> {
    metadata
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn spawn_memory_task<T, F, Fut>(op: F) -> Receiver<Result<T, String>>
where
    T: Send + 'static,
    F: FnOnce(SqliteMemoryStatsService) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, MemoryError>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))
            .and_then(|runtime| {
                runtime.block_on(async move {
                    let service = SqliteMemoryStatsService::open_default()
                        .await
                        .map_err(|err| format!("failed to open memory stats service: {err}"))?;
                    op(service)
                        .await
                        .map_err(|err| format!("memory stats operation failed: {err}"))
                })
            });
        let _ = tx.send(result);
    });
    rx
}

fn spawn_session_search_task(
    input_session_key: String,
    query: String,
    within_days: i64,
    limit: usize,
) -> Receiver<Result<SessionSearchOutput, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))
            .and_then(|runtime| {
                runtime.block_on(async move {
                    let store = open_default_store()
                        .await
                        .map_err(|err| format!("failed to open session store: {err}"))?;
                    search_session_history(store, input_session_key, query, within_days, limit)
                        .await
                })
            });
        let _ = tx.send(result);
    });
    rx
}

async fn search_session_history(
    store: impl SessionStorage,
    input_session_key: String,
    query: String,
    within_days: i64,
    limit: usize,
) -> Result<SessionSearchOutput, String> {
    let base_session_key = match store
        .get_session_by_active_session_key(&input_session_key)
        .await
    {
        Ok(base) => base.session_key,
        Err(_) => input_session_key.clone(),
    };
    let active_session_key = store
        .get_session(&base_session_key)
        .await
        .ok()
        .and_then(|session| session.active_session_key)
        .filter(|value| !value.trim().is_empty());
    let mut session_keys = vec![base_session_key.clone()];
    if let Some(active_session_key) = active_session_key {
        if active_session_key != base_session_key {
            session_keys.push(active_session_key);
        }
    }
    if input_session_key != base_session_key && !session_keys.contains(&input_session_key) {
        session_keys.push(input_session_key.clone());
    }

    let cutoff_ms = (OffsetDateTime::now_utc() - Duration::days(within_days))
        .unix_timestamp_nanos()
        .saturating_div(1_000_000) as i64;
    let mut hits = Vec::new();
    for session_key in &session_keys {
        let records = store
            .read_chat_records(session_key)
            .await
            .map_err(|err| format!("failed to read chat records: {err}"))?;
        for record in records.into_iter().rev().take(1000) {
            if record.ts_ms < cutoff_ms {
                continue;
            }
            if !matches!(record.role.as_str(), "user" | "assistant") {
                continue;
            }
            let Some(score) = session_match_score(&record, &query) else {
                continue;
            };
            hits.push(SessionSearchHit {
                session_key: session_key.clone(),
                ts_ms: record.ts_ms,
                role: record.role,
                content: record.content,
                score,
            });
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.ts_ms.cmp(&a.ts_ms))
    });
    hits.truncate(limit);
    Ok(SessionSearchOutput {
        input_session_key,
        base_session_key,
        session_keys,
        within_days,
        limit,
        hits,
    })
}

fn session_match_score(record: &ChatRecord, query: &str) -> Option<f64> {
    let normalized_content = record.content.to_ascii_lowercase();
    let normalized_query = query.trim().to_ascii_lowercase();
    if normalized_query.is_empty() {
        return None;
    }

    let phrase_match = normalized_content.contains(&normalized_query);
    let tokens = normalized_query
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let token_hits = tokens
        .iter()
        .filter(|token| normalized_content.contains(**token))
        .count();
    if !phrase_match && token_hits == 0 {
        return None;
    }

    let token_score = if tokens.is_empty() {
        0.0
    } else {
        token_hits as f64 / tokens.len() as f64
    };
    let role_boost = if record.role == "assistant" { 0.2 } else { 0.0 };
    Some(if phrase_match {
        2.0 + token_score + role_boost
    } else {
        token_score + role_boost
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::ModelProviderConfig;
    use klaw_storage::SessionIndex;
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

    fn long_term_record(
        id: &str,
        content: &str,
        kind: &str,
        status: &str,
        topic: Option<&str>,
    ) -> MemoryRecord {
        let mut metadata = serde_json::Map::new();
        metadata.insert("kind".to_string(), Value::String(kind.to_string()));
        metadata.insert("status".to_string(), Value::String(status.to_string()));
        if let Some(topic) = topic {
            metadata.insert("topic".to_string(), Value::String(topic.to_string()));
        }
        MemoryRecord {
            id: id.to_string(),
            scope: "long_term".to_string(),
            content: content.to_string(),
            metadata: Value::Object(metadata),
            pinned: false,
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    fn session_index(session_key: &str, active_session_key: Option<&str>) -> SessionIndex {
        SessionIndex {
            session_key: session_key.to_string(),
            chat_id: "chat-1".to_string(),
            channel: "terminal".to_string(),
            title: None,
            active_session_key: active_session_key.map(ToString::to_string),
            model_provider: None,
            model_provider_explicit: false,
            model: None,
            model_explicit: false,
            delivery_metadata_json: None,
            created_at_ms: 1,
            updated_at_ms: 2,
            last_message_at_ms: 2,
            turn_count: 1,
            jsonl_path: "/tmp/session.jsonl".to_string(),
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
    fn filter_long_term_records_applies_status_kind_and_topic() {
        let records = vec![
            long_term_record(
                "1",
                "Default language is Chinese",
                "preference",
                "active",
                Some("reply_language"),
            ),
            long_term_record(
                "2",
                "Default language is English",
                "preference",
                "superseded",
                Some("reply_language"),
            ),
            long_term_record(
                "3",
                "Follow project rule",
                "project_rule",
                "active",
                Some("code_style"),
            ),
        ];

        let filtered = filter_long_term_records(
            &records,
            StatusFilter::Active,
            KindFilter::Preference,
            "reply",
        );

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "1");
    }

    #[test]
    fn governance_summary_renders_supersedes_information() {
        let record = MemoryRecord {
            id: "1".to_string(),
            scope: "long_term".to_string(),
            content: "Use Chinese".to_string(),
            metadata: serde_json::json!({
                "supersedes": ["old-1"],
                "superseded_by": "new-2"
            }),
            pinned: false,
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        let summary = governance_summary(&record);
        assert!(summary.contains("old-1"));
        assert!(summary.contains("new-2"));
    }

    #[test]
    fn session_match_score_prefers_phrase_and_assistant_role() {
        let assistant = ChatRecord {
            ts_ms: 1,
            role: "assistant".to_string(),
            content: "deploy rollback procedure".to_string(),
            metadata_json: None,
            message_id: None,
        };
        let user = ChatRecord {
            ts_ms: 1,
            role: "user".to_string(),
            content: "deploy rollback".to_string(),
            metadata_json: None,
            message_id: None,
        };

        let assistant_score =
            session_match_score(&assistant, "deploy rollback").unwrap_or_default();
        let user_score = session_match_score(&user, "rollback").unwrap_or_default();
        assert!(assistant_score > user_score);
    }

    #[test]
    fn aggregate_session_key_options_dedupes_base_and_active_keys() {
        let sessions = vec![
            session_index("base-1", Some("active-1")),
            session_index("base-2", Some("active-1")),
            session_index("base-1", None),
        ];

        let options = aggregate_session_key_options(&sessions);

        assert_eq!(options, vec!["base-1", "active-1", "base-2"]);
    }
}
