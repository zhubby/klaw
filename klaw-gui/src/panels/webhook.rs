use crate::GatewayStatusSnapshot;
use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::request_gateway_status;
use crate::time_format::format_timestamp_millis;
use chrono::{Datelike, Local, NaiveDate};
use egui::{Color32, RichText};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{
    AppConfig, ConfigError, ConfigSnapshot, ConfigStore, GatewayWebhookConfig, TailscaleMode,
};
use klaw_gateway::{WEBHOOK_AGENTS_PATH, WEBHOOK_EVENTS_PATH};
use klaw_session::{
    SessionError, SessionListQuery, SessionManager, SqliteSessionManager, WebhookAgentQuery,
    WebhookAgentRecord, WebhookEventQuery, WebhookEventRecord, WebhookEventSortOrder,
    WebhookEventStatus,
};
use klaw_util::default_data_dir;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 50.0;
const PROMPT_LIST_HEIGHT: f32 = 320.0;
const PROMPT_TEXT_HEIGHT: f32 = 260.0;
const PREVIEW_HEIGHT: f32 = 260.0;
const SUMMARY_WINDOW_HEIGHT: f32 = 260.0;

struct PendingWebhookRowsRequest {
    receiver: Receiver<Result<Vec<WebhookListRow>, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WebhookQueryKind {
    Events,
    Agents,
}

#[derive(Debug, Clone)]
enum WebhookListRow {
    Event(WebhookEventRecord),
    Agent(WebhookAgentRecord),
}

#[derive(Debug, Clone)]
struct WebhookSummaryState {
    title: String,
    summary: String,
}

impl WebhookListRow {
    fn id(&self) -> &str {
        match self {
            Self::Event(record) => &record.id,
            Self::Agent(record) => &record.id,
        }
    }

    fn received_at_ms(&self) -> i64 {
        match self {
            Self::Event(record) => record.received_at_ms,
            Self::Agent(record) => record.received_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptTemplateRecord {
    hook_id: String,
    path: PathBuf,
}

#[derive(Debug, Clone, Default)]
struct CreatePromptState {
    hook_id: String,
    markdown: String,
    status_message: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct InspectPromptState {
    templates: Vec<PromptTemplateRecord>,
    selected_hook_id: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct ViewPromptState {
    hook_id: String,
    markdown: String,
}

#[derive(Debug, Clone)]
struct DeletePromptState {
    hook_id: String,
    path: PathBuf,
}

#[derive(Debug, Clone, Default)]
struct TrickPromptState {
    hook_id: String,
    session_options: Vec<TrickSessionOption>,
    base_session_key: String,
    provider: String,
    model: String,
    generated_url: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrickSessionOption {
    base_session_key: String,
    label: String,
}

#[derive(Debug, Clone, Default)]
struct WebhookConfigForm {
    enabled: bool,
    events_enabled: bool,
    events_max_body_bytes: String,
    agents_enabled: bool,
    agents_max_body_bytes: String,
}

impl WebhookConfigForm {
    fn from_config(config: &GatewayWebhookConfig) -> Self {
        Self {
            enabled: config.enabled,
            events_enabled: config.events.enabled,
            events_max_body_bytes: config.events.max_body_bytes.to_string(),
            agents_enabled: config.agents.enabled,
            agents_max_body_bytes: config.agents.max_body_bytes.to_string(),
        }
    }

    fn apply_to_config(&self, config: &mut AppConfig) -> Result<(), String> {
        let events_max_body_bytes = self
            .events_max_body_bytes
            .trim()
            .parse::<usize>()
            .map_err(|_| "events max_body_bytes must be a valid integer".to_string())?;
        let agents_max_body_bytes = self
            .agents_max_body_bytes
            .trim()
            .parse::<usize>()
            .map_err(|_| "agents max_body_bytes must be a valid integer".to_string())?;

        config.gateway.webhook.enabled = self.enabled;
        config.gateway.webhook.events.enabled = self.events_enabled;
        config.gateway.webhook.events.max_body_bytes = events_max_body_bytes;
        config.gateway.webhook.agents.enabled = self.agents_enabled;
        config.gateway.webhook.agents.max_body_bytes = agents_max_body_bytes;
        Ok(())
    }
}

pub struct WebhookPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    config_form: WebhookConfigForm,
    config_window_open: bool,
    loaded: bool,
    query_kind: WebhookQueryKind,
    rows: Vec<WebhookListRow>,
    rows_request: Option<PendingWebhookRowsRequest>,
    rows_refresh_queued: bool,
    gateway_status: Option<GatewayStatusSnapshot>,
    gateway_status_request: Option<Receiver<Result<GatewayStatusSnapshot, String>>>,
    source_filter: String,
    event_type_filter: String,
    session_filter: String,
    status_filter: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    page: i64,
    size: i64,
    sort_order: WebhookEventSortOrder,
    selected_id: Option<String>,
    summary_popup: Option<WebhookSummaryState>,
    prompt_dir: Option<PathBuf>,
    create_prompt_open: bool,
    create_prompt: CreatePromptState,
    inspect_prompt_open: bool,
    inspect_prompt: InspectPromptState,
    view_prompt: Option<ViewPromptState>,
    delete_prompt: Option<DeletePromptState>,
    trick_prompt: Option<TrickPromptState>,
}

impl Default for WebhookPanel {
    fn default() -> Self {
        let today = Local::now().date_naive();
        let one_year_ago = today - chrono::Duration::days(365);
        Self {
            store: None,
            config_path: None,
            revision: None,
            config: AppConfig::default(),
            config_form: WebhookConfigForm::default(),
            config_window_open: false,
            loaded: false,
            query_kind: WebhookQueryKind::Events,
            rows: Vec::new(),
            rows_request: None,
            rows_refresh_queued: false,
            gateway_status: None,
            gateway_status_request: None,
            source_filter: String::new(),
            event_type_filter: String::new(),
            session_filter: String::new(),
            status_filter: String::new(),
            start_date: Some(one_year_ago),
            end_date: Some(today),
            page: 1,
            size: 100,
            sort_order: WebhookEventSortOrder::ReceivedAtDesc,
            selected_id: None,
            summary_popup: None,
            prompt_dir: None,
            create_prompt_open: false,
            create_prompt: CreatePromptState::default(),
            inspect_prompt_open: false,
            inspect_prompt: InspectPromptState::default(),
            view_prompt: None,
            delete_prompt: None,
            trick_prompt: None,
        }
    }
}

impl WebhookPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        self.ensure_store_loaded(notifications);
        if self.loaded {
            return;
        }
        self.refresh_gateway_status();
        self.refresh(notifications);
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
        self.config_form = WebhookConfigForm::from_config(&self.config.gateway.webhook);
        self.prompt_dir = prompt_templates_dir_from_config(&self.config);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let _ = notifications;
        let size = self.size.max(1);
        let page = self.page.max(1);
        let offset = (page - 1) * size;
        if self.rows_request.is_some() {
            self.rows_refresh_queued = true;
            return;
        }

        let query_kind = self.query_kind;
        let source_filter = self.source_filter.clone();
        let event_type_filter = self.event_type_filter.clone();
        let session_filter = self.session_filter.clone();
        let status_filter = self.status_filter.clone();
        let start_date = self.start_date;
        let end_date = self.end_date;
        let sort_order = self.sort_order;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = run_session_task(move |manager| async move {
                match query_kind {
                    WebhookQueryKind::Events => {
                        let query = WebhookEventQuery {
                            source: normalize_filter(&source_filter),
                            event_type: normalize_filter(&event_type_filter),
                            session_key: normalize_filter(&session_filter),
                            status: parse_status_filter(&status_filter),
                            received_from_ms: start_date.and_then(date_start_ms),
                            received_to_ms: end_date.and_then(date_end_ms),
                            limit: size,
                            offset,
                            sort_order,
                        };
                        manager
                            .list_webhook_events(&query)
                            .await
                            .map(|rows| rows.into_iter().map(WebhookListRow::Event).collect())
                    }
                    WebhookQueryKind::Agents => {
                        let query = WebhookAgentQuery {
                            hook_id: normalize_filter(&event_type_filter),
                            session_key: normalize_filter(&session_filter),
                            status: parse_status_filter(&status_filter),
                            received_from_ms: start_date.and_then(date_start_ms),
                            received_to_ms: end_date.and_then(date_end_ms),
                            limit: size,
                            offset,
                            sort_order,
                        };
                        manager
                            .list_webhook_agents(&query)
                            .await
                            .map(|rows| rows.into_iter().map(WebhookListRow::Agent).collect())
                    }
                }
            });
            let _ = tx.send(result);
        });
        self.rows_request = Some(PendingWebhookRowsRequest { receiver: rx });
    }

    fn poll_rows_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.rows_request.take() else {
            return;
        };

        match request.receiver.try_recv() {
            Ok(result) => match result {
                Ok(rows) => {
                    self.rows = rows;
                    self.loaded = true;
                    if self.rows_refresh_queued {
                        self.rows_refresh_queued = false;
                        self.refresh(notifications);
                    }
                }
                Err(err) => {
                    notifications.error(format!("Failed to load webhook rows: {err}"));
                    if self.rows_refresh_queued {
                        self.rows_refresh_queued = false;
                        self.refresh(notifications);
                    }
                }
            },
            Err(TryRecvError::Empty) => {
                self.rows_request = Some(request);
            }
            Err(TryRecvError::Disconnected) => {
                notifications.error("Webhook rows worker closed unexpectedly");
            }
        }
    }

    fn refresh_gateway_status(&mut self) {
        if self.gateway_status_request.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(request_gateway_status());
        });
        self.gateway_status_request = Some(rx);
    }

    fn poll_gateway_status(&mut self, notifications: &mut NotificationCenter) {
        let Some(receiver) = self.gateway_status_request.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(result) => match result {
                Ok(status) => {
                    self.gateway_status = Some(status);
                }
                Err(err) => {
                    notifications.error(format!("Failed to load gateway status: {err}"));
                }
            },
            Err(TryRecvError::Empty) => {
                self.gateway_status_request = Some(receiver);
            }
            Err(TryRecvError::Disconnected) => {
                notifications.error("Gateway status worker closed unexpectedly");
            }
        }
    }

    fn toggle_sort_order(&mut self) {
        self.sort_order = match self.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => WebhookEventSortOrder::ReceivedAtDesc,
            WebhookEventSortOrder::ReceivedAtDesc => WebhookEventSortOrder::ReceivedAtAsc,
        };
    }

    fn sort_label(&self) -> &'static str {
        match self.sort_order {
            WebhookEventSortOrder::ReceivedAtAsc => "Time ↑",
            WebhookEventSortOrder::ReceivedAtDesc => "Time ↓",
        }
    }

    fn save_webhook_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };

        let config_form = self.config_form.clone();
        match store.update_config(|config| {
            config_form
                .apply_to_config(config)
                .map_err(ConfigError::InvalidConfig)?;
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                self.config_window_open = false;
                self.refresh_gateway_status();
                let running = self
                    .gateway_status
                    .as_ref()
                    .map(|status| status.running)
                    .unwrap_or(false);
                if running {
                    notifications
                        .success("Webhook config saved. Restart gateway to apply runtime changes.");
                } else {
                    notifications.success("Webhook config saved");
                }
            }
            Err(err) => notifications.error(format!("Save failed: {err}")),
        }
    }

    fn reload_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                self.refresh_gateway_status();
                notifications.success("Webhook config reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn open_create_prompt(&mut self) {
        self.create_prompt = CreatePromptState::default();
        self.create_prompt_open = true;
    }

    fn open_inspect_prompt(&mut self, notifications: &mut NotificationCenter) {
        self.inspect_prompt_open = true;
        self.refresh_prompt_templates(notifications);
    }

    fn refresh_prompt_templates(&mut self, notifications: &mut NotificationCenter) {
        let Some(prompt_dir) = self.prompt_dir.clone() else {
            self.inspect_prompt.templates.clear();
            self.inspect_prompt.error_message = Some(
                "Prompt directory is unavailable because the data root could not be resolved."
                    .to_string(),
            );
            return;
        };

        match list_prompt_templates_in_dir(&prompt_dir) {
            Ok(templates) => {
                self.inspect_prompt.templates = templates;
                self.inspect_prompt.error_message = if prompt_dir.exists() {
                    None
                } else {
                    Some(format!(
                        "Prompt directory does not exist yet: {}",
                        prompt_dir.display()
                    ))
                };
                if self
                    .inspect_prompt
                    .selected_hook_id
                    .as_ref()
                    .is_some_and(|selected| {
                        !self
                            .inspect_prompt
                            .templates
                            .iter()
                            .any(|item| &item.hook_id == selected)
                    })
                {
                    self.inspect_prompt.selected_hook_id = None;
                }
            }
            Err(err) => {
                self.inspect_prompt.templates.clear();
                self.inspect_prompt.error_message = Some(err.clone());
                notifications.error(err);
            }
        }
    }

    fn save_prompt_template(&mut self, notifications: &mut NotificationCenter) {
        self.create_prompt.status_message = None;
        self.create_prompt.error_message = None;

        let Some(prompt_dir) = self.prompt_dir.clone() else {
            self.create_prompt.error_message = Some(
                "Prompt directory is unavailable because the data root could not be resolved."
                    .to_string(),
            );
            return;
        };

        let hook_id = match normalize_hook_id(&self.create_prompt.hook_id) {
            Ok(hook_id) => hook_id,
            Err(err) => {
                self.create_prompt.error_message = Some(err);
                return;
            }
        };
        let path = prompt_template_path(&prompt_dir, &hook_id);
        let existed = path.exists();
        if let Err(err) = fs::create_dir_all(&prompt_dir) {
            self.create_prompt.error_message = Some(format!(
                "Failed to create prompt directory {}: {err}",
                prompt_dir.display()
            ));
            return;
        }
        if let Err(err) = fs::write(&path, self.create_prompt.markdown.as_bytes()) {
            self.create_prompt.error_message =
                Some(format!("Failed to save {}: {err}", path.display()));
            return;
        }

        self.create_prompt.status_message = Some(if existed {
            format!("Updated prompt template `{hook_id}`.")
        } else {
            format!("Saved prompt template `{hook_id}`.")
        });
        notifications.success(
            self.create_prompt
                .status_message
                .clone()
                .unwrap_or_else(|| "Prompt template saved".to_string()),
        );
        self.refresh_prompt_templates(notifications);
    }

    fn open_view_prompt(
        &mut self,
        template: &PromptTemplateRecord,
        notifications: &mut NotificationCenter,
    ) {
        match load_prompt_markdown(&template.path) {
            Ok(markdown) => {
                self.view_prompt = Some(ViewPromptState {
                    hook_id: template.hook_id.clone(),
                    markdown,
                });
            }
            Err(err) => notifications.error(err),
        }
    }

    fn open_trick_prompt(
        &mut self,
        template: &PromptTemplateRecord,
        notifications: &mut NotificationCenter,
    ) {
        let session_options = match load_session_options() {
            Ok(session_options) => session_options,
            Err(err) => {
                notifications.error(err.clone());
                self.trick_prompt = Some(TrickPromptState {
                    hook_id: template.hook_id.clone(),
                    error_message: Some(err),
                    provider: default_webhook_provider(&self.config),
                    model: default_webhook_model(
                        &self.config,
                        &default_webhook_provider(&self.config),
                    ),
                    ..TrickPromptState::default()
                });
                return;
            }
        };

        let provider = default_webhook_provider(&self.config);
        let model = default_webhook_model(&self.config, &provider);
        let base_session_key = session_options
            .first()
            .map(|item| item.base_session_key.clone())
            .unwrap_or_default();
        self.trick_prompt = Some(TrickPromptState {
            hook_id: template.hook_id.clone(),
            session_options,
            base_session_key,
            provider,
            model,
            generated_url: None,
            error_message: None,
        });
    }

    fn delete_prompt_template(
        &mut self,
        hook_id: &str,
        path: &Path,
        notifications: &mut NotificationCenter,
    ) {
        match fs::remove_file(path) {
            Ok(()) => {
                notifications.success(format!("Deleted prompt template `{hook_id}`."));
                self.refresh_prompt_templates(notifications);
                if self
                    .view_prompt
                    .as_ref()
                    .is_some_and(|state| state.hook_id == hook_id)
                {
                    self.view_prompt = None;
                }
                if self
                    .trick_prompt
                    .as_ref()
                    .is_some_and(|state| state.hook_id == hook_id)
                {
                    self.trick_prompt = None;
                }
                if self.inspect_prompt.selected_hook_id.as_deref() == Some(hook_id) {
                    self.inspect_prompt.selected_hook_id = None;
                }
            }
            Err(err) => notifications.error(format!("Failed to delete {}: {err}", path.display())),
        }
    }
}

impl PanelRenderer for WebhookPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);
        self.poll_rows_request(notifications);
        self.poll_gateway_status(notifications);
        if self.gateway_status_request.is_some() || self.rows_request.is_some() {
            ui.ctx().request_repaint();
        }

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_gateway_status();
                self.refresh(notifications);
            }
            if ui.button("Config").clicked() {
                self.config_form = WebhookConfigForm::from_config(&self.config.gateway.webhook);
                self.config_window_open = true;
            }
            if ui.button("Create Prompt").clicked() {
                self.open_create_prompt();
            }
            if ui.button("Inspect Prompt").clicked() {
                self.open_inspect_prompt(notifications);
            }
            ui.label(format!("Rows: {}", self.rows.len()));
        });

        ui.separator();
        render_webhook_config_summary(ui, &self.config, self.gateway_status.as_ref());
        ui.separator();
        let mut need_refresh = false;
        ui.horizontal(|ui| {
            ui.label("type");
            let events_selected = self.query_kind == WebhookQueryKind::Events;
            if ui.selectable_label(events_selected, "Events").clicked() && !events_selected {
                self.query_kind = WebhookQueryKind::Events;
                self.page = 1;
                self.selected_id = None;
                self.summary_popup = None;
                need_refresh = true;
            }
            let agents_selected = self.query_kind == WebhookQueryKind::Agents;
            if ui.selectable_label(agents_selected, "Agents").clicked() && !agents_selected {
                self.query_kind = WebhookQueryKind::Agents;
                self.page = 1;
                self.selected_id = None;
                self.summary_popup = None;
                need_refresh = true;
            }
        });
        ui.horizontal(|ui| {
            if self.query_kind == WebhookQueryKind::Events {
                ui.label("source");
                if ui
                    .add_sized(
                        [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                        egui::TextEdit::singleline(&mut self.source_filter),
                    )
                    .changed()
                {
                    need_refresh = true;
                }
            }
            ui.label(query_mode_primary_label(self.query_kind));
            if ui
                .add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.event_type_filter),
                )
                .changed()
            {
                need_refresh = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("session");
            if ui
                .add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.session_filter),
                )
                .changed()
            {
                need_refresh = true;
            }
            ui.label("status");
            if ui
                .add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.status_filter),
                )
                .changed()
            {
                need_refresh = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("start date");
            if render_date_picker(ui, &mut self.start_date, "webhook-start-date") {
                need_refresh = true;
            }
            ui.label("end date");
            if render_date_picker(ui, &mut self.end_date, "webhook-end-date") {
                need_refresh = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("page");
            if ui
                .add_sized(
                    [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut self.page).range(1..=i64::MAX),
                )
                .changed()
            {
                need_refresh = true;
            }
            ui.label("size");
            if ui
                .add_sized(
                    [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut self.size).range(1..=1000),
                )
                .changed()
            {
                need_refresh = true;
            }
        });
        if need_refresh {
            self.refresh(notifications);
        }

        ui.separator();
        let table_width = ui.available_width();
        let mut open_summary: Option<WebhookSummaryState> = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .max_width(table_width)
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                if self.rows.is_empty() {
                    ui.label("No webhook rows found.");
                    return;
                }
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(120.0))
                    .column(Column::auto().at_least(110.0))
                    .column(Column::auto().at_least(130.0))
                    .column(Column::auto().at_least(140.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(100.0))
                    .sense(egui::Sense::click())
                    .header(22.0, |mut header| {
                        header.col(|ui| {
                            if ui.button(self.sort_label()).clicked() {
                                self.toggle_sort_order();
                                self.refresh(notifications);
                            }
                        });
                        header.col(|ui| {
                            ui.strong(if self.query_kind == WebhookQueryKind::Events {
                                "Source"
                            } else {
                                "Hook ID"
                            });
                        });
                        header.col(|ui| {
                            ui.strong(if self.query_kind == WebhookQueryKind::Events {
                                "Event Type"
                            } else {
                                "Session"
                            });
                        });
                        header.col(|ui| {
                            ui.strong(if self.query_kind == WebhookQueryKind::Events {
                                "Session"
                            } else {
                                "Status"
                            });
                        });
                        header.col(|ui| {
                            ui.strong(if self.query_kind == WebhookQueryKind::Events {
                                "Status"
                            } else {
                                "Sender"
                            });
                        });
                        header.col(|ui| {
                            ui.strong(if self.query_kind == WebhookQueryKind::Events {
                                "Sender"
                            } else {
                                "Sender"
                            });
                        });
                    })
                    .body(|body| {
                        body.rows(22.0, self.rows.len(), |mut row| {
                            let item = &self.rows[row.index()];
                            let is_selected = self.selected_id.as_deref() == Some(item.id());
                            row.set_selected(is_selected);
                            row.col(|ui| {
                                ui.label(format_timestamp_millis(item.received_at_ms()));
                            });
                            row.col(|ui| {
                                match item {
                                    WebhookListRow::Event(record) => ui.label(&record.source),
                                    WebhookListRow::Agent(record) => ui.label(&record.hook_id),
                                };
                            });
                            row.col(|ui| {
                                match item {
                                    WebhookListRow::Event(record) => ui.label(&record.event_type),
                                    WebhookListRow::Agent(record) => ui.label(&record.session_key),
                                };
                            });
                            row.col(|ui| {
                                match item {
                                    WebhookListRow::Event(record) => ui.label(&record.session_key),
                                    WebhookListRow::Agent(record) => {
                                        ui.label(record.status.as_str())
                                    }
                                };
                            });
                            row.col(|ui| {
                                match item {
                                    WebhookListRow::Event(record) => {
                                        ui.label(record.status.as_str())
                                    }
                                    WebhookListRow::Agent(record) => ui.label(&record.sender_id),
                                };
                            });
                            row.col(|ui| {
                                match item {
                                    WebhookListRow::Event(record) => ui.label(&record.sender_id),
                                    WebhookListRow::Agent(record) => ui.label(&record.sender_id),
                                };
                            });
                            let response = row.response();
                            if response.clicked() {
                                self.selected_id = if is_selected {
                                    None
                                } else {
                                    Some(item.id().to_string())
                                };
                            }
                            if response.double_clicked() {
                                open_summary = webhook_summary_state(item);
                            }
                        });
                    });
            });

        if let Some(summary) = open_summary {
            self.summary_popup = Some(summary);
        }
        if let Some(summary_state) = &mut self.summary_popup {
            let mut open = true;
            egui::Window::new(&summary_state.title)
                .id(egui::Id::new((
                    "webhook-summary-popup",
                    &summary_state.title,
                )))
                .open(&mut open)
                .resizable(true)
                .default_width(720.0)
                .default_height(360.0)
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("webhook-summary-scroll", &summary_state.title))
                        .max_height(SUMMARY_WINDOW_HEIGHT)
                        .show(ui, |ui| {
                            ui.add_sized(
                                [ui.available_width(), SUMMARY_WINDOW_HEIGHT],
                                egui::TextEdit::multiline(&mut summary_state.summary)
                                    .desired_width(f32::INFINITY)
                                    .interactive(false),
                            );
                        });
                });
            if !open {
                self.summary_popup = None;
            }
        }

        if self.config_window_open {
            let mut open = self.config_window_open;
            egui::Window::new("Webhook Config")
                .id(egui::Id::new("webhook-config-window"))
                .open(&mut open)
                .resizable(true)
                .default_width(520.0)
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Enabled");
                        ui.checkbox(&mut self.config_form.enabled, "");
                    });

                    ui.separator();
                    ui.strong("Events Endpoint");
                    ui.horizontal(|ui| {
                        ui.label("Enabled");
                        ui.checkbox(&mut self.config_form.events_enabled, "");
                    });

                    ui.horizontal(|ui| {
                        ui.label("Path");
                        ui.monospace(WEBHOOK_EVENTS_PATH);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Max Body Bytes");
                        ui.add_sized(
                            [160.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.config_form.events_max_body_bytes),
                        );
                    });

                    ui.separator();
                    ui.strong("Agents Endpoint");
                    ui.horizontal(|ui| {
                        ui.label("Enabled");
                        ui.checkbox(&mut self.config_form.agents_enabled, "");
                    });

                    ui.horizontal(|ui| {
                        ui.label("Path");
                        ui.monospace(WEBHOOK_AGENTS_PATH);
                    });

                    ui.horizontal(|ui| {
                        ui.label("Max Body Bytes");
                        ui.add_sized(
                            [160.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.config_form.agents_max_body_bytes),
                        );
                    });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Reload").clicked() {
                            self.reload_config(notifications);
                        }
                        if ui.button("Save").clicked() {
                            self.save_webhook_config(notifications);
                        }
                    });
                });
            self.config_window_open = open;
        }

        if self.create_prompt_open {
            let mut open = self.create_prompt_open;
            egui::Window::new("Create Prompt")
                .id(egui::Id::new("webhook-create-prompt"))
                .open(&mut open)
                .resizable(true)
                .default_width(920.0)
                .default_height(620.0)
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Hook ID");
                        ui.add_sized(
                            [280.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.create_prompt.hook_id),
                        );
                        if let Some(prompt_dir) = &self.prompt_dir {
                            ui.label(format!("Save To: {}", prompt_dir.display()));
                        }
                    });
                    if let Some(message) = &self.create_prompt.status_message {
                        ui.colored_label(Color32::from_rgb(0x22, 0xC5, 0x5E), message);
                    }
                    if let Some(message) = &self.create_prompt.error_message {
                        ui.colored_label(ui.visuals().error_fg_color, message);
                    }
                    ui.separator();
                    ui.columns(2, |columns| {
                        columns[0].label("Markdown");
                        columns[0].add_sized(
                            [columns[0].available_width(), PROMPT_TEXT_HEIGHT],
                            egui::TextEdit::multiline(&mut self.create_prompt.markdown)
                                .desired_width(f32::INFINITY),
                        );
                        columns[1].label("Preview");
                        egui::ScrollArea::vertical()
                            .id_salt("webhook-create-prompt-preview")
                            .max_height(PREVIEW_HEIGHT)
                            .show(&mut columns[1], |ui| {
                                render_markdown(ui, &self.create_prompt.markdown);
                            });
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.save_prompt_template(notifications);
                        }
                    });
                });
            self.create_prompt_open = open;
        }

        if self.inspect_prompt_open {
            let mut open = self.inspect_prompt_open;
            let mut open_view: Option<PromptTemplateRecord> = None;
            let mut open_trick: Option<PromptTemplateRecord> = None;
            let mut confirm_delete: Option<DeletePromptState> = None;
            egui::Window::new("Inspect Prompt")
                .id(egui::Id::new("webhook-inspect-prompt"))
                .open(&mut open)
                .resizable(true)
                .default_width(760.0)
                .default_height(480.0)
                .show(ui.ctx(), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Reload").clicked() {
                            self.refresh_prompt_templates(notifications);
                        }
                        ui.label(format!(
                            "Templates: {}",
                            self.inspect_prompt.templates.len()
                        ));
                    });
                    if let Some(prompt_dir) = &self.prompt_dir {
                        ui.label(format!("Directory: {}", prompt_dir.display()));
                    }
                    if let Some(message) = &self.inspect_prompt.error_message {
                        ui.colored_label(ui.visuals().error_fg_color, message);
                    }
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("webhook-prompt-list-scroll")
                        .max_height(PROMPT_LIST_HEIGHT)
                        .show(ui, |ui| {
                            if self.inspect_prompt.templates.is_empty() {
                                ui.label("No prompt templates found.");
                                return;
                            }
                            TableBuilder::new(ui)
                                .striped(true)
                                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                .column(Column::remainder().at_least(180.0))
                                .column(Column::remainder().at_least(320.0))
                                .sense(egui::Sense::click())
                                .header(20.0, |mut header| {
                                    header.col(|ui| {
                                        ui.strong("Hook ID");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Path");
                                    });
                                })
                                .body(|body| {
                                    body.rows(
                                        22.0,
                                        self.inspect_prompt.templates.len(),
                                        |mut row| {
                                            let item = &self.inspect_prompt.templates[row.index()];
                                            let is_selected =
                                                self.inspect_prompt.selected_hook_id.as_deref()
                                                    == Some(item.hook_id.as_str());
                                            row.set_selected(is_selected);
                                            row.col(|ui| {
                                                ui.label(&item.hook_id);
                                            });
                                            row.col(|ui| {
                                                ui.label(item.path.display().to_string());
                                            });
                                            let response = row.response();
                                            if response.clicked() {
                                                self.inspect_prompt.selected_hook_id =
                                                    if is_selected {
                                                        None
                                                    } else {
                                                        Some(item.hook_id.clone())
                                                    };
                                            }
                                            if response.double_clicked() {
                                                open_view = Some(item.clone());
                                            }
                                            let item_for_menu = item.clone();
                                            response.context_menu(|ui| {
                                                if ui
                                                    .button(format!("{} View", regular::EYE))
                                                    .clicked()
                                                {
                                                    open_view = Some(item_for_menu.clone());
                                                    ui.close();
                                                }
                                                let trick_enabled = trick_ready_error(
                                                    &self.config,
                                                    self.gateway_status.as_ref(),
                                                )
                                                .is_none();
                                                if ui
                                                    .add_enabled(
                                                        trick_enabled,
                                                        egui::Button::new(format!(
                                                            "{} Trick",
                                                            regular::MAGIC_WAND
                                                        )),
                                                    )
                                                    .clicked()
                                                {
                                                    open_trick = Some(item_for_menu.clone());
                                                    ui.close();
                                                }
                                                ui.separator();
                                                if ui
                                                    .add(egui::Button::new(
                                                        RichText::new(format!(
                                                            "{} Delete",
                                                            regular::TRASH
                                                        ))
                                                        .color(ui.visuals().warn_fg_color),
                                                    ))
                                                    .clicked()
                                                {
                                                    confirm_delete = Some(DeletePromptState {
                                                        hook_id: item_for_menu.hook_id.clone(),
                                                        path: item_for_menu.path.clone(),
                                                    });
                                                    ui.close();
                                                }
                                            });
                                        },
                                    );
                                });
                        });
                });
            self.inspect_prompt_open = open;
            if let Some(item) = open_view {
                self.open_view_prompt(&item, notifications);
            }
            if let Some(item) = open_trick {
                self.open_trick_prompt(&item, notifications);
            }
            if let Some(item) = confirm_delete {
                self.delete_prompt = Some(item);
            }
        }

        if let Some(view_state) = &mut self.view_prompt {
            let mut open = true;
            egui::Window::new(format!("View Prompt: {}", view_state.hook_id))
                .id(egui::Id::new(("webhook-view-prompt", &view_state.hook_id)))
                .open(&mut open)
                .resizable(true)
                .default_width(920.0)
                .default_height(620.0)
                .show(ui.ctx(), |ui| {
                    ui.columns(2, |columns| {
                        columns[0].label("Markdown");
                        columns[0].add_sized(
                            [columns[0].available_width(), PROMPT_TEXT_HEIGHT],
                            egui::TextEdit::multiline(&mut view_state.markdown)
                                .desired_width(f32::INFINITY)
                                .interactive(false),
                        );
                        columns[1].label("Preview");
                        egui::ScrollArea::vertical()
                            .id_salt(("webhook-view-prompt-preview", &view_state.hook_id))
                            .max_height(PREVIEW_HEIGHT)
                            .show(&mut columns[1], |ui| {
                                render_markdown(ui, &view_state.markdown);
                            });
                    });
                });
            if !open {
                self.view_prompt = None;
            }
        }

        if let Some(delete_state) = self.delete_prompt.clone() {
            let mut confirmed = false;
            let mut cancelled = false;
            egui::Window::new("Delete Prompt")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "Delete prompt template '{}'?",
                            delete_state.hook_id
                        ))
                        .strong(),
                    );
                    ui.add_space(8.0);
                    ui.label(delete_state.path.display().to_string());
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
                self.delete_prompt = None;
                self.delete_prompt_template(
                    &delete_state.hook_id,
                    &delete_state.path,
                    notifications,
                );
            }
            if cancelled {
                self.delete_prompt = None;
            }
        }

        if let Some(trick_state) = &mut self.trick_prompt {
            let mut open = true;
            egui::Window::new(format!("Trick Prompt: {}", trick_state.hook_id))
                .id(egui::Id::new((
                    "webhook-trick-prompt",
                    &trick_state.hook_id,
                )))
                .open(&mut open)
                .resizable(true)
                .default_width(700.0)
                .default_height(420.0)
                .show(ui.ctx(), |ui| {
                    if let Some(message) =
                        trick_ready_error(&self.config, self.gateway_status.as_ref())
                    {
                        ui.colored_label(ui.visuals().error_fg_color, message);
                    }
                    if let Some(message) = &trick_state.error_message {
                        ui.colored_label(ui.visuals().error_fg_color, message);
                    }
                    egui::Grid::new("webhook-trick-grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Hook ID");
                            ui.label(&trick_state.hook_id);
                            ui.end_row();

                            ui.label("Base Session");
                            egui::ComboBox::from_id_salt((
                                "webhook-trick-session",
                                &trick_state.hook_id,
                            ))
                            .selected_text(
                                trick_state
                                    .session_options
                                    .iter()
                                    .find(|item| {
                                        item.base_session_key == trick_state.base_session_key
                                    })
                                    .map(|item| item.label.as_str())
                                    .unwrap_or("Select a base session"),
                            )
                            .width(420.0)
                            .show_ui(ui, |ui| {
                                for option in &trick_state.session_options {
                                    let selected =
                                        trick_state.base_session_key == option.base_session_key;
                                    if ui.selectable_label(selected, &option.label).clicked() {
                                        trick_state.base_session_key =
                                            option.base_session_key.clone();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Provider");
                            egui::ComboBox::from_id_salt((
                                "webhook-trick-provider",
                                &trick_state.hook_id,
                            ))
                            .selected_text(if trick_state.provider.is_empty() {
                                "Select a provider"
                            } else {
                                trick_state.provider.as_str()
                            })
                            .width(320.0)
                            .show_ui(ui, |ui| {
                                for provider_id in self.config.model_providers.keys() {
                                    let selected = trick_state.provider == *provider_id;
                                    if ui.selectable_label(selected, provider_id).clicked() {
                                        trick_state.provider = provider_id.clone();
                                        trick_state.model =
                                            default_webhook_model(&self.config, provider_id);
                                    }
                                }
                            });
                            ui.end_row();

                            ui.label("Model");
                            ui.add_sized(
                                [420.0, ui.spacing().interact_size.y],
                                egui::TextEdit::singleline(&mut trick_state.model),
                            );
                            ui.end_row();
                        });
                    if trick_state.session_options.is_empty() {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "No base sessions found. Trick requires an existing IM base session.",
                        );
                    }
                    ui.add_space(8.0);
                    let generate_enabled = trick_state.error_message.is_none()
                        && trick_ready_error(&self.config, self.gateway_status.as_ref()).is_none()
                        && !trick_state.base_session_key.trim().is_empty()
                        && !trick_state.provider.trim().is_empty();
                    if ui
                        .add_enabled(generate_enabled, egui::Button::new("Generate"))
                        .clicked()
                    {
                        trick_state.generated_url = None;
                        trick_state.error_message = None;
                        match build_trick_url(
                            &self.config,
                            self.gateway_status.as_ref(),
                            trick_state,
                        ) {
                            Ok(url) => trick_state.generated_url = Some(url),
                            Err(err) => trick_state.error_message = Some(err),
                        }
                    }
                    if let Some(url) = &trick_state.generated_url {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("Webhook URL");
                            if ui.button(format!("{} Copy URL", regular::COPY)).clicked() {
                                ui.ctx().output_mut(|output| {
                                    output
                                        .commands
                                        .push(egui::OutputCommand::CopyText(url.clone()));
                                });
                                notifications.success("Webhook URL copied to clipboard");
                            }
                        });
                        let mut copy = url.clone();
                        ui.add(
                            egui::TextEdit::multiline(&mut copy)
                                .desired_width(f32::INFINITY)
                                .desired_rows(3)
                                .interactive(false),
                        );
                    }
                });
            if !open {
                self.trick_prompt = None;
            }
        }
    }
}

fn render_webhook_config_summary(
    ui: &mut egui::Ui,
    config: &AppConfig,
    _gateway_status: Option<&GatewayStatusSnapshot>,
) {
    let webhook = &config.gateway.webhook;

    egui::Grid::new("webhook-config-summary-grid")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            ui.label("Webhook Enabled");
            render_boolean_status(ui, webhook.enabled);
            ui.end_row();

            ui.label("Events Enabled");
            render_boolean_status(ui, webhook.events.enabled);
            ui.end_row();

            ui.label("Events Path");
            ui.monospace(WEBHOOK_EVENTS_PATH);
            ui.end_row();

            ui.label("Agents Enabled");
            render_boolean_status(ui, webhook.agents.enabled);
            ui.end_row();

            ui.label("Agents Path");
            ui.monospace(WEBHOOK_AGENTS_PATH);
            ui.end_row();
        });
}

fn webhook_summary_state(item: &WebhookListRow) -> Option<WebhookSummaryState> {
    match item {
        WebhookListRow::Event(record) => record
            .response_summary
            .as_ref()
            .filter(|summary| !summary.trim().is_empty())
            .map(|summary| WebhookSummaryState {
                title: format!("Response Summary: {}", record.id),
                summary: summary.clone(),
            }),
        WebhookListRow::Agent(record) => record
            .response_summary
            .as_ref()
            .filter(|summary| !summary.trim().is_empty())
            .map(|summary| WebhookSummaryState {
                title: format!("Response Summary: {}", record.hook_id),
                summary: summary.clone(),
            }),
    }
}

fn normalize_filter(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn query_mode_primary_label(kind: WebhookQueryKind) -> &'static str {
    match kind {
        WebhookQueryKind::Events => "event type",
        WebhookQueryKind::Agents => "hook id",
    }
}

fn render_boolean_status(ui: &mut egui::Ui, enabled: bool) {
    let (icon, color, label) = if enabled {
        (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            "Enabled",
        )
    } else {
        (regular::X_CIRCLE, ui.visuals().error_fg_color, "Disabled")
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, icon);
        ui.colored_label(color, label);
    });
}

fn gateway_base_url(ws_url: &str) -> String {
    ws_url
        .strip_suffix("/ws/chat")
        .unwrap_or(ws_url)
        .to_string()
}

fn prompt_templates_dir_from_config(config: &AppConfig) -> Option<PathBuf> {
    resolve_prompt_root_dir(config).map(|root| root.join("hooks").join("prompts"))
}

fn resolve_prompt_root_dir(config: &AppConfig) -> Option<PathBuf> {
    config
        .storage
        .root_dir
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(default_data_dir)
}

fn prompt_template_path(prompt_dir: &Path, hook_id: &str) -> PathBuf {
    prompt_dir.join(format!("{hook_id}.md"))
}

fn list_prompt_templates_in_dir(prompt_dir: &Path) -> Result<Vec<PromptTemplateRecord>, String> {
    if !prompt_dir.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(prompt_dir)
        .map_err(|err| format!("Failed to read {}: {err}", prompt_dir.display()))?;
    let mut templates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            format!(
                "Failed to read an entry from {}: {err}",
                prompt_dir.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("Failed to inspect {}: {err}", entry.path().display()))?;
        if !file_type.is_file() || !is_markdown_path(&path) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        templates.push(PromptTemplateRecord {
            hook_id: stem.to_string(),
            path,
        });
    }
    templates.sort_by(|a, b| a.hook_id.cmp(&b.hook_id));
    Ok(templates)
}

fn load_prompt_markdown(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("Failed to read {}: {err}", path.display()))
}

fn load_session_options() -> Result<Vec<TrickSessionOption>, String> {
    run_session_task(move |manager| async move {
        let sessions = manager.list_sessions(SessionListQuery::default()).await?;
        let active_session_keys = sessions
            .iter()
            .filter_map(|session| session.active_session_key.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut session_options = sessions
            .into_iter()
            .filter(|session| session.channel != "webhook")
            .filter(|session| !active_session_keys.contains(&session.session_key))
            .map(|session| TrickSessionOption {
                label: format!(
                    "{} ({}, {})",
                    session.session_key, session.channel, session.chat_id
                ),
                base_session_key: session.session_key,
            })
            .collect::<Vec<_>>();
        session_options.sort_by(|a, b| a.label.cmp(&b.label));
        session_options.dedup_by(|a, b| a.base_session_key == b.base_session_key);
        Ok(session_options)
    })
}

fn default_webhook_provider(config: &AppConfig) -> String {
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

fn default_webhook_model(config: &AppConfig, provider: &str) -> String {
    config
        .model_providers
        .get(provider)
        .map(|provider| provider.default_model.trim().to_string())
        .filter(|model| !model.is_empty())
        .unwrap_or_default()
}

fn normalize_hook_id(raw: &str) -> Result<String, String> {
    let hook_id = raw.trim();
    if hook_id.is_empty() {
        return Err("hook_id cannot be empty".to_string());
    }
    if hook_id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Ok(hook_id.to_string());
    }
    Err("hook_id may only contain letters, numbers, `_`, `-`, and `.`".to_string())
}

fn trick_ready_error(
    config: &AppConfig,
    gateway_status: Option<&GatewayStatusSnapshot>,
) -> Option<String> {
    if !config.gateway.webhook.enabled {
        return Some("Webhook is disabled in config.".to_string());
    }
    if !config.gateway.webhook.agents.enabled {
        return Some("Agents webhook endpoint is disabled in config.".to_string());
    }
    let status = gateway_status?;
    if !status.running {
        return Some("Gateway is not running.".to_string());
    }
    if status.info.is_none() {
        return Some("Gateway runtime info is unavailable.".to_string());
    }
    None
}

fn build_trick_url(
    config: &AppConfig,
    gateway_status: Option<&GatewayStatusSnapshot>,
    trick: &TrickPromptState,
) -> Result<String, String> {
    if let Some(err) = trick_ready_error(config, gateway_status) {
        return Err(err);
    }
    let hook_id = normalize_hook_id(&trick.hook_id)?;
    let base_session_key = trick.base_session_key.trim();
    if base_session_key.is_empty() {
        return Err("base_session_key is required".to_string());
    }
    let provider = trick.provider.trim();
    if provider.is_empty() {
        return Err("provider is required".to_string());
    }
    let model = trick.model.trim();
    let base = trick_base_url(gateway_status.expect("checked by trick_ready_error"))?;
    let mut url = format!("{base}{WEBHOOK_AGENTS_PATH}");
    let mut query = vec![
        ("hook_id", percent_encode_query_value(&hook_id)),
        (
            "base_session_key",
            percent_encode_query_value(base_session_key),
        ),
    ];
    query.push(("provider", percent_encode_query_value(provider)));
    if !model.is_empty() {
        query.push(("model", percent_encode_query_value(model)));
    }
    url.push('?');
    url.push_str(
        &query
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&"),
    );
    Ok(url)
}

fn trick_base_url(gateway_status: &GatewayStatusSnapshot) -> Result<String, String> {
    let info = gateway_status
        .info
        .as_ref()
        .ok_or_else(|| "Gateway runtime info is unavailable.".to_string())?;
    if gateway_status.tailscale_mode == TailscaleMode::Funnel
        && let Some(public_url) = info
            .tailscale
            .as_ref()
            .and_then(|tailscale| tailscale.public_url.as_deref())
            .filter(|value| !value.is_empty())
    {
        return Ok(public_url.trim_end_matches('/').to_string());
    }
    Ok(gateway_base_url(&info.ws_url))
}

fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn render_markdown(ui: &mut egui::Ui, markdown: &str) {
    let mut in_code_block = false;
    let mut code_block = String::new();

    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                ui.add_sized(
                    [ui.available_width(), 220.0],
                    egui::TextEdit::multiline(&mut code_block)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace)
                        .interactive(false),
                );
                code_block.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_block.push_str(line);
            code_block.push('\n');
            continue;
        }

        if let Some(text) = line.strip_prefix("# ") {
            ui.heading(text);
            continue;
        }
        if let Some(text) = line.strip_prefix("## ") {
            ui.add_space(6.0);
            ui.strong(text);
            continue;
        }
        if let Some(text) = line.strip_prefix("### ") {
            ui.label(RichText::new(text).strong());
            continue;
        }
        if let Some(text) = line.strip_prefix("- ") {
            ui.label(format!("• {text}"));
            continue;
        }
        if line.trim().is_empty() {
            ui.add_space(4.0);
            continue;
        }
        ui.label(line);
    }
}

fn parse_status_filter(raw: &str) -> Option<WebhookEventStatus> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        WebhookEventStatus::parse(trimmed)
    }
}

fn render_date_picker(ui: &mut egui::Ui, value: &mut Option<NaiveDate>, id: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if let Some(date) = value.as_mut() {
            if ui
                .add(DatePickerButton::new(date).id_salt(id).format("%Y/%m/%d"))
                .changed()
            {
                changed = true;
            }
            if ui.small_button("×").clicked() {
                *value = None;
                changed = true;
            }
        }
    });
    changed
}

fn date_start_ms(date: NaiveDate) -> Option<i64> {
    date_boundary_ms(date, Time::MIDNIGHT)
}

fn date_end_ms(date: NaiveDate) -> Option<i64> {
    let time = Time::from_hms_milli(23, 59, 59, 999).ok()?;
    date_boundary_ms(date, time)
}

fn date_boundary_ms(date: NaiveDate, time: Time) -> Option<i64> {
    let month = Month::try_from(date.month() as u8).ok()?;
    let date = time::Date::from_calendar_date(date.year(), month, date.day() as u8).ok()?;
    let datetime = PrimitiveDateTime::new(date, time).assume_utc();
    Some(offset_to_ms(datetime))
}

fn offset_to_ms(datetime: OffsetDateTime) -> i64 {
    datetime.unix_timestamp_nanos().saturating_div(1_000_000) as i64
}

fn run_session_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Box<dyn SessionManager>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, SessionError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let manager: Box<dyn SessionManager> = Box::new(
                SqliteSessionManager::open_default()
                    .await
                    .map_err(|err| format!("failed to open session manager: {err}"))?,
            );
            op(manager)
                .await
                .map_err(|err| format!("session operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("session operation thread panicked".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PromptTemplateRecord, TrickPromptState, TrickSessionOption, WebhookPanel, WebhookQueryKind,
        build_trick_url, default_webhook_model, list_prompt_templates_in_dir, normalize_hook_id,
        percent_encode_query_value, prompt_templates_dir_from_config, query_mode_primary_label,
        trick_base_url,
    };
    use crate::GatewayStatusSnapshot;
    use klaw_config::{AppConfig, TailscaleMode};
    use klaw_gateway::{GatewayRuntimeInfo, TailscaleRuntimeInfo, TailscaleStatus};
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn prompt_dir_uses_storage_root_when_configured() {
        let mut config = AppConfig::default();
        config.storage.root_dir = Some("/tmp/klaw-test-root".to_string());
        assert_eq!(
            prompt_templates_dir_from_config(&config),
            Some(PathBuf::from("/tmp/klaw-test-root/hooks/prompts"))
        );
    }

    #[test]
    fn list_prompt_templates_filters_non_markdown_files() {
        let temp = std::env::temp_dir().join(format!("klaw-webhook-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp).expect("create temp dir");
        fs::write(temp.join("a.md"), "# A").expect("write markdown");
        fs::write(temp.join("b.txt"), "ignore").expect("write txt");
        fs::create_dir(temp.join("nested")).expect("create dir");

        let templates = list_prompt_templates_in_dir(&temp).expect("list templates");
        assert_eq!(
            templates,
            vec![PromptTemplateRecord {
                hook_id: "a".to_string(),
                path: temp.join("a.md"),
            }]
        );
        fs::remove_dir_all(&temp).expect("cleanup temp dir");
    }

    #[test]
    fn normalize_hook_id_rejects_unsafe_characters() {
        assert_eq!(
            normalize_hook_id("order_sync"),
            Ok("order_sync".to_string())
        );
        assert!(normalize_hook_id("../oops").is_err());
        assert!(normalize_hook_id("bad/name").is_err());
    }

    #[test]
    fn webhook_panel_defaults_to_events_query_mode() {
        assert_eq!(WebhookPanel::default().query_kind, WebhookQueryKind::Events);
    }

    #[test]
    fn agent_query_mode_uses_hook_id_label() {
        assert_eq!(
            query_mode_primary_label(WebhookQueryKind::Agents),
            "hook id"
        );
    }

    #[test]
    fn webhook_default_model_tracks_provider_default() {
        let mut config = AppConfig::default();
        config.model_providers.insert(
            "anthropic".to_string(),
            klaw_config::ModelProviderConfig {
                default_model: "claude-sonnet".to_string(),
                ..klaw_config::ModelProviderConfig::default()
            },
        );
        assert_eq!(default_webhook_model(&config, "anthropic"), "claude-sonnet");
    }

    #[test]
    fn trick_base_url_prefers_funnel_public_url() {
        let status = GatewayStatusSnapshot {
            running: true,
            tailscale_mode: TailscaleMode::Funnel,
            info: Some(GatewayRuntimeInfo {
                listen_ip: "127.0.0.1".to_string(),
                configured_port: 9000,
                actual_port: 9000,
                ws_url: "http://127.0.0.1:9000/ws/chat".to_string(),
                health_url: "http://127.0.0.1:9000/health/status".to_string(),
                metrics_url: "http://127.0.0.1:9000/metrics".to_string(),
                started_at_unix_seconds: 0,
                tailscale: Some(TailscaleRuntimeInfo {
                    mode: TailscaleMode::Funnel,
                    status: TailscaleStatus::Connected,
                    public_url: Some("https://demo.ts.net/".to_string()),
                    message: None,
                }),
                auth_configured: true,
            }),
            ..GatewayStatusSnapshot::default()
        };
        assert_eq!(
            trick_base_url(&status).expect("base url"),
            "https://demo.ts.net"
        );
    }

    #[test]
    fn trick_url_falls_back_to_gateway_base_url() {
        let mut config = AppConfig::default();
        config.gateway.webhook.enabled = true;
        config.gateway.webhook.agents.enabled = true;
        let status = GatewayStatusSnapshot {
            running: true,
            tailscale_mode: TailscaleMode::Off,
            info: Some(GatewayRuntimeInfo {
                listen_ip: "127.0.0.1".to_string(),
                configured_port: 9000,
                actual_port: 9000,
                ws_url: "http://127.0.0.1:9000/ws/chat".to_string(),
                health_url: "http://127.0.0.1:9000/health/status".to_string(),
                metrics_url: "http://127.0.0.1:9000/metrics".to_string(),
                started_at_unix_seconds: 0,
                tailscale: None,
                auth_configured: false,
            }),
            ..GatewayStatusSnapshot::default()
        };
        let trick = TrickPromptState {
            hook_id: "order_sync".to_string(),
            session_options: vec![TrickSessionOption {
                base_session_key: "session 1".to_string(),
                label: "session 1 (telegram, chat-1)".to_string(),
            }],
            base_session_key: "session 1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4.1-mini".to_string(),
            ..TrickPromptState::default()
        };
        let url = build_trick_url(&config, Some(&status), &trick).expect("trick url");
        assert_eq!(
            url,
            "http://127.0.0.1:9000/webhook/agents?hook_id=order_sync&base_session_key=session%201&provider=openai&model=gpt-4.1-mini"
        );
    }

    #[test]
    fn percent_encode_query_value_encodes_spaces_and_symbols() {
        assert_eq!(percent_encode_query_value("a b/c"), "a%20b%2Fc".to_string());
    }
}
