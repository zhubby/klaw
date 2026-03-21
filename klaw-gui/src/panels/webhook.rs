use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use crate::widgets::show_json_tree;
use chrono::{Datelike, Local, NaiveDate};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use klaw_session::{
    SessionError, SessionManager, SqliteSessionManager, WebhookEventQuery, WebhookEventRecord,
    WebhookEventSortOrder, WebhookEventStatus,
};
use std::future::Future;
use std::thread;
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 110.0;

#[derive(Default)]
pub struct WebhookPanel {
    loaded: bool,
    rows: Vec<WebhookEventRecord>,
    source_filter: String,
    event_type_filter: String,
    session_filter: String,
    status_filter: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    limit_text: String,
    offset_text: String,
    sort_order: WebhookEventSortOrder,
    selected_id: Option<String>,
    detail_record: Option<WebhookEventRecord>,
}

impl WebhookPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        if self.limit_text.is_empty() {
            self.limit_text = "100".to_string();
        }
        self.sort_order = WebhookEventSortOrder::ReceivedAtDesc;
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let query = WebhookEventQuery {
            source: normalize_filter(&self.source_filter),
            event_type: normalize_filter(&self.event_type_filter),
            session_key: normalize_filter(&self.session_filter),
            status: parse_status_filter(&self.status_filter),
            received_from_ms: self.start_date.and_then(date_start_ms),
            received_to_ms: self.end_date.and_then(date_end_ms),
            limit: self.limit_text.trim().parse::<i64>().unwrap_or(100),
            offset: self.offset_text.trim().parse::<i64>().unwrap_or(0),
            sort_order: self.sort_order,
        };
        match run_session_task(
            move |manager| async move { manager.list_webhook_events(&query).await },
        ) {
            Ok(rows) => {
                self.rows = rows;
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to load webhook rows: {err}")),
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
}

impl PanelRenderer for WebhookPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications);
            }
            ui.label(format!("Rows: {}", self.rows.len()));
        });

        ui.separator();
        egui::Grid::new("webhook-filter-grid")
            .num_columns(4)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("source");
                ui.add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.source_filter),
                );
                ui.label("event type");
                ui.add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.event_type_filter),
                );
                ui.end_row();

                ui.label("session");
                ui.add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.session_filter),
                );
                ui.label("status");
                ui.add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.status_filter),
                );
                ui.end_row();

                ui.label("start date");
                render_date_picker(ui, &mut self.start_date, "webhook-start-date");
                ui.label("end date");
                render_date_picker(ui, &mut self.end_date, "webhook-end-date");
                ui.end_row();

                ui.label("limit");
                ui.add_sized(
                    [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.limit_text),
                );
                ui.label("offset");
                ui.add_sized(
                    [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.offset_text),
                );
                ui.end_row();
            });
        if ui.button("Apply").clicked() {
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
    }
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

fn parse_status_filter(raw: &str) -> Option<WebhookEventStatus> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        WebhookEventStatus::parse(trimmed)
    }
}

fn render_date_picker(ui: &mut egui::Ui, value: &mut Option<NaiveDate>, id: &str) {
    ui.horizontal(|ui| {
        if let Some(date) = value.as_mut() {
            ui.add(DatePickerButton::new(date).id_salt(id).format("%Y/%m/%d"));
            if ui.small_button("Clear").clicked() {
                *value = None;
            }
        } else if ui.button("Set Date").clicked() {
            *value = Some(Local::now().date_naive());
        }
    });
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
