use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use crate::widgets::show_json_tree_with_id;
use chrono::{Datelike, Local, NaiveDate};
use egui::Color32;
use egui_extras::{Column, DatePickerButton, TableBuilder};
use egui_phosphor::regular;
use klaw_session::{
    LlmAuditFilterOptionsQuery, LlmAuditQuery, LlmAuditRecord, LlmAuditSortOrder, LlmAuditStatus,
    SessionError, SessionManager, SqliteSessionManager,
};
use std::future::Future;
use std::thread;
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 50.0;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    #[default]
    Request,
    Response,
}

pub struct LlmPanel {
    loaded: bool,
    rows: Vec<LlmAuditRecord>,
    session_options: Vec<String>,
    provider_options: Vec<String>,
    session_filter: Option<String>,
    provider_filter: Option<String>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    page: i64,
    size: i64,
    sort_order: LlmAuditSortOrder,
    selected_id: Option<String>,
    detail_record: Option<LlmAuditRecord>,
    detail_tab: DetailTab,
}

impl Default for LlmPanel {
    fn default() -> Self {
        let today = Local::now().date_naive();
        let one_year_ago = today - chrono::Duration::days(365);
        Self {
            loaded: false,
            rows: Vec::new(),
            session_options: Vec::new(),
            provider_options: Vec::new(),
            session_filter: None,
            provider_filter: None,
            start_date: Some(one_year_ago),
            end_date: Some(today),
            page: 1,
            size: 100,
            sort_order: LlmAuditSortOrder::RequestedAtDesc,
            selected_id: None,
            detail_record: None,
            detail_tab: DetailTab::default(),
        }
    }
}

impl LlmPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let size = self.size.max(1);
        let page = self.page.max(1);
        let offset = (page - 1) * size;
        let filter_query = LlmAuditFilterOptionsQuery {
            requested_from_ms: self.start_date.and_then(date_start_ms),
            requested_to_ms: self.end_date.and_then(date_end_ms),
        };
        let query = LlmAuditQuery {
            session_key: self.session_filter.clone(),
            provider: self.provider_filter.clone(),
            requested_from_ms: filter_query.requested_from_ms,
            requested_to_ms: filter_query.requested_to_ms,
            limit: size,
            offset,
            sort_order: self.sort_order,
        };
        match run_session_task(move |manager| async move {
            let filter_options = manager.list_llm_audit_filter_options(&filter_query).await?;
            let rows = manager.list_llm_audit(&query).await?;
            Ok((filter_options, rows))
        }) {
            Ok((filter_options, rows)) => {
                self.session_options = filter_options.session_keys;
                self.provider_options = filter_options.providers;
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
        let mut need_refresh = false;
        ui.horizontal(|ui| {
            ui.label("session");
            let combo_resp = egui::ComboBox::from_id_salt("llm-audit-session-filter")
                .selected_text(self.session_filter.as_deref().unwrap_or("All"))
                .width(FILTER_INPUT_WIDTH)
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    if ui
                        .selectable_value(&mut self.session_filter, None, "All")
                        .changed()
                    {
                        changed = true;
                    }
                    for session_key in &self.session_options {
                        if ui
                            .selectable_value(
                                &mut self.session_filter,
                                Some(session_key.clone()),
                                session_key,
                            )
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    changed
                });
            if combo_resp.inner.unwrap_or(false) {
                need_refresh = true;
            }
            ui.label("provider");
            let combo_resp = egui::ComboBox::from_id_salt("llm-audit-provider-filter")
                .selected_text(self.provider_filter.as_deref().unwrap_or("All"))
                .width(FILTER_INPUT_WIDTH)
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    if ui
                        .selectable_value(&mut self.provider_filter, None, "All")
                        .changed()
                    {
                        changed = true;
                    }
                    for provider in &self.provider_options {
                        if ui
                            .selectable_value(
                                &mut self.provider_filter,
                                Some(provider.clone()),
                                provider,
                            )
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    changed
                });
            if combo_resp.inner.unwrap_or(false) {
                need_refresh = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label("start date");
            if render_date_picker(ui, &mut self.start_date, "llm-audit-start-date") {
                need_refresh = true;
            }
            ui.label("end date");
            if render_date_picker(ui, &mut self.end_date, "llm-audit-end-date") {
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
                                let (icon, color, text) = llm_status_display(item.status);
                                ui.label(
                                    egui::RichText::new(format!("{icon} {text}"))
                                        .color(color)
                                        .strong(),
                                );
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
                                if ui
                                    .button(format!("{} View Details", regular::EYE))
                                    .clicked()
                                {
                                    open_detail = Some(item.clone());
                                    ui.close();
                                }
                                if ui
                                    .button(format!("{} Copy Session Key", regular::KEY))
                                    .clicked()
                                {
                                    ui.ctx().output_mut(|o| {
                                        o.commands.push(egui::OutputCommand::CopyText(
                                            item.session_key.clone(),
                                        ));
                                    });
                                    ui.close();
                                }
                                if ui
                                    .button(format!("{} Copy Request ID", regular::FINGERPRINT))
                                    .clicked()
                                {
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
                .default_height(500.0)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Session: {}", record.session_key));
                    ui.label(format!(
                        "Time: {}",
                        format_timestamp_millis(record.requested_at_ms)
                    ));
                    ui.label(format!("Provider: {}", record.provider));
                    ui.label(format!("Model: {}", record.model));
                    ui.label(format!("Wire API: {}", record.wire_api));
                    let (icon, color, text) = llm_status_display(record.status);
                    ui.label(
                        egui::RichText::new(format!("Status: {icon} {text}"))
                            .color(color)
                            .strong(),
                    );
                    if let Some(error_code) = &record.error_code {
                        ui.label(format!("Error Code: {error_code}"));
                    }
                    if let Some(error_message) = &record.error_message {
                        ui.label(format!("Error Message: {error_message}"));
                    }
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.detail_tab, DetailTab::Request, "Request");
                        ui.selectable_value(&mut self.detail_tab, DetailTab::Response, "Response");
                    });
                    ui.separator();

                    match self.detail_tab {
                        DetailTab::Request => {
                            render_json_payload(ui, &record.request_body_json);
                        }
                        DetailTab::Response => {
                            if let Some(body) = &record.response_body_json {
                                render_json_payload(ui, body);
                            } else {
                                ui.monospace("<empty>");
                            }
                        }
                    }
                });
            if !open {
                self.detail_record = None;
            }
        }
    }
}

fn llm_status_display(status: LlmAuditStatus) -> (&'static str, Color32, &'static str) {
    match status {
        LlmAuditStatus::Success => ("✓", Color32::from_rgb(50, 180, 80), "success"),
        LlmAuditStatus::Failed => ("✗", Color32::from_rgb(220, 60, 60), "failed"),
    }
}

fn render_json_payload(ui: &mut egui::Ui, raw: &str) {
    egui::ScrollArea::both()
        .id_salt(("llm-audit-json-scroll", raw.len()))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            match serde_json::from_str::<serde_json::Value>(raw) {
                Ok(value) => show_json_tree_with_id(ui, &value, &format!("llm-json:{raw}")),
                Err(_) => {
                    let mut text = raw.to_string();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(25)
                            .interactive(false),
                    );
                }
            }
        });
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
