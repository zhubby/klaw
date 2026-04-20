use crate::panels::{PanelRenderer, RenderCtx};
use std::time::Duration;

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub struct MonitorPanel {
    profiler_ui: puffin_egui::GlobalProfilerUi,
}

impl Default for MonitorPanel {
    fn default() -> Self {
        Self {
            profiler_ui: puffin_egui::GlobalProfilerUi::default(),
        }
    }
}

impl PanelRenderer for MonitorPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        ui.ctx().request_repaint_after(REFRESH_INTERVAL);
        self.profiler_ui.ui(ui);
    }
}
