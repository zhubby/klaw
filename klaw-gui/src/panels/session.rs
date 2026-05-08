use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use crate::widgets::{ChatBox, ChatMessage, ChatRole};
use chrono::{Datelike, Local, NaiveDate};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use egui_phosphor::regular;
use klaw_session::{
    LlmUsageSummary, SessionCleanupProgress, SessionCleanupQuery, SessionCleanupSummary,
    SessionError, SessionIndex, SessionListQuery, SessionManager, SessionSortOrder,
    SqliteSessionManager,
};
use std::{future::Future, sync::mpsc, thread, time::Duration};
use time::{Month, OffsetDateTime, PrimitiveDateTime, Time};
use tokio::runtime::Builder;

const PAGING_INPUT_WIDTH: f32 = 50.0;

pub struct SessionPanel {
    loaded: bool,
    sessions: Vec<SessionRow>,
    total_count: i64,
    channels: Vec<String>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    channel_filter: Option<String>,
    sort_order: SessionSortOrder,
    page: i64,
    size: i64,
    selected_session: Option<String>,
    chat_box: Option<ChatBox>,
    cleanup_open: bool,
    cleanup_updated_before: Option<NaiveDate>,
    cleanup_cron: bool,
    cleanup_webhook: bool,
    cleanup_request: Option<mpsc::Receiver<SessionCleanupTaskMessage>>,
    cleanup_progress: Option<SessionCleanupProgress>,
}

impl Default for SessionPanel {
    fn default() -> Self {
        let today = Local::now().date_naive();
        let one_year_ago = today - chrono::Duration::days(365);
        Self {
            loaded: false,
            sessions: Vec::new(),
            total_count: 0,
            channels: Vec::new(),
            start_date: Some(one_year_ago),
            end_date: Some(today),
            channel_filter: None,
            sort_order: SessionSortOrder::UpdatedAtDesc,
            page: 1,
            size: 100,
            selected_session: None,
            chat_box: None,
            cleanup_open: false,
            cleanup_updated_before: Some(today),
            cleanup_cron: true,
            cleanup_webhook: true,
            cleanup_request: None,
            cleanup_progress: None,
        }
    }
}

#[derive(Debug, Clone)]
struct SessionRow {
    session: SessionIndex,
    usage: LlmUsageSummary,
}

enum SessionCleanupTaskMessage {
    Progress(SessionCleanupProgress),
    Completed(Result<SessionCleanupSummary, String>),
}

impl SessionPanel {
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
        let query = SessionListQuery {
            limit: Some(size),
            offset,
            updated_from_ms: self.start_date.and_then(date_start_ms),
            updated_to_ms: self.end_date.and_then(date_end_ms),
            channel: self.channel_filter.clone(),
            session_key_prefix: None,
            sort_order: self.sort_order,
        };

        match run_session_task(move |manager| async move {
            let channels = manager.list_session_channels().await?;
            let total_count = manager.count_sessions(query.clone()).await?;
            let sessions = manager.list_sessions(query).await?;
            let mut rows = Vec::with_capacity(sessions.len());
            for session in sessions {
                let usage = manager
                    .sum_llm_usage_by_session(&session.session_key)
                    .await?;
                rows.push(SessionRow { session, usage });
            }
            Ok((channels, total_count, rows))
        }) {
            Ok((channels, total_count, sessions)) => {
                self.channels = channels;
                self.total_count = total_count;
                self.sessions = sessions;
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to load sessions: {err}")),
        }
    }

    fn load_chat_session(&mut self, session_key: &str, notifications: &mut NotificationCenter) {
        let session_key_owned = session_key.to_string();
        match run_session_task(move |manager| async move {
            manager.read_chat_records(&session_key_owned).await
        }) {
            Ok(records) => {
                let messages: Vec<ChatMessage> = records
                    .iter()
                    .map(|r| {
                        ChatMessage::new(ChatRole::from_str(&r.role), &r.content)
                            .with_timestamp(r.ts_ms)
                    })
                    .collect();

                let mut chat_box =
                    ChatBox::new(format!("Chat: {}", session_key)).with_messages(messages);
                chat_box.open();
                self.chat_box = Some(chat_box);
            }
            Err(err) => {
                notifications.error(format!("Failed to load chat records: {err}"));
            }
        }
    }

    fn begin_clean_sessions(&mut self, notifications: &mut NotificationCenter) {
        if self.cleanup_request.is_some() {
            notifications.info("Session cleanup is already in progress.");
            return;
        }
        let Some(query) = self.cleanup_query() else {
            notifications.error("Select an Updated At date and at least one session type.");
            return;
        };

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = run_session_task(move |manager| async move {
                manager
                    .clean_sessions_with_progress(&query, &move |progress| {
                        let _ = progress_tx.send(SessionCleanupTaskMessage::Progress(progress));
                    })
                    .await
            });
            let _ = tx.send(SessionCleanupTaskMessage::Completed(result));
        });
        self.cleanup_progress = Some(SessionCleanupProgress::default());
        self.cleanup_request = Some(rx);
    }

    fn poll_cleanup_request(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(rx) = self.cleanup_request.take() else {
            return;
        };

        let mut completed = false;
        loop {
            match rx.try_recv() {
                Ok(SessionCleanupTaskMessage::Progress(progress)) => {
                    self.cleanup_progress = Some(progress);
                }
                Ok(SessionCleanupTaskMessage::Completed(Ok(summary))) => {
                    self.cleanup_progress = None;
                    completed = true;
                    notifications.success(format!(
                        "Cleaned {} sessions and deleted {} JSONL files ({} already missing).",
                        summary.session_records_deleted,
                        summary.jsonl_files_deleted,
                        summary.jsonl_files_missing
                    ));
                    self.refresh(notifications);
                    break;
                }
                Ok(SessionCleanupTaskMessage::Completed(Err(err))) => {
                    self.cleanup_progress = None;
                    completed = true;
                    notifications.error(format!("Failed to clean sessions: {err}"));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.cleanup_progress = None;
                    completed = true;
                    notifications
                        .error("Failed to clean sessions: cleanup task stopped unexpectedly");
                    break;
                }
            }
        }

        if !completed {
            self.cleanup_request = Some(rx);
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn toggle_sort_order(&mut self) {
        self.sort_order = match self.sort_order {
            SessionSortOrder::UpdatedAtAsc => SessionSortOrder::UpdatedAtDesc,
            SessionSortOrder::UpdatedAtDesc | SessionSortOrder::CreatedAtDesc => {
                SessionSortOrder::UpdatedAtAsc
            }
        };
    }

    fn updated_at_label(&self) -> &'static str {
        match self.sort_order {
            SessionSortOrder::UpdatedAtAsc => "Updated At ↑",
            SessionSortOrder::UpdatedAtDesc => "Updated At ↓",
            SessionSortOrder::CreatedAtDesc => "Created At ↓",
        }
    }

    fn cleanup_channels(&self) -> Vec<String> {
        let mut channels = Vec::new();
        if self.cleanup_cron {
            channels.push("cron".to_string());
        }
        if self.cleanup_webhook {
            channels.push("webhook".to_string());
        }
        channels
    }

    fn cleanup_query(&self) -> Option<SessionCleanupQuery> {
        let updated_before_ms = self.cleanup_updated_before.and_then(date_start_ms)?;
        let channels = self.cleanup_channels();
        if channels.is_empty() {
            return None;
        }
        Some(SessionCleanupQuery {
            updated_before_ms,
            channels,
        })
    }

    fn render_cleanup_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        if !self.cleanup_open {
            return;
        }

        let mut open = self.cleanup_open;
        let mut should_clean = false;
        let mut should_close = false;
        egui::Window::new("Clean Sessions")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Delete cron/webhook sessions updated before the selected date.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Updated At before");
                    if let Some(date) = self.cleanup_updated_before.as_mut() {
                        ui.add(
                            DatePickerButton::new(date)
                                .id_salt("session-clean-updated-before")
                                .format("%Y/%m/%d"),
                        );
                    }
                });
                ui.add_space(8.0);
                ui.label("Session types");
                ui.checkbox(&mut self.cleanup_cron, "cron");
                ui.checkbox(&mut self.cleanup_webhook, "webhook");
                ui.add_space(8.0);

                if self.cleanup_query().is_none() {
                    ui.label("Select a date and at least one session type to continue.");
                }

                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.cleanup_query().is_some(), egui::Button::new("Clean"))
                        .clicked()
                    {
                        should_clean = true;
                    }
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                });
            });

        self.cleanup_open = open && !should_close;
        if should_clean {
            self.cleanup_open = false;
            self.begin_clean_sessions(notifications);
        }
    }

    fn render_cleanup_progress_dialog(&self, ctx: &egui::Context) {
        if self.cleanup_request.is_none() {
            return;
        }

        ctx.request_repaint_after(Duration::from_millis(100));
        egui::Window::new("Cleaning Sessions")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label("Cleaning expired cron/webhook sessions...");
                });
                ui.add_space(8.0);
                let progress = self.cleanup_progress.clone().unwrap_or_default();
                let fraction = if progress.total_sessions > 0 {
                    progress.deleted_sessions as f32 / progress.total_sessions as f32
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                        .desired_width(360.0)
                        .show_percentage()
                        .text(format!(
                            "{} / {}",
                            progress.deleted_sessions, progress.total_sessions
                        )),
                );
                ui.label(format!("Total: {}", progress.total_sessions));
                ui.label(format!("Deleted: {}", progress.deleted_sessions));
                ui.small("This dialog will close automatically when cleanup finishes.");
            });
    }
}

impl PanelRenderer for SessionPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);
        self.poll_cleanup_request(ui.ctx(), notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh(notifications);
            }
            if ui.button("Clean").clicked() {
                if self.cleanup_updated_before.is_none() {
                    self.cleanup_updated_before = Some(Local::now().date_naive());
                }
                self.cleanup_open = true;
            }
            ui.label(format!("Sessions: {}", self.total_count));
        });

        ui.separator();
        let mut need_refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.horizontal(|ui| {
                ui.label("Start Date");
                if render_date_picker(ui, &mut self.start_date, "session-start-date") {
                    need_refresh = true;
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("End Date");
                if render_date_picker(ui, &mut self.end_date, "session-end-date") {
                    need_refresh = true;
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Channel");
                let combo_resp = egui::ComboBox::from_id_salt("session-channel-filter")
                    .selected_text(self.channel_filter.as_deref().unwrap_or("All"))
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        let mut changed = false;
                        if ui
                            .selectable_value(&mut self.channel_filter, None, "All")
                            .changed()
                        {
                            changed = true;
                        }
                        for channel in &self.channels {
                            if ui
                                .selectable_value(
                                    &mut self.channel_filter,
                                    Some(channel.clone()),
                                    channel,
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                        changed
                    });
                if combo_resp.inner.unwrap_or(false) {
                    self.page = 1;
                    need_refresh = true;
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Page");
                if ui
                    .add_sized(
                        [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut self.page).range(1..=i64::MAX),
                    )
                    .changed()
                {
                    need_refresh = true;
                }
                ui.label("Size");
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
        });
        if need_refresh {
            self.refresh(notifications);
        }

        ui.separator();

        let mut view_session_key: Option<String> = None;

        let table_width = ui.available_width();
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .max_width(table_width)
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                if self.sessions.is_empty() {
                    ui.label("No sessions found.");
                    return;
                }

                let available_height = ui.available_height();
                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(60.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(50.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::remainder().at_least(100.0))
                    .min_scrolled_height(0.0)
                    .max_scroll_height(available_height)
                    .sense(egui::Sense::click())
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("Session Key");
                        });
                        header.col(|ui| {
                            ui.strong("Chat ID");
                        });
                        header.col(|ui| {
                            ui.strong("Channel");
                        });
                        header.col(|ui| {
                            ui.strong("Active Session");
                        });
                        header.col(|ui| {
                            ui.strong("Provider");
                        });
                        header.col(|ui| {
                            ui.strong("Model");
                        });
                        header.col(|ui| {
                            ui.strong("Turns");
                        });
                        header.col(|ui| {
                            ui.strong("Input");
                        });
                        header.col(|ui| {
                            ui.strong("Output");
                        });
                        header.col(|ui| {
                            ui.strong("Total");
                        });
                        header.col(|ui| {
                            if ui.button(self.updated_at_label()).clicked() {
                                self.toggle_sort_order();
                                self.refresh(notifications);
                            }
                        });
                        header.col(|ui| {
                            ui.strong("JSONL Path");
                        });
                    })
                    .body(|body| {
                        body.rows(20.0, self.sessions.len(), |mut row| {
                            let idx = row.index();
                            let session_row = &self.sessions[idx];
                            let session = &session_row.session;
                            let is_selected =
                                self.selected_session.as_deref() == Some(&session.session_key);

                            row.set_selected(is_selected);

                            row.col(|ui| {
                                ui.label(&session.session_key);
                            });
                            row.col(|ui| {
                                ui.label(&session.chat_id);
                            });
                            row.col(|ui| {
                                ui.label(&session.channel);
                            });
                            row.col(|ui| {
                                ui.label(session.active_session_key.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(session.model_provider.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(session.model.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(session.turn_count.to_string());
                            });
                            row.col(|ui| {
                                ui.label(session_row.usage.input_tokens.to_string());
                            });
                            row.col(|ui| {
                                ui.label(session_row.usage.output_tokens.to_string());
                            });
                            row.col(|ui| {
                                ui.label(session_row.usage.total_tokens.to_string());
                            });
                            row.col(|ui| {
                                ui.label(format_timestamp_millis(session.updated_at_ms));
                            });
                            row.col(|ui| {
                                ui.label(&session.jsonl_path);
                            });

                            let response = row.response();

                            if response.clicked() {
                                self.selected_session = if is_selected {
                                    None
                                } else {
                                    Some(session.session_key.clone())
                                };
                            }

                            if response.double_clicked() {
                                view_session_key = Some(session.session_key.clone());
                            }

                            response.context_menu(|ui| {
                                if ui
                                    .button(format!("{} View Chat", regular::CHATS_CIRCLE))
                                    .clicked()
                                {
                                    view_session_key = Some(session.session_key.clone());
                                    ui.close();
                                }
                                if ui
                                    .button(format!("{} Copy Session Key", regular::KEY))
                                    .clicked()
                                {
                                    ui.ctx().output_mut(|o| {
                                        o.commands.push(egui::OutputCommand::CopyText(
                                            session.session_key.clone(),
                                        ));
                                    });
                                    ui.close();
                                }
                            });
                        });
                    });
            });

        if let Some(session_key) = view_session_key {
            self.load_chat_session(&session_key, notifications);
        }

        if let Some(chat_box) = &mut self.chat_box {
            chat_box.show(ui.ctx());
        }
        self.render_cleanup_dialog(ui.ctx(), notifications);
        self.render_cleanup_progress_dialog(ui.ctx());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_query_requires_date_and_channel() {
        let panel = SessionPanel {
            cleanup_updated_before: None,
            ..Default::default()
        };
        assert!(panel.cleanup_query().is_none());

        let panel = SessionPanel {
            cleanup_updated_before: Some(
                NaiveDate::from_ymd_opt(2026, 5, 7).expect("test date should be valid"),
            ),
            cleanup_cron: false,
            cleanup_webhook: false,
            ..Default::default()
        };
        assert!(panel.cleanup_query().is_none());
    }

    #[test]
    fn cleanup_query_uses_selected_cleanup_channels() {
        let panel = SessionPanel {
            cleanup_updated_before: Some(
                NaiveDate::from_ymd_opt(2026, 5, 7).expect("test date should be valid"),
            ),
            cleanup_cron: true,
            cleanup_webhook: false,
            ..Default::default()
        };

        let query = panel
            .cleanup_query()
            .expect("selected date and channel should build query");

        assert_eq!(query.channels, vec!["cron".to_string()]);
        assert_eq!(
            query.updated_before_ms,
            date_start_ms(panel.cleanup_updated_before.expect("date should exist"))
                .expect("date should convert")
        );
    }
}
