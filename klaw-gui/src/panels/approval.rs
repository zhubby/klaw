use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::time_format::format_timestamp_millis;
use klaw_approval::{
    ApprovalListQuery, ApprovalManager, ApprovalResolveDecision, ApprovalStatus,
    SqliteApprovalManager,
};
use std::future::Future;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Builder;

#[derive(Default)]
pub struct ApprovalPanel {
    loaded: bool,
    approvals: Vec<klaw_approval::ApprovalRecord>,
    session_key_filter: String,
    tool_name_filter: String,
    status_filter: String,
    limit_text: String,
    offset_text: String,
}

impl ApprovalPanel {
    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        if self.limit_text.is_empty() {
            self.limit_text = "100".to_string();
        }
        self.refresh(notifications);
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let status = match parse_status(&self.status_filter) {
            Ok(status) => status,
            Err(err) => {
                notifications.error(err);
                return;
            }
        };

        let query = ApprovalListQuery {
            session_key: optional_trimmed(&self.session_key_filter),
            tool_name: optional_trimmed(&self.tool_name_filter),
            status,
            limit: self.limit_text.trim().parse::<i64>().unwrap_or(100),
            offset: self.offset_text.trim().parse::<i64>().unwrap_or(0),
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
        egui::Grid::new("approval-filter-grid")
            .num_columns(4)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                ui.label("session_key");
                ui.text_edit_singleline(&mut self.session_key_filter);
                ui.label("tool_name");
                ui.text_edit_singleline(&mut self.tool_name_filter);
                ui.end_row();

                ui.label("status");
                ui.text_edit_singleline(&mut self.status_filter);
                ui.label("limit");
                ui.text_edit_singleline(&mut self.limit_text);
                ui.end_row();

                ui.label("offset");
                ui.text_edit_singleline(&mut self.offset_text);
                ui.end_row();
            });

        if ui.button("Apply").clicked() {
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

                egui::Grid::new("approval-table-grid")
                    .striped(true)
                    .num_columns(10)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("ID");
                        ui.strong("Session");
                        ui.strong("Tool");
                        ui.strong("Risk");
                        ui.strong("Status");
                        ui.strong("Requested By");
                        ui.strong("Approved By");
                        ui.strong("Expires At");
                        ui.strong("Preview");
                        ui.strong("Actions");
                        ui.end_row();

                        let approvals = self.approvals.clone();
                        for approval in approvals {
                            ui.label(&approval.id);
                            ui.label(&approval.session_key);
                            ui.label(&approval.tool_name);
                            ui.label(&approval.risk_level);
                            ui.label(approval.status.as_str());
                            ui.label(&approval.requested_by);
                            ui.label(approval.approved_by.as_deref().unwrap_or(""));
                            ui.label(format_timestamp_millis(approval.expires_at_ms));
                            ui.label(&approval.command_preview);
                            ui.horizontal(|ui| {
                                if ui.small_button("Approve").clicked() {
                                    self.resolve(
                                        &approval.id,
                                        ApprovalResolveDecision::Approve,
                                        notifications,
                                    );
                                }
                                if ui.small_button("Reject").clicked() {
                                    self.resolve(
                                        &approval.id,
                                        ApprovalResolveDecision::Reject,
                                        notifications,
                                    );
                                }
                                if ui.small_button("Consume").clicked() {
                                    self.consume(&approval.id, notifications);
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
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

fn parse_status(value: &str) -> Result<Option<ApprovalStatus>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    ApprovalStatus::parse(trimmed).map(Some).ok_or_else(|| {
        "status must be one of: pending, approved, rejected, expired, consumed".to_string()
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
