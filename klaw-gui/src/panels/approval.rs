use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_approval::{
    ApprovalListQuery, ApprovalManager, ApprovalResolveDecision, ApprovalStatus,
    SqliteApprovalManager,
};
use klaw_storage::ApprovalRecord;
use std::future::Future;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 220.0;
const PAGING_INPUT_WIDTH: f32 = 50.0;

#[derive(Default)]
pub struct ApprovalPanel {
    loaded: bool,
    approvals: Vec<ApprovalRecord>,
    session_key_filter: String,
    tool_name_filter: String,
    status_filter: Option<ApprovalStatus>,
    page: i64,
    size: i64,
    selected_approval: Option<String>,
    view_approval: Option<ApprovalRecord>,
}

impl ApprovalPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        if self.size == 0 {
            self.size = 100;
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let size = self.size.max(1);
        let page = self.page.max(1);
        let offset = (page - 1) * size;
        let query = ApprovalListQuery {
            session_key: optional_trimmed(&self.session_key_filter),
            tool_name: optional_trimmed(&self.tool_name_filter),
            status: self.status_filter,
            limit: size,
            offset,
        };

        match run_approval_task(move |manager| async move { manager.list_approvals(query).await }) {
            Ok(approvals) => {
                self.approvals = approvals;
                self.loaded = true;
            }
            Err(err) => notifications.error(format!("Failed to load approvals: {err}")),
        }
    }

    fn resolve(
        &mut self,
        approval_id: &str,
        decision: ApprovalResolveDecision,
        notifications: &mut NotificationCenter,
    ) {
        let approval_id = approval_id.to_string();
        match run_approval_task(move |manager| async move {
            manager
                .resolve_approval(&approval_id, decision, Some("gui-user"), now_ms())
                .await
        }) {
            Ok(outcome) => {
                if outcome.updated {
                    notifications.success(format!("Approval {} updated", outcome.approval.id));
                }
                self.refresh(notifications);
            }
            Err(err) => notifications.error(format!("Failed to update approval: {err}")),
        }
    }

    fn consume(&mut self, approval_id: &str, notifications: &mut NotificationCenter) {
        let approval_id = approval_id.to_string();
        match run_approval_task(move |manager| async move {
            manager.consume_approval(&approval_id, now_ms()).await
        }) {
            Ok(outcome) => {
                if outcome.updated {
                    notifications.success(format!("Approval {} consumed", outcome.approval.id));
                } else {
                    notifications
                        .error(format!("Approval {} was not consumed", outcome.approval.id));
                }
                self.refresh(notifications);
            }
            Err(err) => notifications.error(format!("Failed to consume approval: {err}")),
        }
    }
}

impl PanelRenderer for ApprovalPanel {
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
            ui.label(format!("Approvals: {}", self.approvals.len()));
        });

        ui.separator();
        let mut need_refresh = false;
        ui.horizontal(|ui| {
            ui.label("session_key");
            if ui
                .add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.session_key_filter),
                )
                .changed()
            {
                need_refresh = true;
            }
            ui.label("tool_name");
            if ui
                .add_sized(
                    [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                    egui::TextEdit::singleline(&mut self.tool_name_filter),
                )
                .changed()
            {
                need_refresh = true;
            }
            ui.label("status");
            let combo_resp = egui::ComboBox::from_id_salt("status_filter")
                .selected_text(self.status_filter.map_or("All", |s| s.as_str()))
                .show_ui(ui, |ui| {
                    let mut changed = false;
                    if ui
                        .selectable_value(&mut self.status_filter, None, "All")
                        .changed()
                    {
                        changed = true;
                    }
                    for status in [
                        ApprovalStatus::Pending,
                        ApprovalStatus::Approved,
                        ApprovalStatus::Rejected,
                        ApprovalStatus::Expired,
                        ApprovalStatus::Consumed,
                    ] {
                        if ui
                            .selectable_value(
                                &mut self.status_filter,
                                Some(status),
                                status.as_str(),
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
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .max_width(table_width)
            .show(ui, |ui| {
                ui.set_min_width(table_width);
                if self.approvals.is_empty() {
                    ui.label("No approvals found.");
                    return;
                }

                let available_height = ui.available_height();
                let mut approve_id: Option<String> = None;
                let mut reject_id: Option<String> = None;
                let mut consume_id: Option<String> = None;

                let mut view_id: Option<String> = None;

                TableBuilder::new(ui)
                    .striped(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(60.0))
                    .column(Column::auto().at_least(70.0))
                    .column(Column::auto().at_least(100.0))
                    .column(Column::auto().at_least(80.0))
                    .column(Column::auto().at_least(120.0))
                    .column(Column::remainder().at_least(150.0))
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
                            ui.strong("Tool");
                        });
                        header.col(|ui| {
                            ui.strong("Risk");
                        });
                        header.col(|ui| {
                            ui.strong("Status");
                        });
                        header.col(|ui| {
                            ui.strong("Requested By");
                        });
                        header.col(|ui| {
                            ui.strong("Approved By");
                        });
                        header.col(|ui| {
                            ui.strong("Expires At");
                        });
                        header.col(|ui| {
                            ui.strong("Preview");
                        });
                    })
                    .body(|body| {
                        body.rows(20.0, self.approvals.len(), |mut row| {
                            let idx = row.index();
                            let approval = &self.approvals[idx];
                            let is_selected =
                                self.selected_approval.as_deref() == Some(&approval.id);

                            row.set_selected(is_selected);

                            row.col(|ui| {
                                ui.label(&approval.id);
                            });
                            row.col(|ui| {
                                ui.label(&approval.session_key);
                            });
                            row.col(|ui| {
                                ui.label(&approval.tool_name);
                            });
                            row.col(|ui| {
                                ui.label(&approval.risk_level);
                            });
                            row.col(|ui| {
                                let (icon, color, text) = approval_status_display(approval.status);
                                ui.label(
                                    RichText::new(format!("{icon} {text}"))
                                        .color(color)
                                        .strong(),
                                );
                            });
                            row.col(|ui| {
                                ui.label(&approval.requested_by);
                            });
                            row.col(|ui| {
                                ui.label(approval.approved_by.as_deref().unwrap_or(""));
                            });
                            row.col(|ui| {
                                ui.label(format_timestamp_millis(approval.expires_at_ms));
                            });
                            row.col(|ui| {
                                let preview = truncate_preview(&approval.command_preview);
                                ui.label(preview);
                            });

                            let response = row.response();

                            if response.clicked() {
                                self.selected_approval = if is_selected {
                                    None
                                } else {
                                    Some(approval.id.clone())
                                };
                            }

                            response.context_menu(|ui| {
                                if ui.button(format!("{} View", regular::EYE)).clicked() {
                                    view_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .button(format!("{} Approve", regular::CHECK_CIRCLE))
                                    .clicked()
                                {
                                    approve_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                if ui.button(format!("{} Reject", regular::X_CIRCLE)).clicked() {
                                    reject_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                if ui
                                    .button(format!("{} Consume", regular::LIGHTNING))
                                    .clicked()
                                {
                                    consume_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button(format!("{} Copy ID", regular::COPY)).clicked() {
                                    ui.ctx().output_mut(|o| {
                                        o.commands.push(egui::OutputCommand::CopyText(
                                            approval.id.clone(),
                                        ));
                                    });
                                    ui.close();
                                }
                            });
                        });
                    });

                if let Some(id) = approve_id {
                    self.resolve(&id, ApprovalResolveDecision::Approve, notifications);
                }
                if let Some(id) = reject_id {
                    self.resolve(&id, ApprovalResolveDecision::Reject, notifications);
                }
                if let Some(id) = consume_id {
                    self.consume(&id, notifications);
                }
                if let Some(id) = view_id {
                    self.view_approval = self.approvals.iter().find(|a| a.id == id).cloned();
                }
            });

        if let Some(ref approval) = self.view_approval {
            let mut open = true;
            egui::Window::new(format!("Approval: {}", approval.id))
                .open(&mut open)
                .resizable(true)
                .default_size([500.0, 400.0])
                .show(ui.ctx(), |ui| {
                    egui::Grid::new("approval-detail-grid")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("ID:");
                            ui.label(&approval.id);
                            ui.end_row();

                            ui.label("Session:");
                            ui.label(&approval.session_key);
                            ui.end_row();

                            ui.label("Tool:");
                            ui.label(&approval.tool_name);
                            ui.end_row();

                            ui.label("Risk Level:");
                            ui.label(&approval.risk_level);
                            ui.end_row();

                            ui.label("Status:");
                            let (icon, color, text) = approval_status_display(approval.status);
                            ui.label(
                                RichText::new(format!("{icon} {text}"))
                                    .color(color)
                                    .strong(),
                            );
                            ui.end_row();

                            ui.label("Requested By:");
                            ui.label(&approval.requested_by);
                            ui.end_row();

                            ui.label("Approved By:");
                            ui.label(approval.approved_by.as_deref().unwrap_or("-"));
                            ui.end_row();

                            ui.label("Justification:");
                            ui.label(approval.justification.as_deref().unwrap_or("-"));
                            ui.end_row();

                            ui.label("Expires At:");
                            ui.label(format_timestamp_millis(approval.expires_at_ms));
                            ui.end_row();

                            ui.label("Created At:");
                            ui.label(format_timestamp_millis(approval.created_at_ms));
                            ui.end_row();

                            ui.label("Updated At:");
                            ui.label(format_timestamp_millis(approval.updated_at_ms));
                            ui.end_row();

                            ui.label("Consumed At:");
                            ui.label(
                                approval
                                    .consumed_at_ms
                                    .map(format_timestamp_millis)
                                    .as_deref()
                                    .unwrap_or("-"),
                            );
                            ui.end_row();
                        });

                    ui.separator();
                    ui.label("Command Preview:");
                    egui::ScrollArea::vertical()
                        .max_height(100.0)
                        .id_salt("approval_command_preview")
                        .show(ui, |ui| {
                            ui.label(&approval.command_preview);
                        });

                    ui.separator();
                    ui.label("Command Text:");
                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .id_salt("approval_command_text")
                        .show(ui, |ui| {
                            ui.label(&approval.command_text);
                        });
                });
            if !open {
                self.view_approval = None;
            }
        }
    }
}

fn run_approval_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Box<dyn ApprovalManager>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, klaw_approval::ApprovalError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;

        runtime.block_on(async move {
            let manager: Box<dyn ApprovalManager> = Box::new(
                SqliteApprovalManager::open_default()
                    .await
                    .map_err(|err| format!("failed to open approval manager: {err}"))?,
            );
            op(manager)
                .await
                .map_err(|err| format!("approval operation failed: {err}"))
        })
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("approval operation thread panicked".to_string()),
    }
}

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn truncate_preview(text: &str) -> String {
    let max_len = 50;
    if let Some(pos) = text.find('\n') {
        let line = &text[..pos];
        let chars: String = line.chars().take(max_len).collect();
        if line.chars().count() > max_len {
            format!("{}...", chars)
        } else {
            chars
        }
    } else if text.chars().count() > max_len {
        let chars: String = text.chars().take(max_len).collect();
        format!("{}...", chars)
    } else {
        text.to_string()
    }
}

fn approval_status_display(status: ApprovalStatus) -> (&'static str, Color32, &'static str) {
    match status {
        ApprovalStatus::Pending => (
            regular::HOURGLASS_MEDIUM,
            Color32::from_rgb(200, 150, 50),
            "pending",
        ),
        ApprovalStatus::Approved => (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(50, 180, 80),
            "approved",
        ),
        ApprovalStatus::Rejected => (
            regular::X_CIRCLE,
            Color32::from_rgb(220, 60, 60),
            "rejected",
        ),
        ApprovalStatus::Expired => (regular::CLOCK, Color32::from_rgb(140, 140, 140), "expired"),
        ApprovalStatus::Consumed => (
            regular::LIGHTNING,
            Color32::from_rgb(70, 130, 200),
            "consumed",
        ),
    }
}
