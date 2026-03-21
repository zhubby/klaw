use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use crate::widgets::show_json_tree;
use chrono::{Datelike, Local, NaiveDate};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use klaw_session::{
    LlmAuditQuery, LlmAuditRecord, LlmAuditSortOrder, SessionError, SessionManager,
    SqliteSessionManager,
};
use std::future::Future;
use std::thread;
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 110.0;

#[derive(Default)]
pub struct LlmPanel {
    loaded: bool,
    rows: Vec<LlmAuditRecord>,
    session_filter: String,
    provider_filter: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    limit_text: String,
    offset_text: String,
    sort_order: LlmAuditSortOrder,
    selected_id: Option<String>,
    detail_record: Option<LlmAuditRecord>,
}

impl LlmPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        if self.limit_text.is_empty() {
            self.limit_text = "100".to_string();
        }
        self.sort_order = LlmAuditSortOrder::RequestedAtDesc;
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let query = LlmAuditQuery {
            session_key: normalize_filter(&self.session_filter),
            provider: normalize_filter(&self.provider_filter),
            requested_from_ms: self.start_date.and_then(date_start_ms),
            requested_to_ms: self.end_date.and_then(date_end_ms),
            limit: self.limit_text.trim().parse::<i64>().unwrap_or(100),
            offset: self.offset_text.trim().parse::<i64>().unwrap_or(0),
            sort_order: self.sort_order,
        };
        match run_session_task(move |manager| async move { manager.list_llm_audit(&query).await }) {
            Ok(rows) => {
                self.rows = rows;
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to load LLM audit rows: {err}")),
        }
    }

    fn toggle_sort_order(&mut self) {
        self.sort_order = match self.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => LlmAuditSortOrder::RequestedAtDesc,
            LlmAuditSortOrder::RequestedAtDesc => LlmAuditSortOrder::RequestedAtAsc,
        };
    }

    fn sort_label(&self) -> &'static str {
        match self.sort_order {
            LlmAuditSortOrder::RequestedAtAsc => "Time ↑",
            LlmAuditSortOrder::RequestedAtDesc => "Time ↓",
        }
    }
}

impl PanelRenderer for LlmPanel {
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
        egui::Grid::new("llm-audit-filter-grid")
            .num_columns(4)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("session");
                ui.add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.session_filter),
                );
                ui.label("provider");
                ui.add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.provider_filter),
                );
                ui.end_row();

                ui.label("start date");
                render_date_picker(ui, &mut self.start_date, "llm-audit-start-date");
                ui.label("end date");
                render_date_picker(ui, &mut self.end_date, "llm-audit-end-date");
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
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() {
                self.refresh(notifications);
            }
        });

        ui.separator();
        let mut open_detail: Option<LlmAuditRecord> = None;
        let table_width = ui.available_width();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .max_width(table_width)
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                if self.rows.is_empty() {
                    ui.label("No LLM audit rows found.");
                    return;
                }

                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(120.0))
                    .column(Column::auto().at_least(120.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(40.0))
                    .column(Column::auto().at_least(40.0))
                    .column(Column::auto().at_least(60.0))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::remainder().at_least(100.0))
                    .sense(egui::Sense::click())
                    .header(22.0, |mut header| {
                        header.col(|ui| {
                            if ui.button(self.sort_label()).clicked() {
                                self.toggle_sort_order();
                                self.refresh(notifications);
                            }
                        });
                        header.col(|ui| {
                            ui.strong("Session");
                        });
                        header.col(|ui| {
                            ui.strong("Provider");
                        });
                        header.col(|ui| {
                            ui.strong("Model");
                        });
                        header.col(|ui| {
                            ui.strong("Wire API");
                        });
                        header.col(|ui| {
                            ui.strong("Turn");
                        });
                        header.col(|ui| {
                            ui.strong("Seq");
                        });
                        header.col(|ui| {
                            ui.strong("Status");
                        });
                        header.col(|ui| {
                            ui.strong("Req ID");
                        });
                        header.col(|ui| {
                            ui.strong("Resp ID");
                        });
                    })
                    .body(|body| {
                        body.rows(22.0, self.rows.len(), |mut row| {
                            let item = &self.rows[row.index()];
                            let is_selected = self.selected_id.as_deref() == Some(&item.id);
                            row.set_selected(is_selected);

                            row.col(|ui| {
                                ui.label(format_timestamp_millis(item.requested_at_ms));
                            });
                            row.col(|ui| {
                                ui.label(&item.session_key);
                            });
                            row.col(|ui| {
                                ui.label(&item.provider);
                            });
                            row.col(|ui| {
                                ui.label(&item.model);
                            });
                            row.col(|ui| {
                                ui.label(&item.wire_api);
                            });
                            row.col(|ui| {
                                ui.label(item.turn_index.to_string());
                            });
                            row.col(|ui| {
                                ui.label(item.request_seq.to_string());
                            });
                            row.col(|ui| {
                                ui.label(item.status.as_str());
                            });
                            row.col(|ui| {
                                ui.label(item.provider_request_id.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(item.provider_response_id.as_deref().unwrap_or(""));
                            });

                            let response = row.response();
                            if response.clicked() {
                                self.selected_id = if is_selected {
                                    None
                                } else {
                                    Some(item.id.clone())
                                };
                            }
                            response.context_menu(|ui| {
                                if ui.button("View Details").clicked() {
                                    open_detail = Some(item.clone());
                                    ui.close();
                                }
                                if ui.button("Copy Session Key").clicked() {
                                    ui.ctx().output_mut(|o| {
                                        o.commands.push(egui::OutputCommand::CopyText(
                                            item.session_key.clone(),
                                        ));
                                    });
                                    ui.close();
                                }
                                if ui.button("Copy Request ID").clicked() {
                                    ui.ctx().output_mut(|o| {
                                        o.commands.push(egui::OutputCommand::CopyText(
                                            item.provider_request_id.clone().unwrap_or_default(),
                                        ));
                                    });
                                    ui.close();
                                }
                            });
                        });
                    });
            });

        if let Some(record) = open_detail {
            self.detail_record = Some(record);
        }
        if let Some(record) = &mut self.detail_record {
            let mut open = true;
            egui::Window::new("LLM Audit Detail")
                .id(egui::Id::new("llm-audit-detail"))
                .open(&mut open)
                .resizable(true)
                .default_width(860.0)
                .default_height(640.0)
                .max_height(640.0)
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("llm-audit-detail-scroll")
                        .auto_shrink([false, false])
                        .max_height(560.0)
                        .show(ui, |ui| {
                            ui.label(format!("Session: {}", record.session_key));
                            ui.label(format!(
                                "Time: {}",
                                format_timestamp_millis(record.requested_at_ms)
                            ));
                            ui.label(format!("Provider: {}", record.provider));
                            ui.label(format!("Model: {}", record.model));
                            ui.label(format!("Wire API: {}", record.wire_api));
                            ui.label(format!("Status: {}", record.status.as_str()));
                            if let Some(error_code) = &record.error_code {
                                ui.label(format!("Error Code: {error_code}"));
                            }
                            if let Some(error_message) = &record.error_message {
                                ui.label(format!("Error Message: {error_message}"));
                            }
                            ui.separator();

                            ui.collapsing("Request Body", |ui| {
                                render_json_payload(ui, &record.request_body_json);
                            });
                            ui.separator();
                            ui.collapsing("Response Body", |ui| {
                                if let Some(body) = &record.response_body_json {
                                    render_json_payload(ui, body);
                                } else {
                                    ui.monospace("<empty>");
                                }
                            });
                        });
                });
            if !open {
                self.detail_record = None;
            }
        }
    }
}

fn render_json_payload(ui: &mut egui::Ui, raw: &str) {
    egui::ScrollArea::vertical()
        .id_salt(("llm-audit-json-scroll", raw.len()))
        .auto_shrink([false, false])
        .max_height(260.0)
        .show(ui, |ui| match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(value) => show_json_tree(ui, &value),
            Err(_) => {
                let mut text = raw.to_string();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(20)
                        .interactive(false),
                );
            }
        });
}

fn normalize_filter(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn render_date_picker(ui: &mut egui::Ui, value: &mut Option<NaiveDate>, id: &str) {
    ui.horizontal(|ui| {
        if let Some(date) = value.as_mut() {
            ui.add(DatePickerButton::new(date).id_salt(id).format("%Y/%m/%d"));
            if ui.small_button("Clear").clicked() {
                *value = None;
            }
        } else {
            if ui.button("Set Date").clicked() {
                *value = Some(Local::now().date_naive());
            }
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
