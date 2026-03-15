use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct ProfilePanel;

impl PanelRenderer for ProfilePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        ui.heading(ctx.tab_title);
        ui.label("Workspace profile summary (mock data)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Active Profile",
            "Default profile is selected. Data binding is not wired yet.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "profile-grid",
            &[
                ("Name", "default"),
                ("Env", "local"),
                ("Last Updated", "N/A"),
            ],
        );
    }
}
