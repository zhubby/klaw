use crate::panels::{PanelRenderer, RenderCtx};
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROGRESS_BAR_HEIGHT: f32 = 12.0;

fn format_bytes(value: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let raw = value as f64;
    if raw >= GB {
        format!("{:.2} GB", raw / GB)
    } else if raw >= MB {
        format!("{:.2} MB", raw / MB)
    } else if raw >= KB {
        format!("{:.2} KB", raw / KB)
    } else {
        format!("{value} B")
    }
}

pub struct MonitorPanel {
    system: System,
    last_refreshed_at: Instant,
}

impl Default for MonitorPanel {
    fn default() -> Self {
        let mut system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        system.refresh_cpu_usage();
        system.refresh_memory();

        Self {
            system,
            last_refreshed_at: Instant::now(),
        }
    }
}

impl MonitorPanel {
    fn refresh_if_due(&mut self) {
        if self.last_refreshed_at.elapsed() < REFRESH_INTERVAL {
            return;
        }

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.last_refreshed_at = Instant::now();
    }
}

impl PanelRenderer for MonitorPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        self.refresh_if_due();
        ui.ctx().request_repaint_after(REFRESH_INTERVAL);

        let cpu_usage = self.system.global_cpu_usage();
        let logical_cpus = self.system.cpus().len();
        let physical_cores = System::physical_core_count().unwrap_or_default();

        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        let free_memory = total_memory.saturating_sub(used_memory);
        let memory_usage = if total_memory == 0 {
            0.0
        } else {
            (used_memory as f32 / total_memory as f32) * 100.0
        };

        ui.heading(ctx.tab_title);
        ui.label("Real-time resource and system information");
        ui.separator();

        ui.columns(2, |cols| {
            cols[0].vertical(|ui| {
                ui.strong("CPU Usage");
                ui.horizontal(|ui| {
                    let bar = egui::ProgressBar::new((cpu_usage / 100.0).clamp(0.0, 1.0))
                        .show_percentage()
                        .desired_height(PROGRESS_BAR_HEIGHT);
                    ui.add(bar);
                    ui.monospace(format!("{cpu_usage:.1}%"));
                });
                ui.label(format!(
                    "{} logical / {} physical cores",
                    logical_cpus, physical_cores
                ));
            });

            cols[1].vertical(|ui| {
                ui.strong("Memory Usage");
                ui.horizontal(|ui| {
                    let bar = egui::ProgressBar::new((memory_usage / 100.0).clamp(0.0, 1.0))
                        .show_percentage()
                        .desired_height(PROGRESS_BAR_HEIGHT);
                    ui.add(bar);
                    ui.monospace(format!(
                        "{:.1}% ({}/{})",
                        memory_usage,
                        format_bytes(used_memory),
                        format_bytes(total_memory),
                    ));
                });
                ui.label(format!("Free: {}", format_bytes(free_memory)));
            });
        });
    }
}
