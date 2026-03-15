use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct MemoryPanel;

impl PanelRenderer for MemoryPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Memory index and retrieval status (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Memory Stores",
            "BM25/vector store statistics and health checks will appear here.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "memory-grid",
            &[
                ("Documents", "0"),
                ("Embeddings", "0"),
                ("Index State", "idle"),
            ],
        );
    }
}
