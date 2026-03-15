use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct SkillManagePanel;

impl PanelRenderer for SkillManagePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        ui.heading(ctx.tab_title);
        ui.label("Installed skill management (placeholder)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Installed Skills",
            "Installed skill list and enable/disable actions will be connected in the next phase.",
        );
    }
}
