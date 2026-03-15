use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct ArchivePanel;

impl PanelRenderer for ArchivePanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Archive browser (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Archive Items",
            "File list, metadata, and restore actions are placeholders in this build.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "archive-grid",
            &[("Items", "0"), ("Size", "0 B"), ("Last Sync", "N/A")],
        );
    }
}
