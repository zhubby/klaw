use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::current_ui_language;
use crate::time_format::format_timestamp_millis;
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_approval::{
    ApprovalListQuery, ApprovalManager, ApprovalResolveDecision, ApprovalStatus,
    SqliteApprovalManager,
};
use klaw_storage::ApprovalRecord;
use klaw_ui_kit::{LocaleDomain, Translator};
use std::collections::HashMap;
use std::future::Future;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Builder;

const FILTER_INPUT_WIDTH: f32 = 120.0;
const PAGING_INPUT_WIDTH: f32 = 50.0;

#[derive(Default)]
pub struct ApprovalPanel {
    loaded: bool,
    approvals: Vec<ApprovalRecord>,
    session_keys: Vec<String>,
    tool_names: Vec<String>,
    session_key_filter: Option<String>,
    tool_name_filter: Option<String>,
    status_filter: Option<ApprovalStatus>,
    preview_filter: String,
    page: i64,
    size: i64,
    selected_approval: Option<String>,
    view_approval: Option<ApprovalRecord>,
}

impl ApprovalPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        if self.size == 0 {
            self.size = 100;
        }
        self.load_filters(notifications);
        self.refresh(notifications);
    }

    fn load_filters(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        match run_approval_task(move |manager| async move {
            let session_keys = manager.list_session_keys().await?;
            let tool_names = manager.list_tool_names().await?;
            Ok((session_keys, tool_names))
        }) {
            Ok((session_keys, tool_names)) => {
                self.session_keys = session_keys;
                self.tool_names = tool_names;
            }
            Err(err) => notifications.error(t.text_args(
                "approval-notify-filters-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn refresh(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let size = self.size.max(1);
        let page = self.page.max(1);
        let offset = (page - 1) * size;
        let session_key = self.session_key_filter.clone();
        let tool_name = self.tool_name_filter.clone();
        let preview_filter = if self.preview_filter.trim().is_empty() {
            None
        } else {
            Some(self.preview_filter.trim().to_string())
        };
        let query = ApprovalListQuery {
            session_key,
            tool_name,
            status: self.status_filter,
            preview_filter,
            limit: size,
            offset,
        };

        match run_approval_task(move |manager| async move { manager.list_approvals(query).await }) {
            Ok(approvals) => {
                self.approvals = approvals;
                self.loaded = true;
            }
            Err(err) => notifications.error(t.text_args(
                "approval-notify-list-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn resolve(
        &mut self,
        approval_id: &str,
        decision: ApprovalResolveDecision,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        let approval_id = approval_id.to_string();
        match run_approval_task(move |manager| async move {
            manager
                .resolve_approval(&approval_id, decision, Some("gui-user"), now_ms())
                .await
        }) {
            Ok(outcome) => {
                if outcome.updated {
                    notifications.success(t.text_args(
                        "approval-notify-resolved",
                        HashMap::from([("id", outcome.approval.id.clone())]),
                    ));
                }
                self.refresh(notifications);
            }
            Err(err) => notifications.error(t.text_args(
                "approval-notify-resolve-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn consume(&mut self, approval_id: &str, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        let approval_id = approval_id.to_string();
        match run_approval_task(move |manager| async move {
            manager.consume_approval(&approval_id, now_ms()).await
        }) {
            Ok(outcome) => {
                if outcome.updated {
                    notifications.success(t.text_args(
                        "approval-notify-consumed",
                        HashMap::from([("id", outcome.approval.id.clone())]),
                    ));
                } else {
                    notifications.error(t.text_args(
                        "approval-notify-consume-failed",
                        HashMap::from([("id", outcome.approval.id.clone())]),
                    ));
                }
                self.refresh(notifications);
            }
            Err(err) => notifications.error(t.text_args(
                "approval-notify-consume-op-failed",
                HashMap::from([("error", err.to_string())]),
            )),
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

        let t = Self::translator();

        ui.heading(ctx.tab_title);
        ui.label(t.text("approval-subtitle"));
        ui.horizontal(|ui| {
            if ui
                .button(t.text_args(
                    "approval-btn-refresh",
                    HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.refresh(notifications);
            }
            ui.label(t.text_args(
                "approval-label-count",
                HashMap::from([("count", self.approvals.len().to_string())]),
            ));
        });

        ui.separator();
        let mut need_refresh = false;
        let filter_row = egui::ScrollArea::horizontal()
            .id_salt("approval-filter-row")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.text("approval-filter-session-key"));
                    let selected_text = self
                        .session_key_filter
                        .as_deref()
                        .unwrap_or_else(|| t.text("approval-filter-session-key-all").leak());
                    let combo_resp = egui::ComboBox::from_id_salt("session_key_filter")
                        .selected_text(selected_text)
                        .width(FILTER_INPUT_WIDTH)
                        .show_ui(ui, |ui| {
                            let mut changed = false;
                            if ui
                                .selectable_value(
                                    &mut self.session_key_filter,
                                    None,
                                    t.text("approval-filter-session-key-all"),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                            for key in &self.session_keys {
                                if ui
                                    .selectable_value(
                                        &mut self.session_key_filter,
                                        Some(key.clone()),
                                        key,
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

                    ui.separator();

                    ui.label(t.text("approval-filter-tool-name"));
                    let selected_text = self
                        .tool_name_filter
                        .as_deref()
                        .unwrap_or_else(|| t.text("approval-filter-tool-name-all").leak());
                    let combo_resp = egui::ComboBox::from_id_salt("tool_name_filter")
                        .selected_text(selected_text)
                        .width(FILTER_INPUT_WIDTH)
                        .show_ui(ui, |ui| {
                            let mut changed = false;
                            if ui
                                .selectable_value(
                                    &mut self.tool_name_filter,
                                    None,
                                    t.text("approval-filter-tool-name-all"),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                            for name in &self.tool_names {
                                if ui
                                    .selectable_value(
                                        &mut self.tool_name_filter,
                                        Some(name.clone()),
                                        name,
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

                    ui.separator();

                    ui.label(t.text("approval-filter-status"));
                    let combo_resp = egui::ComboBox::from_id_salt("status_filter")
                        .selected_text(approval_status_display_text(self.status_filter, &t))
                        .show_ui(ui, |ui| {
                            let mut changed = false;
                            if ui
                                .selectable_value(
                                    &mut self.status_filter,
                                    None,
                                    t.text("approval-filter-status-all"),
                                )
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
                                        t.text(&format!("approval-status-{}", status.as_str())),
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

                    ui.separator();

                    ui.label(t.text("approval-filter-preview"));
                    if ui
                        .add_sized(
                            [FILTER_INPUT_WIDTH, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut self.preview_filter),
                        )
                        .changed()
                    {
                        self.page = 1;
                        need_refresh = true;
                    }

                    ui.separator();

                    ui.label(t.text("approval-filter-page"));
                    if ui
                        .add_sized(
                            [PAGING_INPUT_WIDTH, ui.spacing().interact_size.y],
                            egui::DragValue::new(&mut self.page).range(1..=i64::MAX),
                        )
                        .changed()
                    {
                        need_refresh = true;
                    }
                    ui.label(t.text("approval-filter-size"));
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
        let _ = filter_row;
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
                    ui.label(t.text("approval-no-rows"));
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
                            ui.strong(t.text("approval-col-id"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-session"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-tool"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-risk"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-status"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-requested-by"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-approved-by"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-expires-at"));
                        });
                        header.col(|ui| {
                            ui.strong(t.text("approval-col-preview"));
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
                                let (icon, color, text) =
                                    approval_status_display(approval.status, &t);
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
                                if ui
                                    .button(t.text_args(
                                        "approval-ctx-view",
                                        HashMap::from([("icon", regular::EYE.to_string())]),
                                    ))
                                    .clicked()
                                {
                                    view_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .button(t.text_args(
                                        "approval-ctx-approve",
                                        HashMap::from([(
                                            "icon",
                                            regular::CHECK_CIRCLE.to_string(),
                                        )]),
                                    ))
                                    .clicked()
                                {
                                    approve_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                if ui
                                    .button(t.text_args(
                                        "approval-ctx-reject",
                                        HashMap::from([("icon", regular::X_CIRCLE.to_string())]),
                                    ))
                                    .clicked()
                                {
                                    reject_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                if ui
                                    .button(t.text_args(
                                        "approval-ctx-consume",
                                        HashMap::from([("icon", regular::LIGHTNING.to_string())]),
                                    ))
                                    .clicked()
                                {
                                    consume_id = Some(approval.id.clone());
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .button(t.text_args(
                                        "approval-ctx-copy-id",
                                        HashMap::from([("icon", regular::COPY.to_string())]),
                                    ))
                                    .clicked()
                                {
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
            egui::Window::new(t.text_args(
                "approval-detail-title",
                HashMap::from([("id", approval.id.clone())]),
            ))
            .open(&mut open)
            .resizable(true)
            .default_size([500.0, 400.0])
            .show(ui.ctx(), |ui| {
                let na = t.text("approval-detail-na");
                egui::Grid::new("approval-detail-grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(t.text("approval-detail-id"));
                        ui.label(&approval.id);
                        ui.end_row();

                        ui.label(t.text("approval-detail-session"));
                        ui.label(&approval.session_key);
                        ui.end_row();

                        ui.label(t.text("approval-detail-tool"));
                        ui.label(&approval.tool_name);
                        ui.end_row();

                        ui.label(t.text("approval-detail-risk-level"));
                        ui.label(&approval.risk_level);
                        ui.end_row();

                        ui.label(t.text("approval-detail-status"));
                        let (icon, color, text) = approval_status_display(approval.status, &t);
                        ui.label(
                            RichText::new(format!("{icon} {text}"))
                                .color(color)
                                .strong(),
                        );
                        ui.end_row();

                        ui.label(t.text("approval-detail-requested-by"));
                        ui.label(&approval.requested_by);
                        ui.end_row();

                        ui.label(t.text("approval-detail-approved-by"));
                        ui.label(approval.approved_by.as_deref().unwrap_or(&na));
                        ui.end_row();

                        ui.label(t.text("approval-detail-justification"));
                        ui.label(approval.justification.as_deref().unwrap_or(&na));
                        ui.end_row();

                        ui.label(t.text("approval-detail-expires-at"));
                        ui.label(format_timestamp_millis(approval.expires_at_ms));
                        ui.end_row();

                        ui.label(t.text("approval-detail-created-at"));
                        ui.label(format_timestamp_millis(approval.created_at_ms));
                        ui.end_row();

                        ui.label(t.text("approval-detail-updated-at"));
                        ui.label(format_timestamp_millis(approval.updated_at_ms));
                        ui.end_row();

                        ui.label(t.text("approval-detail-consumed-at"));
                        ui.label(
                            approval
                                .consumed_at_ms
                                .map(format_timestamp_millis)
                                .as_deref()
                                .unwrap_or(&na),
                        );
                        ui.end_row();
                    });

                ui.separator();
                ui.label(t.text("approval-detail-command-preview"));
                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .id_salt("approval_command_preview")
                    .show(ui, |ui| {
                        ui.label(&approval.command_preview);
                    });

                ui.separator();
                ui.label(t.text("approval-detail-command-text"));
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

fn approval_status_display(
    status: ApprovalStatus,
    t: &Translator,
) -> (&'static str, Color32, String) {
    match status {
        ApprovalStatus::Pending => (
            regular::HOURGLASS_MEDIUM,
            Color32::from_rgb(200, 150, 50),
            t.text("approval-status-pending"),
        ),
        ApprovalStatus::Approved => (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(50, 180, 80),
            t.text("approval-status-approved"),
        ),
        ApprovalStatus::Rejected => (
            regular::X_CIRCLE,
            Color32::from_rgb(220, 60, 60),
            t.text("approval-status-rejected"),
        ),
        ApprovalStatus::Expired => (
            regular::CLOCK,
            Color32::from_rgb(140, 140, 140),
            t.text("approval-status-expired"),
        ),
        ApprovalStatus::Consumed => (
            regular::LIGHTNING,
            Color32::from_rgb(70, 130, 200),
            t.text("approval-status-consumed"),
        ),
    }
}

fn approval_status_display_text(status: Option<ApprovalStatus>, t: &Translator) -> String {
    match status {
        Some(s) => approval_status_display(s, t).2,
        None => t.text("approval-filter-status-all"),
    }
}
