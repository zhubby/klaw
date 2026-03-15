use crate::panels::{PanelRenderer, RenderCtx};
use crate::widgets::placeholder;

pub struct HeartbeatPanel;

impl PanelRenderer for HeartbeatPanel {
    fn render(&mut self, ui: &mut egui::Ui, ctx: &RenderCtx<'_>) {
        ui.heading(ctx.tab_title);
        ui.label("Session heartbeat watcher (mock)");
        ui.separator();
        placeholder::section_card(
            ui,
            "Session Liveness",
            "Heartbeat intervals and expiry detection are not connected yet.",
        );
        ui.add_space(8.0);
        placeholder::key_value_grid(
            ui,
            "heartbeat-grid",
            &[("Tracked", "0"), ("Expired", "0"), ("Latency", "N/A")],
        );
    }
}
