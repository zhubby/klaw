use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct McpPanel;

impl PanelRenderer for McpPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("MCP server integrations (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Connected MCP Servers",
            "Transport status, tools, and resources will be listed here.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "mcp-grid",
            &[("Servers", "0"), ("Tools", "0"), ("Resources", "0")],
        );
    }
}
