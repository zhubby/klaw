use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct ApprovalPanel;

impl PanelRenderer for ApprovalPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        ui.heading(ctx.tab_title);
        ui.label("Approval management (placeholder)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Approvals",
            "Approval queue and decision actions will be connected in the next phase.",
        );
    }
}
