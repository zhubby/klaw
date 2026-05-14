use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::current_ui_language;
use crate::time_format::{format_optional_timestamp_millis, format_timestamp_millis};
use crate::{RuntimeRequestHandle, begin_run_heartbeat_now_request};
use chrono::{Local, NaiveDate};
use egui::{Color32, RichText};
use egui_extras::{Column, DatePickerButton, TableBuilder};
use egui_phosphor::regular;
use klaw_heartbeat::{
    DEFAULT_RECENT_MESSAGES_LIMIT, DEFAULT_SILENT_ACK_TOKEN, HeartbeatInput, HeartbeatManager,
};
use klaw_storage::{
    DefaultSessionStore, HeartbeatJob, HeartbeatTaskRun, HeartbeatTaskStatus, SessionIndex,
    SessionStorage, open_default_store,
};
use klaw_ui_kit::{LocaleDomain, Translator, label_with_hint, toggle::toggle};
use klaw_util::system_timezone_name;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::thread;
use tokio::runtime::Builder;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct HeartbeatForm {
    original_id: Option<String>,
    id: String,
    session_key: String,
    channel: String,
    chat_id: String,
    enabled: bool,
    every: String,
    prompt: String,
    silent_ack_token: String,
    recent_messages_limit: i64,
    timezone: String,
}

impl HeartbeatForm {
    fn new(defaults: &HeartbeatDefaults) -> Self {
        Self {
            original_id: None,
            id: Uuid::new_v4().to_string(),
            session_key: String::new(),
            channel: String::new(),
            chat_id: String::new(),
            enabled: defaults.enabled,
            every: "30m".to_string(),
            prompt: String::new(),
            silent_ack_token: DEFAULT_SILENT_ACK_TOKEN.to_string(),
            recent_messages_limit: defaults.recent_messages_limit,
            timezone: defaults.timezone.clone(),
        }
    }

    fn edit(item: &HeartbeatJob) -> Self {
        Self {
            original_id: Some(item.id.clone()),
            id: item.id.clone(),
            session_key: item.session_key.clone(),
            channel: item.channel.clone(),
            chat_id: item.chat_id.clone(),
            enabled: item.enabled,
            every: item.every.clone(),
            prompt: item.prompt.clone(),
            silent_ack_token: item.silent_ack_token.clone(),
            recent_messages_limit: item.recent_messages_limit,
            timezone: item.timezone.clone(),
        }
    }

    fn title(&self) -> String {
        let t = Translator::new(LocaleDomain::Gui, current_ui_language());
        if self.original_id.is_some() {
            t.text("hb-form-title-edit")
        } else {
            t.text("hb-form-title-add")
        }
    }

    fn to_input(&self) -> HeartbeatInput {
        HeartbeatInput {
            id: Some(self.id.trim().to_string()),
            session_key: self.session_key.trim().to_string(),
            channel: self.channel.trim().to_string(),
            chat_id: self.chat_id.trim().to_string(),
            enabled: self.enabled,
            every: self.every.trim().to_string(),
            prompt: self.prompt.trim().to_string(),
            silent_ack_token: self.silent_ack_token.trim().to_string(),
            recent_messages_limit: self.recent_messages_limit,
            timezone: self.timezone.trim().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct HeartbeatDefaults {
    enabled: bool,
    recent_messages_limit: i64,
    timezone: String,
}

impl Default for HeartbeatDefaults {
    fn default() -> Self {
        Self {
            enabled: true,
            recent_messages_limit: DEFAULT_RECENT_MESSAGES_LIMIT,
            timezone: system_timezone_name(),
        }
    }
}

pub struct HeartbeatPanel {
    loaded: bool,
    defaults: HeartbeatDefaults,
    sessions: Vec<SessionIndex>,
    jobs: Vec<HeartbeatJob>,
    runs_heartbeat_id: Option<String>,
    runs: Vec<HeartbeatTaskRun>,
    form: Option<HeartbeatForm>,
    delete_confirm_id: Option<String>,
    selected_heartbeat: Option<String>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    page: i64,
    size: i64,
    config_window: bool,
    run_now_request: Option<(String, RuntimeRequestHandle<String>)>,
}

impl Default for HeartbeatPanel {
    fn default() -> Self {
        let today = Local::now().date_naive();
        Self {
            loaded: false,
            defaults: HeartbeatDefaults::default(),
            sessions: Vec::new(),
            jobs: Vec::new(),
            runs_heartbeat_id: None,
            runs: Vec::new(),
            form: None,
            delete_confirm_id: None,
            selected_heartbeat: None,
            start_date: Some(today),
            end_date: Some(today),
            page: 1,
            size: 20,
            config_window: false,
            run_now_request: None,
        }
    }
}

impl HeartbeatPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.refresh_sessions(notifications);
        self.refresh_jobs(notifications);
    }

    fn refresh_sessions(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        match run_session_query(500, 0) {
            Ok(sessions) => {
                self.sessions = sessions;
            }
            Err(err) => notifications
                .error(t.text_args("hb-notify-sessions-failed", HashMap::from([("error", err)]))),
        }
    }

    fn refresh_jobs(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        match run_heartbeat_task(move |manager| async move { manager.list_jobs(200, 0).await }) {
            Ok(jobs) => {
                self.jobs = jobs;
                self.loaded = true;
                if let Some(id) = self.runs_heartbeat_id.clone() {
                    self.load_runs(&id, notifications);
                }
            }
            Err(err) => notifications
                .error(t.text_args("hb-notify-jobs-failed", HashMap::from([("error", err)]))),
        }
    }

    fn load_runs(&mut self, heartbeat_id: &str, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let heartbeat_id = heartbeat_id.to_string();
        let heartbeat_id_for_query = heartbeat_id.clone();
        match run_heartbeat_task(move |manager| async move {
            manager.list_runs(&heartbeat_id_for_query, 30, 0).await
        }) {
            Ok(runs) => {
                self.runs_heartbeat_id = Some(heartbeat_id);
                self.runs = runs;
            }
            Err(err) => notifications
                .error(t.text_args("hb-notify-runs-failed", HashMap::from([("error", err)]))),
        }
    }

    fn open_add_form(&mut self) {
        self.form = Some(HeartbeatForm::new(&self.defaults));
        self.sync_form_session_selection();
    }

    fn open_edit_form(&mut self, heartbeat_id: &str) {
        if let Some(item) = self.jobs.iter().find(|job| job.id == heartbeat_id) {
            self.form = Some(HeartbeatForm::edit(item));
            self.sync_form_session_selection();
        }
    }

    fn sync_form_session_selection(&mut self) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if let Some(session) = self
            .sessions
            .iter()
            .find(|session| session.session_key == form.session_key)
        {
            form.channel = session.channel.clone();
            form.chat_id = session.chat_id.clone();
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(form) = self.form.as_ref() else {
            notifications.error(t.text("hb-notify-form-unavailable"));
            return;
        };

        let input = form.to_input();
        if input.id.as_deref().is_some_and(|id| id.is_empty()) {
            notifications.error(t.text("hb-notify-id-empty"));
            return;
        }
        if input.session_key.is_empty() {
            notifications.error(t.text("hb-notify-session-empty"));
            return;
        }
        if input.channel.is_empty() {
            notifications.error(t.text("hb-notify-channel-empty"));
            return;
        }
        if input.chat_id.is_empty() {
            notifications.error(t.text("hb-notify-chat-id-empty"));
            return;
        }
        if input.every.is_empty() {
            notifications.error(t.text("hb-notify-every-empty"));
            return;
        }
        if input.silent_ack_token.is_empty() {
            notifications.error(t.text("hb-notify-ack-token-empty"));
            return;
        }
        if input.recent_messages_limit <= 0 {
            notifications.error(t.text("hb-notify-recent-msgs-zero"));
            return;
        }
        if input.timezone.is_empty() {
            notifications.error(t.text("hb-notify-timezone-empty"));
            return;
        }

        if let Some(original_id) = &form.original_id {
            let original_id = original_id.clone();
            let input = input.clone();
            match run_heartbeat_task(move |manager| async move {
                manager.update_job(&original_id, &input).await
            }) {
                Ok(_) => {
                    notifications.success(t.text("hb-notify-updated"));
                    self.form = None;
                    self.refresh_jobs(notifications);
                }
                Err(err) => notifications
                    .error(t.text_args("hb-notify-update-failed", HashMap::from([("error", err)]))),
            }
            return;
        }

        match run_heartbeat_task(move |manager| async move { manager.create_job(&input).await }) {
            Ok(_) => {
                notifications.success(t.text("hb-notify-created"));
                self.form = None;
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications
                .error(t.text_args("hb-notify-create-failed", HashMap::from([("error", err)]))),
        }
    }

    fn set_enabled(
        &mut self,
        heartbeat_id: &str,
        enabled: bool,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        let heartbeat_id = heartbeat_id.to_string();
        match run_heartbeat_task(move |manager| async move {
            manager.set_enabled(&heartbeat_id, enabled).await
        }) {
            Ok(()) => {
                notifications.success(if enabled {
                    t.text("hb-notify-enabled")
                } else {
                    t.text("hb-notify-disabled")
                });
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(t.text_args(
                "hb-notify-set-enabled-failed",
                HashMap::from([("error", err)]),
            )),
        }
    }

    fn delete_heartbeat(&mut self, heartbeat_id: &str, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let heartbeat_id = heartbeat_id.to_string();
        let heartbeat_id_for_delete = heartbeat_id.clone();
        match run_heartbeat_task(move |manager| async move {
            manager.delete_job(&heartbeat_id_for_delete).await
        }) {
            Ok(()) => {
                notifications.success(t.text("hb-notify-deleted"));
                if self.runs_heartbeat_id.as_deref() == Some(heartbeat_id.as_str()) {
                    self.runs_heartbeat_id = None;
                    self.runs.clear();
                }
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications
                .error(t.text_args("hb-notify-delete-failed", HashMap::from([("error", err)]))),
        }
    }

    fn run_heartbeat_now(&mut self, heartbeat_id: &str, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        if self.run_now_request.is_some() {
            notifications.info(t.text("hb-notify-already-running"));
            return;
        }
        self.run_now_request = Some((
            heartbeat_id.to_string(),
            begin_run_heartbeat_now_request(heartbeat_id.to_string()),
        ));
        notifications.info(t.text_args(
            "hb-notify-running-bg",
            HashMap::from([("id", heartbeat_id.to_string())]),
        ));
    }

    fn poll_run_now_request(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some((_, request)) = self.run_now_request.as_mut() else {
            return;
        };
        let Some(result) = request.try_take_result() else {
            return;
        };
        let heartbeat_id = self
            .run_now_request
            .take()
            .map(|(heartbeat_id, _)| heartbeat_id);
        match result {
            Ok(message_id) => {
                notifications.success(
                    t.text_args("hb-notify-executed", HashMap::from([("id", message_id)])),
                );
                self.refresh_jobs(notifications);
                if let Some(heartbeat_id) = heartbeat_id {
                    self.load_runs(&heartbeat_id, notifications);
                }
            }
            Err(err) => notifications
                .error(t.text_args("hb-notify-run-failed", HashMap::from([("error", err)]))),
        }
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new(form.title())
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(620.0);
                let session_items = self
                    .sessions
                    .iter()
                    .map(|session| {
                        (
                            session.session_key.clone(),
                            format!(
                                "{}  [{} / {}]",
                                session.session_key, session.channel, session.chat_id
                            ),
                        )
                    })
                    .collect::<Vec<_>>();
                egui::Grid::new("heartbeat-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t.text("hb-form-id"));
                        if form.original_id.is_some() {
                            ui.add_enabled(false, egui::TextEdit::singleline(&mut form.id));
                        } else {
                            ui.text_edit_singleline(&mut form.id);
                        }
                        ui.end_row();

                        ui.label(t.text("hb-form-session-key"));
                        egui::ComboBox::from_id_salt("heartbeat-session-key")
                            .selected_text(if form.session_key.trim().is_empty() {
                                t.text("hb-form-session-select")
                            } else {
                                form.session_key.clone()
                            })
                            .width(420.0)
                            .show_ui(ui, |ui| {
                                for (session_key, label) in &session_items {
                                    let selected = form.session_key == *session_key;
                                    if ui.selectable_label(selected, label).clicked() {
                                        form.session_key = session_key.clone();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label(t.text("hb-form-channel"));
                        ui.add_enabled(false, egui::TextEdit::singleline(&mut form.channel));
                        ui.end_row();

                        ui.label(t.text("hb-form-chat-id"));
                        ui.add_enabled(false, egui::TextEdit::singleline(&mut form.chat_id));
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("hb-form-enabled"),
                            &t.text("hb-form-enabled-hint"),
                        );
                        ui.add(toggle(&mut form.enabled));
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("hb-form-every"),
                            &t.text("hb-form-every-hint"),
                        );
                        ui.text_edit_singleline(&mut form.every);
                        ui.end_row();

                        ui.label(t.text("hb-form-timezone"));
                        ui.text_edit_singleline(&mut form.timezone);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("hb-form-silent-ack-token"),
                            &t.text("hb-form-silent-ack-token-hint"),
                        );
                        ui.text_edit_singleline(&mut form.silent_ack_token);
                        ui.end_row();

                        label_with_hint(
                            ui,
                            &t.text("hb-form-recent-messages"),
                            &t.text("hb-form-recent-messages-hint"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut form.recent_messages_limit).range(1..=10_000),
                        );
                        ui.end_row();
                    });

                if self.sessions.is_empty() {
                    ui.add_space(8.0);
                    ui.colored_label(ui.visuals().warn_fg_color, t.text("hb-form-no-sessions"));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t.text("hb-form-save")).clicked() {
                        save_clicked = true;
                    }
                    if ui.button(t.text("hb-form-cancel")).clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        self.sync_form_session_selection();

        if save_clicked {
            self.save_form(notifications);
        }
        if cancel_clicked {
            self.form = None;
        }
    }

    fn render_runs_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let Some(heartbeat_id) = self.runs_heartbeat_id.clone() else {
            return;
        };

        let mut keep_open = true;
        egui::Window::new(t.text_args(
            "hb-runs-title",
            HashMap::from([("id", heartbeat_id.clone())]),
        ))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(true)
        .open(&mut keep_open)
        .show(ui.ctx(), |ui| {
            ui.set_min_width(820.0);
            ui.horizontal(|ui| {
                if ui.button(t.text("hb-runs-refresh")).clicked() {
                    self.load_runs(&heartbeat_id, notifications);
                }
                if ui
                    .add_enabled(
                        self.run_now_request.is_none(),
                        egui::Button::new(t.text("hb-runs-run-now")),
                    )
                    .clicked()
                {
                    self.run_heartbeat_now(&heartbeat_id, notifications);
                }
            });

            ui.separator();

            if self.runs.is_empty() {
                ui.label(t.text("hb-runs-no-rows"));
                return;
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("heartbeat-run-grid")
                    .striped(true)
                    .num_columns(6)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong(t.text("hb-runs-col-id"));
                        ui.strong(t.text("hb-runs-col-status"));
                        ui.strong(t.text("hb-runs-col-scheduled"));
                        ui.strong(t.text("hb-runs-col-started"));
                        ui.strong(t.text("hb-runs-col-finished"));
                        ui.strong(t.text("hb-runs-col-error"));
                        ui.end_row();

                        for run in &self.runs {
                            let (icon, color, status_text) = status_display(run.status, &t);
                            ui.label(&run.id);
                            ui.label(
                                egui::RichText::new(format!("{icon} {status_text}"))
                                    .color(color)
                                    .strong(),
                            );
                            ui.label(format_timestamp_millis(run.scheduled_at_ms));
                            ui.label(format_optional_timestamp_millis(run.started_at_ms));
                            ui.label(format_optional_timestamp_millis(run.finished_at_ms));
                            ui.label(run.error_message.clone().unwrap_or_default());
                            ui.end_row();
                        }
                    });
            });
        });

        if !keep_open {
            self.runs_heartbeat_id = None;
            self.runs.clear();
        }
    }
}

impl PanelRenderer for HeartbeatPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        self.poll_run_now_request(notifications);
        self.ensure_loaded(notifications);
        if self.run_now_request.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }

        ui.heading(ctx.tab_title);
        ui.label(t.text("hb-subtitle"));
        ui.horizontal(|ui| {
            if ui
                .button(t.text_args(
                    "hb-btn-refresh",
                    HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.refresh_sessions(notifications);
                self.refresh_jobs(notifications);
            }
            if ui
                .button(t.text_args(
                    "hb-btn-add",
                    HashMap::from([("icon", regular::PLUS.to_string())]),
                ))
                .clicked()
            {
                self.open_add_form();
            }
            if ui
                .button(t.text_args(
                    "hb-btn-config",
                    HashMap::from([("icon", regular::GEAR.to_string())]),
                ))
                .clicked()
            {
                self.config_window = true;
            }
            ui.label(t.text_args(
                "hb-label-jobs",
                HashMap::from([("count", self.jobs.len().to_string())]),
            ));
            if self.run_now_request.is_some() {
                ui.label(t.text("hb-label-running"));
            }
        });

        ui.separator();
        let mut need_refresh = false;
        egui::ScrollArea::horizontal()
            .id_salt("heartbeat-filter-row")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.text("hb-filter-start-date"));
                    if render_date_picker(ui, &mut self.start_date, "heartbeat-start-date") {
                        need_refresh = true;
                    }
                    ui.separator();
                    ui.label(t.text("hb-filter-end-date"));
                    if render_date_picker(ui, &mut self.end_date, "heartbeat-end-date") {
                        need_refresh = true;
                    }
                    ui.separator();
                    ui.label(t.text("hb-filter-page"));
                    ui.add_sized(
                        [50.0, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut self.page).range(1..=i64::MAX),
                    );
                    ui.label(t.text("hb-filter-size"));
                    ui.add_sized(
                        [50.0, ui.spacing().interact_size.y],
                        egui::DragValue::new(&mut self.size).range(1..=1000),
                    );
                });
            });
        let _ = (); // suppress unused-variable warning from ScrollArea show()
        if need_refresh {
            self.refresh_sessions(notifications);
            self.refresh_jobs(notifications);
        }

        ui.separator();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.jobs.is_empty() {
                    ui.label(t.text("hb-no-rows"));
                } else {
                    let available_height = ui.available_height();
                    let mut edit_heartbeat_id: Option<String> = None;
                    let mut toggle_heartbeat: Option<(String, bool)> = None;
                    let mut delete_heartbeat_id: Option<String> = None;
                    let mut runs_heartbeat_id: Option<String> = None;
                    let mut run_now_heartbeat_id: Option<String> = None;

                    TableBuilder::new(ui)
                        .striped(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(100.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(120.0))
                        .column(Column::auto().at_least(70.0))
                        .column(Column::auto().at_least(120.0))
                        .column(Column::remainder().at_least(120.0))
                        .min_scrolled_height(0.0)
                        .max_scroll_height(available_height)
                        .sense(egui::Sense::click())
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-id"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-session"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-channel"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-enabled"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-every"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-recent-msgs"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-next-run"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-last-run"));
                            });
                            header.col(|ui| {
                                ui.strong(t.text("hb-col-updated-at"));
                            });
                        })
                        .body(|body| {
                            body.rows(20.0, self.jobs.len(), |mut row| {
                                let idx = row.index();
                                let job = &self.jobs[idx];
                                let is_selected =
                                    self.selected_heartbeat.as_deref() == Some(&job.id);

                                row.set_selected(is_selected);

                                row.col(|ui| {
                                    ui.label(job.id.clone());
                                });
                                row.col(|ui| {
                                    ui.label(job.session_key.clone());
                                });
                                row.col(|ui| {
                                    ui.label(job.channel.clone());
                                });
                                row.col(|ui| {
                                    ui.label(if job.enabled {
                                        t.text("hb-enabled-yes")
                                    } else {
                                        t.text("hb-enabled-no")
                                    });
                                });
                                row.col(|ui| {
                                    ui.label(job.every.clone());
                                });
                                row.col(|ui| {
                                    ui.label(job.recent_messages_limit.to_string());
                                });
                                row.col(|ui| {
                                    ui.label(format_timestamp_millis(job.next_run_at_ms));
                                });
                                row.col(|ui| {
                                    ui.label(format_optional_timestamp_millis(job.last_run_at_ms));
                                });
                                row.col(|ui| {
                                    ui.label(format_timestamp_millis(job.updated_at_ms));
                                });

                                let response = row.response();
                                if response.clicked() {
                                    self.selected_heartbeat = if is_selected {
                                        None
                                    } else {
                                        Some(job.id.clone())
                                    };
                                }

                                if response.double_clicked() {
                                    runs_heartbeat_id = Some(job.id.clone());
                                }

                                response.context_menu(|ui| {
                                    if ui
                                        .button(t.text_args(
                                            "hb-ctx-runs",
                                            HashMap::from([("icon", regular::LIST.to_string())]),
                                        ))
                                        .clicked()
                                    {
                                        runs_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled(
                                            self.run_now_request.is_none(),
                                            egui::Button::new(t.text_args(
                                                "hb-ctx-run-now",
                                                HashMap::from([(
                                                    "icon",
                                                    regular::PLAY.to_string(),
                                                )]),
                                            )),
                                        )
                                        .clicked()
                                    {
                                        run_now_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .button(t.text_args(
                                            "hb-ctx-edit",
                                            HashMap::from([(
                                                "icon",
                                                regular::PENCIL_SIMPLE.to_string(),
                                            )]),
                                        ))
                                        .clicked()
                                    {
                                        edit_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    let toggle_key = if job.enabled {
                                        "hb-ctx-disable"
                                    } else {
                                        "hb-ctx-enable"
                                    };
                                    if ui
                                        .button(t.text_args(
                                            toggle_key,
                                            HashMap::from([("icon", regular::POWER.to_string())]),
                                        ))
                                        .clicked()
                                    {
                                        toggle_heartbeat = Some((job.id.clone(), !job.enabled));
                                        ui.close();
                                    }
                                    if ui
                                        .button(
                                            RichText::new(t.text_args(
                                                "hb-ctx-delete",
                                                HashMap::from([(
                                                    "icon",
                                                    regular::TRASH.to_string(),
                                                )]),
                                            ))
                                            .color(ui.visuals().warn_fg_color),
                                        )
                                        .clicked()
                                    {
                                        delete_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(t.text_args(
                                            "hb-ctx-copy-id",
                                            HashMap::from([("icon", regular::COPY.to_string())]),
                                        ))
                                        .clicked()
                                    {
                                        ui.ctx().output_mut(|o| {
                                            o.commands.push(egui::OutputCommand::CopyText(
                                                job.id.clone(),
                                            ));
                                        });
                                        ui.close();
                                    }
                                });
                            });
                        });

                    if let Some(id) = runs_heartbeat_id {
                        self.load_runs(&id, notifications);
                    }
                    if let Some(id) = run_now_heartbeat_id {
                        self.run_heartbeat_now(&id, notifications);
                    }
                    if let Some(id) = edit_heartbeat_id {
                        self.open_edit_form(&id);
                    }
                    if let Some((id, enabled)) = toggle_heartbeat {
                        self.set_enabled(&id, enabled, notifications);
                    }
                    if let Some(id) = delete_heartbeat_id {
                        self.delete_confirm_id = Some(id);
                    }
                }
            });

        if let Some(heartbeat_id) = self.delete_confirm_id.clone() {
            egui::Window::new(t.text("hb-delete-title"))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(t.text_args(
                        "hb-delete-prompt",
                        HashMap::from([("id", heartbeat_id.clone())]),
                    ));
                    ui.horizontal(|ui| {
                        if ui.button(t.text("hb-delete-btn")).clicked() {
                            self.delete_heartbeat(&heartbeat_id, notifications);
                            self.delete_confirm_id = None;
                        }
                        if ui.button(t.text("hb-delete-cancel")).clicked() {
                            self.delete_confirm_id = None;
                        }
                    });
                });
        }

        self.render_runs_window(ui, notifications);
        self.render_form_window(ui, notifications);

        if self.config_window {
            egui::Window::new(t.text("hb-config-title"))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .open(&mut self.config_window)
                .show(ui.ctx(), |ui| {
                    ui.label(t.text("hb-config-form-defaults"));
                    ui.add_space(6.0);
                    label_with_hint(
                        ui,
                        &t.text("hb-config-enabled-default"),
                        &t.text("hb-config-enabled-default-hint"),
                    );
                    ui.add(toggle(&mut self.defaults.enabled));
                    ui.horizontal(|ui| {
                        ui.label(t.text("hb-config-recent-messages"));
                        ui.add(
                            egui::DragValue::new(&mut self.defaults.recent_messages_limit)
                                .range(1..=10_000),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new(t.text("hb-config-info")).small().weak());
                });
        }
    }
}

fn status_display(status: HeartbeatTaskStatus, t: &Translator) -> (&'static str, Color32, String) {
    let (icon, color) = match status {
        HeartbeatTaskStatus::Pending => ("\u{25d7}", Color32::from_rgb(140, 140, 140)),
        HeartbeatTaskStatus::Running => ("\u{25d1}", Color32::from_rgb(70, 130, 200)),
        HeartbeatTaskStatus::Success => ("\u{2713}", Color32::from_rgb(50, 180, 80)),
        HeartbeatTaskStatus::Failed => ("\u{2717}", Color32::from_rgb(220, 60, 60)),
    };
    let text = match status {
        HeartbeatTaskStatus::Pending => t.text("hb-status-pending"),
        HeartbeatTaskStatus::Running => t.text("hb-status-running"),
        HeartbeatTaskStatus::Success => t.text("hb-status-success"),
        HeartbeatTaskStatus::Failed => t.text("hb-status-failed"),
    };
    (icon, color, text)
}

fn run_heartbeat_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(HeartbeatManager<DefaultSessionStore>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, klaw_heartbeat::HeartbeatError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let store = open_default_store()
                .await
                .map_err(|err| format!("failed to open heartbeat store: {err}"))?;
            let manager = HeartbeatManager::new(Arc::new(store));
            op(manager)
                .await
                .map_err(|err| format!("heartbeat operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("heartbeat operation thread panicked".to_string()),
    }
}

fn run_session_query(limit: i64, offset: i64) -> Result<Vec<SessionIndex>, String> {
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let store = open_default_store()
                .await
                .map_err(|err| format!("failed to open session store: {err}"))?;
            store
                .list_sessions(
                    Some(limit),
                    offset,
                    None,
                    None,
                    None,
                    None,
                    klaw_storage::SessionSortOrder::UpdatedAtDesc,
                )
                .await
                .map_err(|err| format!("session query failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("session query thread panicked".to_string()),
    }
}

fn render_date_picker(ui: &mut egui::Ui, value: &mut Option<NaiveDate>, id: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let date = value.get_or_insert_with(|| Local::now().date_naive());
        if ui
            .add(DatePickerButton::new(date).id_salt(id).format("%Y/%m/%d"))
            .changed()
        {
            changed = true;
        }
        if ui.small_button("×").clicked() {
            *value = Some(Local::now().date_naive());
            changed = true;
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_defaults_to_system_timezone() {
        let defaults = HeartbeatDefaults::default();
        assert_eq!(defaults.timezone, system_timezone_name());
        assert_eq!(HeartbeatForm::new(&defaults).timezone, defaults.timezone);
    }
}
