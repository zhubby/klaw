use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::request_run_heartbeat_now;
use crate::time_format::{format_optional_timestamp_millis, format_timestamp_millis};
use chrono::NaiveDate;
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
use klaw_util::system_timezone_name;
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

    fn title(&self) -> &'static str {
        if self.original_id.is_some() {
            "Edit Heartbeat Job"
        } else {
            "Add Heartbeat Job"
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
}

impl Default for HeartbeatPanel {
    fn default() -> Self {
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
            start_date: None,
            end_date: None,
            page: 1,
            size: 20,
            config_window: false,
        }
    }
}

impl HeartbeatPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.refresh_sessions(notifications);
        self.refresh_jobs(notifications);
    }

    fn refresh_sessions(&mut self, notifications: &mut NotificationCenter) {
        match run_session_query(500, 0) {
            Ok(sessions) => {
                self.sessions = sessions;
            }
            Err(err) => notifications.error(format!("Failed to list sessions: {err}")),
        }
    }

    fn refresh_jobs(&mut self, notifications: &mut NotificationCenter) {
        match run_heartbeat_task(move |manager| async move { manager.list_jobs(200, 0).await }) {
            Ok(jobs) => {
                self.jobs = jobs;
                self.loaded = true;
                if let Some(id) = self.runs_heartbeat_id.clone() {
                    self.load_runs(&id, notifications);
                }
            }
            Err(err) => notifications.error(format!("Failed to list heartbeat jobs: {err}")),
        }
    }

    fn load_runs(&mut self, heartbeat_id: &str, notifications: &mut NotificationCenter) {
        let heartbeat_id = heartbeat_id.to_string();
        let heartbeat_id_for_query = heartbeat_id.clone();
        match run_heartbeat_task(move |manager| async move {
            manager.list_runs(&heartbeat_id_for_query, 30, 0).await
        }) {
            Ok(runs) => {
                self.runs_heartbeat_id = Some(heartbeat_id);
                self.runs = runs;
            }
            Err(err) => notifications.error(format!("Failed to load heartbeat runs: {err}")),
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
        let Some(form) = self.form.as_ref() else {
            notifications.error("Heartbeat form is not available");
            return;
        };

        let input = form.to_input();
        if input.id.as_deref().is_some_and(|id| id.is_empty()) {
            notifications.error("Heartbeat ID cannot be empty");
            return;
        }
        if input.session_key.is_empty() {
            notifications.error("Session key cannot be empty");
            return;
        }
        if input.channel.is_empty() {
            notifications.error("Channel cannot be empty");
            return;
        }
        if input.chat_id.is_empty() {
            notifications.error("Chat ID cannot be empty");
            return;
        }
        if input.every.is_empty() {
            notifications.error("Every cannot be empty");
            return;
        }
        if input.silent_ack_token.is_empty() {
            notifications.error("Silent Ack Token cannot be empty");
            return;
        }
        if input.recent_messages_limit <= 0 {
            notifications.error("Recent Messages must be greater than zero");
            return;
        }
        if input.timezone.is_empty() {
            notifications.error("Timezone cannot be empty");
            return;
        }

        if let Some(original_id) = &form.original_id {
            let original_id = original_id.clone();
            let input = input.clone();
            match run_heartbeat_task(move |manager| async move {
                manager.update_job(&original_id, &input).await
            }) {
                Ok(_) => {
                    notifications.success("Heartbeat job updated");
                    self.form = None;
                    self.refresh_jobs(notifications);
                }
                Err(err) => notifications.error(format!("Failed to update heartbeat job: {err}")),
            }
            return;
        }

        match run_heartbeat_task(move |manager| async move { manager.create_job(&input).await }) {
            Ok(_) => {
                notifications.success("Heartbeat job created");
                self.form = None;
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to create heartbeat job: {err}")),
        }
    }

    fn set_enabled(
        &mut self,
        heartbeat_id: &str,
        enabled: bool,
        notifications: &mut NotificationCenter,
    ) {
        let heartbeat_id = heartbeat_id.to_string();
        match run_heartbeat_task(move |manager| async move {
            manager.set_enabled(&heartbeat_id, enabled).await
        }) {
            Ok(()) => {
                notifications.success(if enabled {
                    "Heartbeat enabled"
                } else {
                    "Heartbeat disabled"
                });
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to set enabled: {err}")),
        }
    }

    fn delete_heartbeat(&mut self, heartbeat_id: &str, notifications: &mut NotificationCenter) {
        let heartbeat_id = heartbeat_id.to_string();
        let heartbeat_id_for_delete = heartbeat_id.clone();
        match run_heartbeat_task(move |manager| async move {
            manager.delete_job(&heartbeat_id_for_delete).await
        }) {
            Ok(()) => {
                notifications.success("Heartbeat job deleted");
                if self.runs_heartbeat_id.as_deref() == Some(heartbeat_id.as_str()) {
                    self.runs_heartbeat_id = None;
                    self.runs.clear();
                }
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to delete heartbeat job: {err}")),
        }
    }

    fn run_heartbeat_now(&mut self, heartbeat_id: &str, notifications: &mut NotificationCenter) {
        match request_run_heartbeat_now(heartbeat_id) {
            Ok(message_id) => {
                notifications.success(format!("Heartbeat executed: {message_id}"));
                self.refresh_jobs(notifications);
                self.load_runs(heartbeat_id, notifications);
            }
            Err(err) => notifications.error(format!("Failed to run heartbeat now: {err}")),
        }
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
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
                        ui.label("ID");
                        if form.original_id.is_some() {
                            ui.add_enabled(false, egui::TextEdit::singleline(&mut form.id));
                        } else {
                            ui.text_edit_singleline(&mut form.id);
                        }
                        ui.end_row();

                        ui.label("Session Key");
                        egui::ComboBox::from_id_salt("heartbeat-session-key")
                            .selected_text(if form.session_key.trim().is_empty() {
                                "Select a session"
                            } else {
                                form.session_key.as_str()
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

                        ui.label("Channel");
                        ui.add_enabled(false, egui::TextEdit::singleline(&mut form.channel));
                        ui.end_row();

                        ui.label("Chat ID");
                        ui.add_enabled(false, egui::TextEdit::singleline(&mut form.chat_id));
                        ui.end_row();

                        ui.label("Enabled");
                        ui.checkbox(&mut form.enabled, "");
                        ui.end_row();

                        ui.label("Every");
                        ui.text_edit_singleline(&mut form.every);
                        ui.end_row();

                        ui.label("Timezone");
                        ui.text_edit_singleline(&mut form.timezone);
                        ui.end_row();

                        ui.label("Silent Ack Token");
                        ui.text_edit_singleline(&mut form.silent_ack_token);
                        ui.end_row();

                        ui.label("Recent Messages");
                        ui.add(
                            egui::DragValue::new(&mut form.recent_messages_limit).range(1..=10_000),
                        );
                        ui.end_row();
                    });

                if self.sessions.is_empty() {
                    ui.add_space(8.0);
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "No indexed sessions found. Heartbeat must target an existing session.",
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
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
        let Some(heartbeat_id) = self.runs_heartbeat_id.clone() else {
            return;
        };

        let mut keep_open = true;
        egui::Window::new(format!("Heartbeat Runs: {heartbeat_id}"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .open(&mut keep_open)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(820.0);
                ui.horizontal(|ui| {
                    if ui.button("Refresh Runs").clicked() {
                        self.load_runs(&heartbeat_id, notifications);
                    }
                    if ui.button("Run Now").clicked() {
                        self.run_heartbeat_now(&heartbeat_id, notifications);
                    }
                });

                ui.separator();

                if self.runs.is_empty() {
                    ui.label("No heartbeat runs found.");
                    return;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("heartbeat-run-grid")
                        .striped(true)
                        .num_columns(6)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.strong("Run ID");
                            ui.strong("Status");
                            ui.strong("Scheduled At");
                            ui.strong("Started At");
                            ui.strong("Finished At");
                            ui.strong("Error");
                            ui.end_row();

                            for run in &self.runs {
                                let (icon, color, text) = status_display(run.status);
                                ui.label(&run.id);
                                ui.label(
                                    egui::RichText::new(format!("{icon} {text}"))
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
        self.ensure_loaded(notifications);

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_sessions(notifications);
                self.refresh_jobs(notifications);
            }
            if ui.button("Add Heartbeat Job").clicked() {
                self.open_add_form();
            }
            if ui.button(format!("{} Config", regular::GEAR)).clicked() {
                self.config_window = true;
            }
            ui.label(format!("Jobs: {}", self.jobs.len()));
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("start date");
            render_date_picker(ui, &mut self.start_date, "heartbeat-start-date");
            ui.label("end date");
            render_date_picker(ui, &mut self.end_date, "heartbeat-end-date");
        });
        ui.horizontal(|ui| {
            ui.label("page");
            ui.add_sized(
                [50.0, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut self.page).range(1..=i64::MAX),
            );
            ui.label("size");
            ui.add_sized(
                [50.0, ui.spacing().interact_size.y],
                egui::DragValue::new(&mut self.size).range(1..=1000),
            );
        });

        ui.separator();

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.jobs.is_empty() {
                    ui.label("No heartbeat jobs found in database.");
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
                                ui.strong("ID");
                            });
                            header.col(|ui| {
                                ui.strong("Session");
                            });
                            header.col(|ui| {
                                ui.strong("Channel");
                            });
                            header.col(|ui| {
                                ui.strong("Enabled");
                            });
                            header.col(|ui| {
                                ui.strong("Every");
                            });
                            header.col(|ui| {
                                ui.strong("Recent Msgs");
                            });
                            header.col(|ui| {
                                ui.strong("Next Run At");
                            });
                            header.col(|ui| {
                                ui.strong("Last Run At");
                            });
                            header.col(|ui| {
                                ui.strong("Updated At");
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
                                    ui.label(if job.enabled { "yes" } else { "no" });
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

                                response.context_menu(|ui| {
                                    if ui.button(format!("{} Runs", regular::LIST)).clicked() {
                                        runs_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    if ui.button(format!("{} Run Now", regular::PLAY)).clicked() {
                                        run_now_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                        .clicked()
                                    {
                                        edit_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    let toggle_text = if job.enabled {
                                        format!("{} Disable", regular::POWER)
                                    } else {
                                        format!("{} Enable", regular::POWER)
                                    };
                                    if ui.button(toggle_text).clicked() {
                                        toggle_heartbeat = Some((job.id.clone(), !job.enabled));
                                        ui.close();
                                    }
                                    if ui
                                        .button(
                                            RichText::new(format!("{} Delete", regular::TRASH))
                                                .color(ui.visuals().warn_fg_color),
                                        )
                                        .clicked()
                                    {
                                        delete_heartbeat_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button(format!("{} Copy ID", regular::COPY)).clicked() {
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
            egui::Window::new("Delete heartbeat job")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Delete heartbeat job '{heartbeat_id}'?"));
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            self.delete_heartbeat(&heartbeat_id, notifications);
                            self.delete_confirm_id = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.delete_confirm_id = None;
                        }
                    });
                });
        }

        self.render_runs_window(ui, notifications);
        self.render_form_window(ui, notifications);

        if self.config_window {
            egui::Window::new("Heartbeat Config")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .open(&mut self.config_window)
                .show(ui.ctx(), |ui| {
                    ui.label("Form Defaults");
                    ui.add_space(6.0);
                    ui.checkbox(&mut self.defaults.enabled, "Enabled by default");
                    ui.horizontal(|ui| {
                        ui.label("Recent messages");
                        ui.add(
                            egui::DragValue::new(&mut self.defaults.recent_messages_limit)
                                .range(1..=10_000),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("Only the default enabled state and recent-message window are kept locally in the GUI.\nOther heartbeat fields use built-in defaults.").small().weak());
                });
        }
    }
}

fn status_display(status: HeartbeatTaskStatus) -> (&'static str, Color32, &'static str) {
    match status {
        HeartbeatTaskStatus::Pending => ("◷", Color32::from_rgb(140, 140, 140), "pending"),
        HeartbeatTaskStatus::Running => ("◑", Color32::from_rgb(70, 130, 200), "running"),
        HeartbeatTaskStatus::Success => ("✓", Color32::from_rgb(50, 180, 80), "success"),
        HeartbeatTaskStatus::Failed => ("✗", Color32::from_rgb(220, 60, 60), "failed"),
    }
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

fn render_date_picker(ui: &mut egui::Ui, value: &mut Option<NaiveDate>, id: &str) {
    ui.horizontal(|ui| {
        if let Some(date) = value.as_mut() {
            ui.add(DatePickerButton::new(date).id_salt(id).format("%Y/%m/%d"));
            if ui.small_button("×").clicked() {
                *value = None;
            }
        }
    });
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
