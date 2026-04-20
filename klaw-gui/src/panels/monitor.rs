use crate::panels::{PanelRenderer, RenderCtx};
use klaw_config::ConfigStore;
use std::time::Duration;

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub struct MonitorPanel {
    profiler_ui: puffin_egui::GlobalProfilerUi,
    profiler_enabled: bool,
}

impl Default for MonitorPanel {
    fn default() -> Self {
        Self {
            profiler_ui: puffin_egui::GlobalProfilerUi::default(),
            profiler_enabled: false,
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
        let enabled = ConfigStore::open(None)
            .ok()
            .map(|store| store.snapshot().config.profiler.enabled)
            .unwrap_or(false);

        if enabled != self.profiler_enabled {
            puffin::set_scopes_on(enabled);
            self.profiler_enabled = enabled;
        }

        ui.ctx().request_repaint_after(REFRESH_INTERVAL);
        self.profiler_ui.ui(ui);
    }
}
