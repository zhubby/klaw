use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use crate::widgets::show_json_tree_with_id;
use chrono::{Datelike, Local, NaiveDate};
use egui::Color32;
use egui_extras::{Column, DatePickerButton, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore};
use klaw_session::{
    LlmAuditFilterOptions, LlmAuditFilterOptionsQuery, LlmAuditQuery, LlmAuditRecord,
    LlmAuditSortOrder, LlmAuditStatus, SessionError, SessionManager, SqliteSessionManager,
};
use std::future::Future;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 50.0;
const LLM_AUDIT_POLL_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum DetailTab {
    #[default]
    Request,
    Response,
}

struct LlmAuditLoad {
    filter_options: LlmAuditFilterOptions,
    rows: Vec<LlmAuditRecord>,
}

struct PendingLlmAuditLoad {
    receiver: Receiver<Result<LlmAuditLoad, String>>,
}

pub struct LlmPanel {
    loaded: bool,
    loading: bool,
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
    load_request: Option<PendingLlmAuditLoad>,
    refresh_queued: bool,
    store: Option<ConfigStore>,
    config: AppConfig,
}

impl Default for LlmPanel {
    fn default() -> Self {
        let today = Local::now().date_naive();
        let one_year_ago = today - chrono::Duration::days(365);
        Self {
            loaded: false,
            loading: false,
            rows: Vec::new(),
            session_options: Vec::new(),
            provider_options: Vec::new(),
            session_filter: None,
            provider_filter: None,
            start_date: Some(one_year_ago),
            end_date: Some(today),
            page: 1,
            size: 50,
            sort_order: LlmAuditSortOrder::RequestedAtDesc,
            selected_id: None,
            detail_record: None,
            detail_tab: DetailTab::default(),
            load_request: None,
            refresh_queued: false,
            store: None,
            config: AppConfig::default(),
        }
    }
}

impl LlmPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }

        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_config_snapshot(snapshot);
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_config_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
    }

    fn reload_config(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            return;
        };

        match store.reload() {
            Ok(snapshot) => self.apply_config_snapshot(snapshot),
            Err(err) => notifications.error(format!("Failed to reload config: {err}")),
        }
    }

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
        self.loading = true;
        self.load_request = Some(PendingLlmAuditLoad {
            receiver: spawn_session_task(move |manager| async move {
                let filter_options = manager.list_llm_audit_filter_options(&filter_query).await?;
                let rows = manager.list_llm_audit(&query).await?;
                Ok(LlmAuditLoad {
                    filter_options,
                    rows,
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
                Ok(load) => {
                    self.session_options = load.filter_options.session_keys;
                    self.provider_options = load.filter_options.providers;
                    self.rows = load.rows;
                    self.loaded = true;
                    self.loading = false;
                    if self.refresh_queued {
                        self.refresh_queued = false;
                        self.refresh(notifications);
                    }
                }
                Err(err) => {
                    self.loading = false;
                    notifications.error(format!("Failed to load LLM audit rows: {err}"));
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
                notifications.error("LLM audit loader closed unexpectedly");
            }
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

    fn provider_display_name<'a>(&'a self, provider_id: &'a str) -> &'a str {
        self.config
            .model_providers
            .get(provider_id)
            .and_then(|provider| provider.name.as_deref())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(provider_id)
    }
}

impl PanelRenderer for LlmPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        self.ensure_loaded(notifications);
        self.poll_load_request(notifications);
        if self.load_request.is_some() {
            ui.ctx().request_repaint_after(LLM_AUDIT_POLL_INTERVAL);
        }

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.reload_config(notifications);
                self.refresh(notifications);
            }
            ui.label(format!("Rows: {}", self.rows.len()));
            if self.loading {
                ui.add(egui::Spinner::new());
                ui.small("Loading...");
            }
        });

        ui.separator();
        let mut need_refresh = false;
        ui.horizontal_wrapped(|ui| {
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
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("provider");
                let combo_resp = egui::ComboBox::from_id_salt("llm-audit-provider-filter")
                    .selected_text(
                        self.provider_filter
                            .as_deref()
                            .map(|provider| self.provider_display_name(provider))
                            .unwrap_or("All"),
                    )
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
                            let provider_label = self.provider_display_name(provider).to_string();
                            if ui
                                .selectable_value(
                                    &mut self.provider_filter,
                                    Some(provider.clone()),
                                    provider_label,
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
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("start date");
                if render_date_picker(ui, &mut self.start_date, "llm-audit-start-date") {
                    need_refresh = true;
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("end date");
                if render_date_picker(ui, &mut self.end_date, "llm-audit-end-date") {
                    need_refresh = true;
                }
            });
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
            self.reload_config(notifications);
            self.refresh(notifications);
        }

        ui.separator();
        let mut open_detail: Option<LlmAuditRecord> = None;
        let table_width = ui.available_width();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                if self.loading && !self.loaded {
                    ui.vertical_centered(|ui| {
                        ui.add_space(16.0);
                        ui.add(egui::Spinner::new());
                        ui.label("Loading LLM audit rows...");
                    });
                    return;
                }
                if self.rows.is_empty() {
                    ui.label("No LLM audit rows found.");
                    return;
                }

                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::exact(170.0))
                    .column(Column::remainder().at_least(320.0))
                    .column(Column::exact(190.0))
                    .column(Column::exact(180.0))
                    .column(Column::exact(170.0))
                    .column(Column::exact(56.0))
                    .column(Column::exact(56.0))
                    .column(Column::exact(132.0))
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
                                render_truncated_cell(ui, &item.session_key);
                            });
                            row.col(|ui| {
                                render_truncated_cell(
                                    ui,
                                    self.provider_display_name(&item.provider),
                                );
                            });
                            row.col(|ui| {
                                render_truncated_cell(ui, &item.model);
                            });
                            row.col(|ui| {
                                render_truncated_cell(ui, &item.wire_api);
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
                            let interaction = handle_row_interaction(
                                is_selected,
                                item.id.clone(),
                                response.clicked(),
                                response.double_clicked(),
                            );
                            self.selected_id = interaction.selected_id;
                            if interaction.open_detail {
                                open_detail = Some(item.clone());
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
        if let Some(record) = self.detail_record.clone() {
            let mut open = true;
            let provider_display_name = self.provider_display_name(&record.provider).to_string();
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
                    ui.label(format!("Provider: {provider_display_name}"));
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

struct RowInteraction {
    selected_id: Option<String>,
    open_detail: bool,
}

fn handle_row_interaction(
    is_selected: bool,
    item_id: String,
    clicked: bool,
    double_clicked: bool,
) -> RowInteraction {
    if double_clicked {
        return RowInteraction {
            selected_id: Some(item_id),
            open_detail: true,
        };
    }

    let selected_id = if clicked {
        if is_selected { None } else { Some(item_id) }
    } else if is_selected {
        Some(item_id)
    } else {
        None
    };

    RowInteraction {
        selected_id,
        open_detail: false,
    }
}

fn render_truncated_cell(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(text).truncate())
        .on_hover_text(text);
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

fn spawn_session_task<T, F, Fut>(op: F) -> Receiver<Result<T, String>>
where
    T: Send + 'static,
    F: FnOnce(Box<dyn SessionManager>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, SessionError>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))
            .and_then(|runtime| {
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
        let _ = tx.send(result);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::ModelProviderConfig;

    fn sample_record() -> LlmAuditRecord {
        LlmAuditRecord {
            id: "audit-1".to_string(),
            session_key: "session-1".to_string(),
            chat_id: "chat-1".to_string(),
            turn_index: 1,
            request_seq: 1,
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            wire_api: "responses".to_string(),
            status: LlmAuditStatus::Success,
            error_code: None,
            error_message: None,
            provider_request_id: Some("req-1".to_string()),
            provider_response_id: Some("resp-1".to_string()),
            request_body_json: "{}".to_string(),
            response_body_json: Some("{}".to_string()),
            metadata_json: None,
            requested_at_ms: 1_700_000_000_000,
            responded_at_ms: Some(1_700_000_000_100),
            created_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn default_page_size_is_fifty() {
        assert_eq!(LlmPanel::default().size, 50);
    }

    #[test]
    fn provider_display_name_prefers_config_name_and_falls_back_to_id() {
        let mut panel = LlmPanel::default();
        panel.config.model_providers.insert(
            "openai".to_string(),
            ModelProviderConfig {
                name: Some("OpenAI Prod".to_string()),
                ..ModelProviderConfig::default()
            },
        );
        panel.config.model_providers.insert(
            "anthropic".to_string(),
            ModelProviderConfig {
                name: Some("   ".to_string()),
                ..ModelProviderConfig::default()
            },
        );

        assert_eq!(panel.provider_display_name("openai"), "OpenAI Prod");
        assert_eq!(panel.provider_display_name("anthropic"), "anthropic");
        assert_eq!(panel.provider_display_name("missing"), "missing");
    }

    #[test]
    fn refresh_queues_when_request_is_in_flight() {
        let (_tx, rx) = mpsc::channel();
        let mut panel = LlmPanel {
            loading: true,
            load_request: Some(PendingLlmAuditLoad { receiver: rx }),
            ..LlmPanel::default()
        };

        panel.refresh(&mut NotificationCenter::default());

        assert!(panel.refresh_queued);
        assert!(panel.load_request.is_some());
    }

    #[test]
    fn poll_load_request_applies_loaded_rows() {
        let (tx, rx) = mpsc::channel();
        let mut panel = LlmPanel {
            loading: true,
            load_request: Some(PendingLlmAuditLoad { receiver: rx }),
            ..LlmPanel::default()
        };
        tx.send(Ok(LlmAuditLoad {
            filter_options: LlmAuditFilterOptions {
                session_keys: vec!["session-1".to_string()],
                providers: vec!["openai".to_string()],
            },
            rows: vec![sample_record()],
        }))
        .expect("send load result");

        panel.poll_load_request(&mut NotificationCenter::default());

        assert!(panel.loaded);
        assert!(!panel.loading);
        assert_eq!(panel.rows.len(), 1);
        assert_eq!(panel.session_options, vec!["session-1".to_string()]);
        assert_eq!(panel.provider_options, vec!["openai".to_string()]);
        assert!(panel.load_request.is_none());
    }

    #[test]
    fn single_click_toggles_selection_without_opening_detail() {
        let interaction = handle_row_interaction(false, "audit-1".to_string(), true, false);
        assert_eq!(interaction.selected_id, Some("audit-1".to_string()));
        assert!(!interaction.open_detail);

        let interaction = handle_row_interaction(true, "audit-1".to_string(), true, false);
        assert_eq!(interaction.selected_id, None);
        assert!(!interaction.open_detail);
    }

    #[test]
    fn double_click_keeps_row_selected_and_opens_detail() {
        let interaction = handle_row_interaction(false, "audit-1".to_string(), true, true);
        assert_eq!(interaction.selected_id, Some("audit-1".to_string()));
        assert!(interaction.open_detail);

        let interaction = handle_row_interaction(true, "audit-1".to_string(), true, true);
        assert_eq!(interaction.selected_id, Some("audit-1".to_string()));
        assert!(interaction.open_detail);
    }
}
