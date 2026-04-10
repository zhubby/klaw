use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::{format_optional_timestamp_millis, format_timestamp_millis};
use crate::{RuntimeRequestHandle, begin_run_cron_now_request};
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_cron::{
    CronError, CronJob, CronListQuery, CronScheduleKind, CronTaskRun, NewCronJob,
    SqliteCronManager, UpdateCronJobPatch,
};
use klaw_storage::CronTaskStatus;
use klaw_util::system_timezone_name;
use std::future::Future;
use std::thread;
use std::time::Duration;
use tokio::runtime::Builder;
use uuid::Uuid;

const CRON_RUNS_WINDOW_WIDTH: f32 = 760.0;

#[derive(Debug, Clone)]
struct CronForm {
    original_id: Option<String>,
    id: String,
    name: String,
    schedule_kind: CronScheduleKind,
    schedule_expr: String,
    payload_json: String,
    timezone: String,
    enabled: bool,
}

impl CronForm {
    fn new() -> Self {
        Self {
            original_id: None,
            id: Uuid::new_v4().to_string(),
            name: String::new(),
            schedule_kind: CronScheduleKind::Every,
            schedule_expr: "5m".to_string(),
            payload_json: "{}".to_string(),
            timezone: system_timezone_name(),
            enabled: true,
        }
    }

    fn edit(item: &CronJob) -> Self {
        Self {
            original_id: Some(item.id.clone()),
            id: item.id.clone(),
            name: item.name.clone(),
            schedule_kind: item.schedule_kind,
            schedule_expr: item.schedule_expr.clone(),
            payload_json: item.payload_json.clone(),
            timezone: item.timezone.clone(),
            enabled: item.enabled,
        }
    }

    fn title(&self) -> &'static str {
        if self.original_id.is_some() {
            "Edit Cron Job"
        } else {
            "Add Cron Job"
        }
    }
}

#[derive(Default)]
pub struct CronPanel {
    loaded: bool,
    jobs: Vec<CronJob>,
    runs_cron_id: Option<String>,
    runs: Vec<CronTaskRun>,
    form: Option<CronForm>,
    delete_confirm_id: Option<String>,
    selected_cron: Option<String>,
    run_now_request: Option<RuntimeRequestHandle<String>>,
    pending_run_now_cron_id: Option<String>,
}

impl CronPanel {
    fn poll_run_now_request(&mut self, notifications: &mut NotificationCenter) {
        let Some(request) = self.run_now_request.as_mut() else {
            return;
        };
        let Some(result) = request.try_take_result() else {
            return;
        };

        let cron_id = self.pending_run_now_cron_id.take();
        self.run_now_request = None;
        match result {
            Ok(message_id) => {
                notifications.success(format!("Cron executed: {message_id}"));
                self.refresh_jobs(notifications);
                if let Some(cron_id) = cron_id {
                    self.load_runs(&cron_id, notifications);
                }
            }
            Err(err) => notifications.error(format!("Failed to run cron now: {err}")),
        }
    }

    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.refresh_jobs(notifications);
    }

    fn refresh_jobs(&mut self, notifications: &mut NotificationCenter) {
        let query = CronListQuery {
            limit: 200,
            offset: 0,
        };

        match run_cron_task(move |manager| async move { manager.list_jobs(query).await }) {
            Ok(jobs) => {
                self.jobs = jobs;
                self.loaded = true;
                if let Some(id) = self.runs_cron_id.clone() {
                    self.load_runs(&id, notifications);
                }
            }
            Err(err) => notifications.error(format!("Failed to list cron jobs: {err}")),
        }
    }

    fn load_runs(&mut self, cron_id: &str, notifications: &mut NotificationCenter) {
        let cron_id = cron_id.to_string();
        let cron_id_for_query = cron_id.clone();
        match run_cron_task(move |manager| async move {
            manager.list_runs(&cron_id_for_query, 30, 0).await
        }) {
            Ok(runs) => {
                self.runs_cron_id = Some(cron_id);
                self.runs = runs;
            }
            Err(err) => notifications.error(format!("Failed to load task runs: {err}")),
        }
    }

    fn open_add_form(&mut self) {
        self.form = Some(CronForm::new());
    }

    fn open_edit_form(&mut self, cron_id: &str) {
        if let Some(item) = self.jobs.iter().find(|job| job.id == cron_id) {
            self.form = Some(CronForm::edit(item));
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.as_ref() else {
            notifications.error("Cron form is not available");
            return;
        };

        let id = form.id.trim().to_string();
        if id.is_empty() {
            notifications.error("Cron ID cannot be empty");
            return;
        }

        let name = form.name.trim().to_string();
        if name.is_empty() {
            notifications.error("Cron name cannot be empty");
            return;
        }

        let expr = form.schedule_expr.trim().to_string();
        if expr.is_empty() {
            notifications.error("Schedule expression cannot be empty");
            return;
        }

        let payload_json = form.payload_json.trim().to_string();
        if payload_json.is_empty() {
            notifications.error("Payload JSON cannot be empty");
            return;
        }
        let payload_value = match serde_json::from_str::<serde_json::Value>(&payload_json) {
            Ok(value) => value,
            Err(err) => {
                notifications.error(format!("Payload JSON is invalid: {err}"));
                return;
            }
        };
        if let Err(err) = validate_inbound_payload_value(&payload_value) {
            notifications.error(format!(
                "Payload JSON must be a valid InboundMessage-like object: {err}"
            ));
            return;
        }

        let timezone = form.timezone.trim().to_string();
        if timezone.is_empty() {
            notifications.error("Timezone cannot be empty");
            return;
        }

        let kind = form.schedule_kind;
        let next_run_at_ms = match SqliteCronManager::compute_next_run_at_ms(kind, &expr, &timezone)
        {
            Ok(next) => next,
            Err(err) => {
                notifications.error(format!("Invalid schedule: {err}"));
                return;
            }
        };

        if let Some(original_id) = &form.original_id {
            let original_id = original_id.clone();
            let enabled = form.enabled;
            let patch = UpdateCronJobPatch {
                name: Some(name),
                schedule_kind: Some(kind),
                schedule_expr: Some(expr),
                payload_json: Some(payload_json),
                timezone: Some(timezone),
                next_run_at_ms: Some(next_run_at_ms),
            };

            let result = run_cron_task(move |manager| async move {
                let updated = manager.update_job(&original_id, &patch).await?;
                if updated.enabled != enabled {
                    manager.set_enabled(&updated.id, enabled).await?;
                }
                Ok(())
            });

            match result {
                Ok(()) => {
                    notifications.success("Cron job updated");
                    self.form = None;
                    self.refresh_jobs(notifications);
                }
                Err(err) => notifications.error(format!("Failed to update cron job: {err}")),
            }
            return;
        }

        let input = NewCronJob {
            id,
            name,
            schedule_kind: kind,
            schedule_expr: expr,
            payload_json,
            enabled: form.enabled,
            timezone,
            next_run_at_ms,
        };

        match run_cron_task(move |manager| async move { manager.create_job(&input).await }) {
            Ok(_) => {
                notifications.success("Cron job created");
                self.form = None;
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to create cron job: {err}")),
        }
    }

    fn set_enabled(
        &mut self,
        cron_id: &str,
        enabled: bool,
        notifications: &mut NotificationCenter,
    ) {
        let cron_id = cron_id.to_string();
        match run_cron_task(
            move |manager| async move { manager.set_enabled(&cron_id, enabled).await },
        ) {
            Ok(()) => {
                notifications.success(if enabled {
                    "Cron enabled"
                } else {
                    "Cron disabled"
                });
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to set enabled: {err}")),
        }
    }

    fn delete_cron(&mut self, cron_id: &str, notifications: &mut NotificationCenter) {
        let cron_id = cron_id.to_string();
        let cron_id_for_delete = cron_id.clone();
        match run_cron_task(
            move |manager| async move { manager.delete_job(&cron_id_for_delete).await },
        ) {
            Ok(()) => {
                notifications.success("Cron job deleted");
                if self.runs_cron_id.as_deref() == Some(cron_id.as_str()) {
                    self.runs_cron_id = None;
                    self.runs.clear();
                }
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to delete cron job: {err}")),
        }
    }

    fn run_cron_now(&mut self, cron_id: &str, notifications: &mut NotificationCenter) {
        if self.run_now_request.is_some() {
            notifications.info("A cron run is already in progress");
            return;
        }
        self.pending_run_now_cron_id = Some(cron_id.to_string());
        self.run_now_request = Some(begin_run_cron_now_request(cron_id.to_string()));
        notifications.info(format!("Running cron '{cron_id}' in background..."));
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
                ui.set_min_width(560.0);
                egui::Grid::new("cron-form-grid")
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

                        ui.label("Name");
                        ui.text_edit_singleline(&mut form.name);
                        ui.end_row();

                        ui.label("Schedule Kind");
                        egui::ComboBox::from_id_salt("cron-schedule-kind")
                            .selected_text(match form.schedule_kind {
                                CronScheduleKind::Cron => "cron",
                                CronScheduleKind::Every => "every",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut form.schedule_kind,
                                    CronScheduleKind::Cron,
                                    "cron",
                                );
                                ui.selectable_value(
                                    &mut form.schedule_kind,
                                    CronScheduleKind::Every,
                                    "every",
                                );
                            });
                        ui.end_row();

                        ui.label("Schedule Expr");
                        ui.text_edit_singleline(&mut form.schedule_expr);
                        ui.end_row();

                        ui.label("Timezone");
                        ui.text_edit_singleline(&mut form.timezone);
                        ui.end_row();

                        ui.label("Enabled");
                        ui.checkbox(&mut form.enabled, "");
                        ui.end_row();
                    });

                ui.separator();
                ui.label("Payload JSON");
                ui.add(
                    egui::TextEdit::multiline(&mut form.payload_json)
                        .desired_rows(8)
                        .desired_width(f32::INFINITY),
                );

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

        if save_clicked {
            self.save_form(notifications);
        }
        if cancel_clicked {
            self.form = None;
        }
    }
    fn render_runs_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let Some(cron_id) = self.runs_cron_id.clone() else {
            return;
        };

        let mut keep_open = true;
        egui::Window::new(format!("Task Runs: {cron_id}"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .default_width(CRON_RUNS_WINDOW_WIDTH)
            .min_width(CRON_RUNS_WINDOW_WIDTH)
            .max_width(CRON_RUNS_WINDOW_WIDTH)
            .open(&mut keep_open)
            .show(ui.ctx(), |ui| {
                ui.set_width(CRON_RUNS_WINDOW_WIDTH);
                ui.horizontal(|ui| {
                    if ui.button("Refresh Runs").clicked() {
                        self.load_runs(&cron_id, notifications);
                    }
                    if ui
                        .add_enabled(self.run_now_request.is_none(), egui::Button::new("Run Now"))
                        .clicked()
                    {
                        self.run_cron_now(&cron_id, notifications);
                    }
                });

                ui.separator();

                if self.runs.is_empty() {
                    ui.label("No task runs found.");
                    return;
                }

                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("cron-run-grid")
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
                                    let (icon, color, text) = cron_status_display(run.status);
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
            self.runs_cron_id = None;
            self.runs.clear();
        }
    }
}

impl PanelRenderer for CronPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.poll_run_now_request(notifications);
        self.ensure_loaded(notifications);
        if self.run_now_request.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.refresh_jobs(notifications);
            }
            if ui.button("Add Cron Job").clicked() {
                self.open_add_form();
            }
            if let Some(cron_id) = self.pending_run_now_cron_id.as_deref() {
                ui.label(
                    RichText::new(format!("Running: {cron_id}"))
                        .color(Color32::from_rgb(70, 130, 200))
                        .strong(),
                );
            }
        });

        ui.separator();
        ui.label(format!("Jobs: {}", self.jobs.len()));

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.jobs.is_empty() {
                    ui.label("No cron jobs found in database.");
                } else {
                    let available_height = ui.available_height();
                    let mut edit_cron_id: Option<String> = None;
                    let mut toggle_cron: Option<(String, bool)> = None;
                    let mut delete_cron_id: Option<String> = None;
                    let mut runs_cron_id: Option<String> = None;
                    let mut run_now_cron_id: Option<String> = None;

                    TableBuilder::new(ui)
                        .striped(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(100.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(80.0))
                        .column(Column::auto().at_least(60.0))
                        .column(Column::auto().at_least(120.0))
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
                                ui.strong("Name");
                            });
                            header.col(|ui| {
                                ui.strong("Kind");
                            });
                            header.col(|ui| {
                                ui.strong("Expr");
                            });
                            header.col(|ui| {
                                ui.strong("Enabled");
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
                                let is_selected = self.selected_cron.as_deref() == Some(&job.id);

                                row.set_selected(is_selected);

                                row.col(|ui| {
                                    ui.label(job.id.clone());
                                });
                                row.col(|ui| {
                                    ui.label(job.name.clone());
                                });
                                row.col(|ui| {
                                    let (icon, color, label) = cron_kind_display(job.schedule_kind);
                                    ui.label(
                                        RichText::new(format!("{icon} {label}"))
                                            .color(color)
                                            .strong(),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(job.schedule_expr.clone());
                                });
                                row.col(|ui| {
                                    let (icon, color, label) = enabled_display(job.enabled);
                                    ui.label(
                                        RichText::new(format!("{icon} {label}"))
                                            .color(color)
                                            .strong(),
                                    );
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
                                    self.selected_cron = if is_selected {
                                        None
                                    } else {
                                        Some(job.id.clone())
                                    };
                                }

                                response.context_menu(|ui| {
                                    if ui.button(format!("{} Runs", regular::LIST)).clicked() {
                                        runs_cron_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .add_enabled(
                                            self.run_now_request.is_none(),
                                            egui::Button::new(format!("{} Run Now", regular::PLAY)),
                                        )
                                        .clicked()
                                    {
                                        run_now_cron_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    if ui
                                        .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                        .clicked()
                                    {
                                        edit_cron_id = Some(job.id.clone());
                                        ui.close();
                                    }
                                    let toggle_text = if job.enabled {
                                        format!("{} Disable", regular::POWER)
                                    } else {
                                        format!("{} Enable", regular::POWER)
                                    };
                                    if ui.button(toggle_text).clicked() {
                                        toggle_cron = Some((job.id.clone(), !job.enabled));
                                        ui.close();
                                    }
                                    if ui
                                        .button(
                                            RichText::new(format!("{} Delete", regular::TRASH))
                                                .color(ui.visuals().warn_fg_color),
                                        )
                                        .clicked()
                                    {
                                        delete_cron_id = Some(job.id.clone());
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

                    if let Some(id) = runs_cron_id {
                        self.load_runs(&id, notifications);
                    }
                    if let Some(id) = run_now_cron_id {
                        self.run_cron_now(&id, notifications);
                    }
                    if let Some(id) = edit_cron_id {
                        self.open_edit_form(&id);
                    }
                    if let Some((id, enabled)) = toggle_cron {
                        self.set_enabled(&id, enabled, notifications);
                    }
                    if let Some(id) = delete_cron_id {
                        self.delete_confirm_id = Some(id);
                    }
                }
            });

        if let Some(cron_id) = self.delete_confirm_id.clone() {
            egui::Window::new("Delete cron job")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Delete cron job '{cron_id}'?"));
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            self.delete_cron(&cron_id, notifications);
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
    }
}

fn cron_status_display(status: CronTaskStatus) -> (&'static str, Color32, &'static str) {
    match status {
        CronTaskStatus::Pending => ("◷", Color32::from_rgb(140, 140, 140), "pending"),
        CronTaskStatus::Running => ("◑", Color32::from_rgb(70, 130, 200), "running"),
        CronTaskStatus::Success => ("✓", Color32::from_rgb(50, 180, 80), "success"),
        CronTaskStatus::Failed => ("✗", Color32::from_rgb(220, 60, 60), "failed"),
    }
}

fn cron_kind_display(kind: CronScheduleKind) -> (&'static str, Color32, &'static str) {
    match kind {
        CronScheduleKind::Cron => ("C", Color32::from_rgb(0xF5, 0x9E, 0x0B), "cron"),
        CronScheduleKind::Every => ("E", Color32::from_rgb(0x38, 0xBD, 0xF8), "every"),
    }
}

fn enabled_display(enabled: bool) -> (&'static str, Color32, &'static str) {
    if enabled {
        (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            "yes",
        )
    } else {
        (regular::X_CIRCLE, Color32::from_rgb(0xEF, 0x44, 0x44), "no")
    }
}

fn run_cron_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SqliteCronManager) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, CronError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let manager = SqliteCronManager::open_default()
                .await
                .map_err(|err| format!("failed to open cron manager: {err}"))?;
            op(manager)
                .await
                .map_err(|err| format!("cron operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("cron operation thread panicked".to_string()),
    }
}

fn validate_inbound_payload_value(payload: &serde_json::Value) -> Result<(), String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "payload must be a JSON object".to_string())?;

    require_string_field(object, "channel")?;
    require_string_field(object, "sender_id")?;
    require_string_field(object, "chat_id")?;
    require_string_field(object, "session_key")?;
    require_string_field(object, "content")?;

    match object.get("metadata") {
        Some(serde_json::Value::Object(_)) => {}
        Some(_) => return Err("`metadata` must be a JSON object".to_string()),
        None => return Err("missing required field `metadata`".to_string()),
    }

    if let Some(media_references) = object.get("media_references") {
        if !media_references.is_array() {
            return Err("`media_references` must be an array when provided".to_string());
        }
    }

    Ok(())
}

fn require_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<(), String> {
    match object.get(key) {
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => Ok(()),
        Some(serde_json::Value::String(_)) => Err(format!("`{key}` cannot be empty")),
        Some(_) => Err(format!("`{key}` must be a string")),
        None => Err(format!("missing required field `{key}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cron_form_defaults_to_system_timezone() {
        assert_eq!(CronForm::new().timezone, system_timezone_name());
    }

    #[test]
    fn runs_window_uses_fixed_width_and_bidirectional_scroll() {
        let source = include_str!("cron.rs");
        let render_runs_window = source
            .split("fn render_runs_window")
            .nth(1)
            .and_then(|section| section.split("impl PanelRenderer for CronPanel").next())
            .expect("render_runs_window section should exist");

        assert!(render_runs_window.contains(".default_width(CRON_RUNS_WINDOW_WIDTH)"));
        assert!(render_runs_window.contains(".max_width(CRON_RUNS_WINDOW_WIDTH)"));
        assert!(render_runs_window.contains("egui::ScrollArea::both()"));
    }
}
