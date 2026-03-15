use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct SystemMonitorPanel;

impl PanelRenderer for SystemMonitorPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        ui.heading(ctx.tab_title);
        ui.label("Runtime health and observability (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "System Signals",
            "CPU, memory, queue depth, and task counters will be attached in a later phase.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "system-grid",
            &[("CPU", "N/A"), ("Memory", "N/A"), ("Uptime", "N/A")],
        );
    }
}
