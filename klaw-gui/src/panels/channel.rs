use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct ChannelPanel;

impl PanelRenderer for ChannelPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Inbound and outbound channel surface (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Channel Matrix",
            "stdio / gateway / im adapters will be shown here after data binding.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "channel-grid",
            &[("Active", "0"), ("Pending", "0"), ("Errors", "0")],
        );
    }
}
