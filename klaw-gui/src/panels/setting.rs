use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};

#[derive(Default)]
pub struct SettingPanel;

impl PanelRenderer for SettingPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut NotificationCenter,
    ) {
        ui.heading(ctx.tab_title);
        ui.label("Settings panel is reserved and not implemented yet.");
    }
}
