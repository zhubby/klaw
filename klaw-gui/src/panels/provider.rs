use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct ProviderPanel;

impl PanelRenderer for ProviderPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Provider registry overview (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Configured Providers",
            "OpenAI / Anthropic placeholders. Runtime lookup is intentionally disabled in this phase.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "provider-grid",
            &[
                ("Default", "openai"),
                ("Fallback", "none"),
                ("Status", "mock"),
            ],
        );
    }
}
