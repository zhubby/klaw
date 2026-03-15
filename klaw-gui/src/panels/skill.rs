use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct SkillPanel;

impl PanelRenderer for SkillPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Skill catalog and sync status (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Installed Skills",
            "Skill source, version, and sync controls will be added later.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "skill-grid",
            &[("Installed", "0"), ("Enabled", "0"), ("Failures", "0")],
        );
    }
}
