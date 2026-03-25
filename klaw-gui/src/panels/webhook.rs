use crate::GatewayStatusSnapshot;
use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge::request_gateway_status;
use crate::time_format::format_timestamp_millis;
use crate::widgets::show_json_tree;
use chrono::{Datelike, Local, NaiveDate};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use klaw_config::{AppConfig, ConfigError, ConfigSnapshot, ConfigStore, GatewayWebhookConfig};
use klaw_session::{
    SessionError, SessionManager, SqliteSessionManager, WebhookEventQuery, WebhookEventRecord,
    WebhookEventSortOrder, WebhookEventStatus,
};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 50.0;

struct PendingWebhookRowsRequest {
    receiver: Receiver<Result<Vec<WebhookEventRecord>, String>>,
}

#[derive(Debug, Clone, Default)]
struct WebhookConfigForm {
    enabled: bool,
    events_enabled: bool,
    events_path: String,
    events_max_body_bytes: String,
    agents_enabled: bool,
    agents_path: String,
    agents_max_body_bytes: String,
}

impl WebhookConfigForm {
    fn from_config(config: &GatewayWebhookConfig) -> Self {
        Self {
            enabled: config.enabled,
            events_enabled: config.events.enabled,
            events_path: config.events.path.clone(),
            events_max_body_bytes: config.events.max_body_bytes.to_string(),
            agents_enabled: config.agents.enabled,
            agents_path: config.agents.path.clone(),
            agents_max_body_bytes: config.agents.max_body_bytes.to_string(),
        }
    }

    fn apply_to_config(&self, config: &mut AppConfig) -> Result<(), String> {
        let events_path = self.events_path.trim();
        if events_path.is_empty() {
            return Err("events path cannot be empty".to_string());
        }
        let agents_path = self.agents_path.trim();
        if agents_path.is_empty() {
            return Err("agents path cannot be empty".to_string());
        }

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
        config.gateway.webhook.events.path = events_path.to_string();
        config.gateway.webhook.events.max_body_bytes = events_max_body_bytes;
        config.gateway.webhook.agents.enabled = self.agents_enabled;
        config.gateway.webhook.agents.path = agents_path.to_string();
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
    rows: Vec<WebhookEventRecord>,
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
    detail_record: Option<WebhookEventRecord>,
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
            detail_record: None,
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
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let _ = notifications;
        let size = self.size.max(1);
        let page = self.page.max(1);
        let offset = (page - 1) * size;
        let query = WebhookEventQuery {
            source: normalize_filter(&self.source_filter),
            event_type: normalize_filter(&self.event_type_filter),
            session_key: normalize_filter(&self.session_filter),
            status: parse_status_filter(&self.status_filter),
            received_from_ms: self.start_date.and_then(date_start_ms),
            received_to_ms: self.end_date.and_then(date_end_ms),
            limit: size,
            offset,
            sort_order: self.sort_order,
        };
        if self.rows_request.is_some() {
            self.rows_refresh_queued = true;
            return;
        }

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = run_session_task(move |manager| async move {
                manager.list_webhook_events(&query).await
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
            ui.label(format!("Rows: {}", self.rows.len()));
        });

        ui.separator();
        render_webhook_config_summary(
            ui,
            &self.config,
            self.config_path.as_deref(),
            self.gateway_status.as_ref(),
        );
        ui.separator();
        let mut need_refresh = false;
        ui.horizontal(|ui| {
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
            ui.label("event type");
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
        let mut open_detail: Option<WebhookEventRecord> = None;
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
                    .column(Column::auto().at_least(100.0))
                    .column(Column::auto().at_least(130.0))
                    .column(Column::auto().at_least(140.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::remainder().at_least(180.0))
                    .sense(egui::Sense::click())
                    .header(22.0, |mut header| {
                        header.col(|ui| {
                            if ui.button(self.sort_label()).clicked() {
                                self.toggle_sort_order();
                                self.refresh(notifications);
                            }
                        });
                        header.col(|ui| {
                            ui.strong("Source");
                        });
                        header.col(|ui| {
                            ui.strong("Event Type");
                        });
                        header.col(|ui| {
                            ui.strong("Session");
                        });
                        header.col(|ui| {
                            ui.strong("Status");
                        });
                        header.col(|ui| {
                            ui.strong("Sender");
                        });
                        header.col(|ui| {
                            ui.strong("Response Summary");
                        });
                    })
                    .body(|body| {
                        body.rows(22.0, self.rows.len(), |mut row| {
                            let item = &self.rows[row.index()];
                            let is_selected = self.selected_id.as_deref() == Some(&item.id);
                            row.set_selected(is_selected);
                            row.col(|ui| {
                                ui.label(format_timestamp_millis(item.received_at_ms));
                            });
                            row.col(|ui| {
                                ui.label(&item.source);
                            });
                            row.col(|ui| {
                                ui.label(&item.event_type);
                            });
                            row.col(|ui| {
                                ui.label(&item.session_key);
                            });
                            row.col(|ui| {
                                ui.label(item.status.as_str());
                            });
                            row.col(|ui| {
                                ui.label(&item.sender_id);
                            });
                            row.col(|ui| {
                                ui.label(item.response_summary.as_deref().unwrap_or(""));
                            });
                            let response = row.response();
                            if response.clicked() {
                                self.selected_id = if is_selected {
                                    None
                                } else {
                                    Some(item.id.clone())
                                };
                            }
                            if response.double_clicked() {
                                open_detail = Some(item.clone());
                            }
                        });
                    });
            });

        if let Some(record) = open_detail {
            self.detail_record = Some(record);
        }
        if let Some(record) = &mut self.detail_record {
            let mut open = true;
            egui::Window::new("Webhook Event Detail")
                .id(egui::Id::new("webhook-event-detail"))
                .open(&mut open)
                .resizable(true)
                .default_width(860.0)
                .default_height(520.0)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("ID: {}", record.id));
                    ui.label(format!(
                        "Time: {}",
                        format_timestamp_millis(record.received_at_ms)
                    ));
                    ui.label(format!("Source: {}", record.source));
                    ui.label(format!("Event Type: {}", record.event_type));
                    ui.label(format!("Session: {}", record.session_key));
                    ui.label(format!("Chat ID: {}", record.chat_id));
                    ui.label(format!("Sender: {}", record.sender_id));
                    ui.label(format!("Status: {}", record.status.as_str()));
                    if let Some(processed_at_ms) = record.processed_at_ms {
                        ui.label(format!(
                            "Processed At: {}",
                            format_timestamp_millis(processed_at_ms)
                        ));
                    }
                    if let Some(remote_addr) = &record.remote_addr {
                        ui.label(format!("Remote Addr: {remote_addr}"));
                    }
                    if let Some(error_message) = &record.error_message {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("Error: {error_message}"),
                        );
                    }
                    if let Some(summary) = &record.response_summary {
                        ui.label(format!("Response Summary: {summary}"));
                    }
                    ui.separator();
                    ui.strong("Payload");
                    render_json_payload(ui, record.payload_json.as_deref().unwrap_or("{}"));
                    ui.separator();
                    ui.strong("Metadata");
                    render_json_payload(ui, record.metadata_json.as_deref().unwrap_or("{}"));
                });
            if !open {
                self.detail_record = None;
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
                        ui.add_sized(
                            [320.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.config_form.events_path),
                        );
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
                        ui.add_sized(
                            [320.0, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.config_form.agents_path),
                        );
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
    }
}

fn render_webhook_config_summary(
    ui: &mut egui::Ui,
    config: &AppConfig,
    path: Option<&Path>,
    gateway_status: Option<&GatewayStatusSnapshot>,
) {
    let webhook = &config.gateway.webhook;
    let runtime_base_url = gateway_status.as_ref().and_then(|status| {
        status
            .info
            .as_ref()
            .map(|info| gateway_base_url(&info.ws_url))
    });
    let events_runtime_url = runtime_base_url
        .clone()
        .map(|base| format!("{base}{}", webhook.events.path));
    let agents_runtime_url = runtime_base_url
        .clone()
        .map(|base| format!("{base}{}", webhook.agents.path));
    let auth_configured = gateway_status
        .as_ref()
        .map(|status| status.auth_configured)
        .unwrap_or(false);

    egui::Grid::new("webhook-config-summary-grid")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            ui.label("Webhook Enabled");
            ui.label(if webhook.enabled {
                "enabled"
            } else {
                "disabled"
            });
            ui.end_row();

            ui.label("Events Enabled");
            ui.label(if webhook.events.enabled {
                "enabled"
            } else {
                "disabled"
            });
            ui.end_row();

            ui.label("Events Path");
            ui.label(&webhook.events.path);
            ui.end_row();

            ui.label("Events Runtime URL");
            if let Some(url) = events_runtime_url {
                ui.hyperlink(url);
            } else {
                ui.label("Gateway not running");
            }
            ui.end_row();

            ui.label("Events Max Body Bytes");
            ui.label(webhook.events.max_body_bytes.to_string());
            ui.end_row();

            ui.label("Agents Enabled");
            ui.label(if webhook.agents.enabled {
                "enabled"
            } else {
                "disabled"
            });
            ui.end_row();

            ui.label("Agents Path");
            ui.label(&webhook.agents.path);
            ui.end_row();

            ui.label("Agents Runtime URL");
            if let Some(url) = agents_runtime_url {
                ui.hyperlink(url);
            } else {
                ui.label("Gateway not running");
            }
            ui.end_row();

            ui.label("Agents Max Body Bytes");
            ui.label(webhook.agents.max_body_bytes.to_string());
            ui.end_row();

            ui.label("Auth Source");
            ui.label(webhook_auth_label(auth_configured));
            ui.end_row();

            if let Some(path) = path {
                ui.label("Config Path");
                ui.label(path.display().to_string());
                ui.end_row();
            }
        });
}

fn render_json_payload(ui: &mut egui::Ui, raw: &str) {
    egui::ScrollArea::both()
        .id_salt(("webhook-json-scroll", raw.len()))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            match serde_json::from_str::<serde_json::Value>(raw) {
                Ok(value) => show_json_tree(ui, &value),
                Err(_) => {
                    let mut text = raw.to_string();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(18)
                            .interactive(false),
                    );
                }
            }
        });
}

fn normalize_filter(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn webhook_auth_label(auth_configured: bool) -> &'static str {
    if auth_configured {
        "gateway auth"
    } else {
        "none"
    }
}

fn gateway_base_url(ws_url: &str) -> String {
    ws_url
        .strip_suffix("/ws/chat")
        .unwrap_or(ws_url)
        .to_string()
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
