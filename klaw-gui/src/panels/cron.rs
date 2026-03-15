use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct CronPanel;

impl PanelRenderer for CronPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Scheduled jobs dashboard (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Job Queue",
            "Cron schedules, next runs, and last run results will appear here.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "cron-grid",
            &[("Jobs", "0"), ("Enabled", "0"), ("Missed", "0")],
        );
    }
}
