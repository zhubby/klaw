use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct ToolPanel;

impl PanelRenderer for ToolPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Tool registry and execution status (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Available Tools",
            "Tool discovery, metadata, and execution telemetry will be bound later.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "tool-grid",
            &[("Registered", "0"), ("Enabled", "0"), ("Recent Calls", "0")],
        );
    }
}
