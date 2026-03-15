use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct SessionPanel;

impl PanelRenderer for SessionPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        ui.heading(ctx.tab_title);
        ui.label("Session management (placeholder)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Sessions",
            "Session list and management actions will be connected in the next phase.",
        );
    }
}
