use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_cron::{
    CronError, CronJob, CronListQuery, CronScheduleKind, CronTaskRun, NewCronJob,
    SqliteCronManager, UpdateCronJobPatch,
};
use std::future::Future;
use std::thread;
use tokio::runtime::Builder;
use uuid::Uuid;

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
            timezone: "UTC".to_string(),
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
    selected_cron_id: Option<String>,
    runs: Vec<CronTaskRun>,
    form: Option<CronForm>,
    delete_confirm_id: Option<String>,
}

impl CronPanel {
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
                if let Some(id) = self.selected_cron_id.clone() {
                    self.load_runs(&id, notifications);
                }
            }
            Err(err) => notifications.error(format!("Failed to list cron jobs: {err}")),
        }
    }

    fn load_runs(&mut self, cron_id: &str, notifications: &mut NotificationCenter) {
        let cron_id = cron_id.to_string();
        match run_cron_task(move |manager| async move { manager.list_runs(&cron_id, 30, 0).await })
        {
            Ok(runs) => {
                self.selected_cron_id = Some(cron_id);
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
        if serde_json::from_str::<serde_json::Value>(&payload_json).is_err() {
            notifications.error("Payload JSON is invalid");
            return;
        }

        let timezone = form.timezone.trim().to_string();
        if timezone.is_empty() {
            notifications.error("Timezone cannot be empty");
            return;
        }

        let kind = form.schedule_kind;
        let next_run_at_ms = match SqliteCronManager::compute_next_run_at_ms(kind, &expr) {
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

    fn set_enabled(&mut self, cron_id: &str, enabled: bool, notifications: &mut NotificationCenter) {
        let cron_id = cron_id.to_string();
        match run_cron_task(move |manager| async move {
            manager.set_enabled(&cron_id, enabled).await
        }) {
            Ok(()) => {
                notifications.success(if enabled { "Cron enabled" } else { "Cron disabled" });
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to set enabled: {err}")),
        }
    }

    fn delete_cron(&mut self, cron_id: &str, notifications: &mut NotificationCenter) {
        let cron_id = cron_id.to_string();
        match run_cron_task(move |manager| async move { manager.delete_job(&cron_id).await }) {
            Ok(()) => {
                notifications.success("Cron job deleted");
                if self.selected_cron_id.as_deref() == Some(cron_id.as_str()) {
                    self.selected_cron_id = None;
                    self.runs.clear();
                }
                self.refresh_jobs(notifications);
            }
            Err(err) => notifications.error(format!("Failed to delete cron job: {err}")),
        }
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;
        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new(form.title())
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
}

impl PanelRenderer for CronPanel {
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
                self.refresh_jobs(notifications);
            }
            if ui.button("Add Cron Job").clicked() {
                self.open_add_form();
            }
        });

        ui.separator();
        ui.label(format!("Jobs: {}", self.jobs.len()));

        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.jobs.is_empty() {
                ui.label("No cron jobs found in database.");
            } else {
                egui::Grid::new("cron-list-grid")
                    .striped(true)
                    .num_columns(9)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("ID");
                        ui.strong("Name");
                        ui.strong("Kind");
                        ui.strong("Expr");
                        ui.strong("Enabled");
                        ui.strong("Next Run(ms)");
                        ui.strong("Last Run(ms)");
                        ui.strong("Updated(ms)");
                        ui.strong("Actions");
                        ui.end_row();

                        let jobs = self.jobs.clone();
                        for job in jobs {
                            ui.label(job.id.clone());
                            ui.label(job.name.clone());
                            ui.label(match job.schedule_kind {
                                CronScheduleKind::Cron => "cron",
                                CronScheduleKind::Every => "every",
                            });
                            ui.label(job.schedule_expr.clone());
                            ui.label(if job.enabled { "yes" } else { "no" });
                            ui.label(job.next_run_at_ms.to_string());
                            ui.label(job.last_run_at_ms.map(|v| v.to_string()).unwrap_or_default());
                            ui.label(job.updated_at_ms.to_string());

                            ui.horizontal(|ui| {
                                if ui.button("Runs").clicked() {
                                    self.load_runs(&job.id, notifications);
                                }
                                if ui.button("Edit").clicked() {
                                    self.open_edit_form(&job.id);
                                }
                                let toggle_text = if job.enabled { "Disable" } else { "Enable" };
                                if ui.button(toggle_text).clicked() {
                                    self.set_enabled(&job.id, !job.enabled, notifications);
                                }
                                if ui.button("Delete").clicked() {
                                    self.delete_confirm_id = Some(job.id.clone());
                                }
                            });
                            ui.end_row();
                        }
                    });
            }
        });

        if let Some(cron_id) = self.selected_cron_id.clone() {
            ui.separator();
            ui.heading(format!("Task Runs: {cron_id}"));
            if self.runs.is_empty() {
                ui.label("No task runs found.");
            } else {
                egui::Grid::new("cron-run-grid")
                    .striped(true)
                    .num_columns(6)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("Run ID");
                        ui.strong("Status");
                        ui.strong("Scheduled(ms)");
                        ui.strong("Started(ms)");
                        ui.strong("Finished(ms)");
                        ui.strong("Error");
                        ui.end_row();

                        for run in &self.runs {
                            ui.label(&run.id);
                            ui.label(run.status.as_str());
                            ui.label(run.scheduled_at_ms.to_string());
                            ui.label(run.started_at_ms.map(|v| v.to_string()).unwrap_or_default());
                            ui.label(run.finished_at_ms.map(|v| v.to_string()).unwrap_or_default());
                            ui.label(run.error_message.clone().unwrap_or_default());
                            ui.end_row();
                        }
                    });
            }
        }

        if let Some(cron_id) = self.delete_confirm_id.clone() {
            egui::Window::new("Delete cron job")
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

        self.render_form_window(ui, notifications);
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
