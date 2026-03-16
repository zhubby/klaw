use crate::panels::{PanelRenderer, RenderCtx};
use egui_extras::{Size, StripBuilder};
use klaw_config::ConfigStore;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, LoadAvg, MemoryRefreshKind, RefreshKind, System,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CARD_MIN_HEIGHT: f32 = 132.0;
const CARD_GAP: f32 = 12.0;
const PROGRESS_BAR_WIDTH_RATIO: f32 = 0.82;
const SYSTEM_INFO_SECTION_HEIGHT: f32 = 320.0;

pub struct SystemMonitorPanel {
    system: System,
    last_refreshed_at: Instant,
    app_started_at: Instant,
    data_dir_path: PathBuf,
    data_dir_stats: Option<DataDirStats>,
    data_dir_stats_rx: Option<Receiver<DataDirStats>>,
    data_dir_collect_started: bool,
}

impl Default for SystemMonitorPanel {
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
            app_started_at: Instant::now(),
            data_dir_path: resolve_data_dir_path(),
            data_dir_stats: None,
            data_dir_stats_rx: None,
            data_dir_collect_started: false,
        }
    }
}

impl SystemMonitorPanel {
    fn refresh_if_due(&mut self) {
        if self.last_refreshed_at.elapsed() < REFRESH_INTERVAL {
            return;
        }

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.last_refreshed_at = Instant::now();
    }

    fn ensure_data_dir_stats_collection_started(&mut self) {
        if self.data_dir_collect_started {
            return;
        }

        self.data_dir_collect_started = true;
        let data_dir_path = self.data_dir_path.clone();
        let (tx, rx) = mpsc::channel();
        self.data_dir_stats_rx = Some(rx);

        std::thread::spawn(move || {
            let stats = collect_data_dir_stats(&data_dir_path);
            let _ = tx.send(stats);
        });
    }

    fn poll_data_dir_stats(&mut self) {
        let Some(rx) = self.data_dir_stats_rx.as_ref() else {
            return;
        };

        match rx.try_recv() {
            Ok(stats) => {
                self.data_dir_stats = Some(stats);
                self.data_dir_stats_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.data_dir_stats_rx = None;
            }
        }
    }
}

impl PanelRenderer for SystemMonitorPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        _notifications: &mut crate::notifications::NotificationCenter,
    ) {
        self.refresh_if_due();
        self.ensure_data_dir_stats_collection_started();
        self.poll_data_dir_stats();
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

        let uptime_secs = self.app_started_at.elapsed().as_secs();

        let system_uptime_secs = System::uptime();
        let load_avg = System::load_average();

        ui.heading(ctx.tab_title);
        ui.label("Real-time resource and system information");
        ui.separator();

        StripBuilder::new(ui)
            .size(Size::exact(CARD_MIN_HEIGHT))
            .size(Size::exact(SYSTEM_INFO_SECTION_HEIGHT))
            .vertical(|mut strip| {
                strip.cell(|ui| {
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.x = CARD_GAP;
                        StripBuilder::new(ui)
                            .size(Size::remainder())
                            .size(Size::remainder())
                            .size(Size::remainder())
                            .size(Size::remainder())
                            .horizontal(|mut row| {
                                row.cell(|ui| {
                                    resource_card(
                                        ui,
                                        "CPU Usage",
                                        Some(cpu_usage),
                                        format!("{cpu_usage:.1}%"),
                                        format!(
                                            "{logical_cpus} logical cores / {physical_cores} physical cores"
                                        ),
                                    );
                                });
                                row.cell(|ui| {
                                    resource_card(
                                        ui,
                                        "Memory Usage",
                                        Some(memory_usage),
                                        format!(
                                            "{:.1}% ({}/{})",
                                            memory_usage,
                                            format_bytes(used_memory),
                                            format_bytes(total_memory),
                                        ),
                                        format!("Free: {}", format_bytes(free_memory)),
                                    );
                                });
                                row.cell(|ui| {
                                    if let Some(stats) = self.data_dir_stats.as_ref() {
                                        resource_card(
                                            ui,
                                            "Data Directory Disk Usage",
                                            None,
                                            format_bytes(stats.used_bytes),
                                            format!("Path: {}", self.data_dir_path.display()),
                                        );
                                    } else {
                                        loading_card(
                                            ui,
                                            "Data Directory Disk Usage",
                                            format!("Path: {}", self.data_dir_path.display()),
                                            "Collecting disk usage once...",
                                        );
                                    }
                                });
                                row.cell(|ui| {
                                    resource_card(
                                        ui,
                                        "App Uptime",
                                        None,
                                        format_duration(uptime_secs),
                                        "Running in current process".to_string(),
                                    );
                                });
                            });
                    });
                });

                strip.cell(|ui| {
                    ui.separator();
                    ui.strong("System Information");
                    ui.add_space(6.0);

                    egui::ScrollArea::vertical()
                        .id_salt("system-monitor-system-info-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("system-monitor-system-info-grid")
                                .num_columns(2)
                                .spacing([14.0, 6.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    info_row(ui, "Host Name", optional_text(System::host_name()));
                                    info_row(ui, "OS Name", optional_text(System::name()));
                                    info_row(ui, "OS Version", optional_text(System::os_version()));
                                    info_row(
                                        ui,
                                        "Long OS Version",
                                        optional_text(System::long_os_version()),
                                    );
                                    info_row(
                                        ui,
                                        "Kernel Version",
                                        optional_text(System::kernel_version()),
                                    );
                                    info_row(ui, "CPU Architecture", std::env::consts::ARCH.to_string());
                                    info_row(ui, "Logical CPU Count", logical_cpus.to_string());
                                    info_row(ui, "Physical Core Count", physical_cores.to_string());
                                    info_row(
                                        ui,
                                        "Primary CPU Brand",
                                        self.system
                                            .cpus()
                                            .first()
                                            .map(|cpu| cpu.brand().to_string())
                                            .filter(|value| !value.is_empty())
                                            .unwrap_or_else(|| "N/A".to_string()),
                                    );
                                    info_row(
                                        ui,
                                        "Primary CPU Frequency",
                                        self.system
                                            .cpus()
                                            .first()
                                            .map(|cpu| format!("{} MHz", cpu.frequency()))
                                            .unwrap_or_else(|| "N/A".to_string()),
                                    );
                                    info_row(ui, "Total Memory", format_bytes(total_memory));
                                    info_row(ui, "Used Memory", format_bytes(used_memory));
                                    info_row(ui, "Free Memory", format_bytes(free_memory));
                                    info_row(ui, "Total Swap", format_bytes(self.system.total_swap()));
                                    info_row(ui, "Used Swap", format_bytes(self.system.used_swap()));
                                    info_row(ui, "System Uptime", format_duration(system_uptime_secs));
                                    info_row(
                                        ui,
                                        "System Boot Time",
                                        format!("unix: {}", System::boot_time()),
                                    );
                                    info_row(ui, "Load Average", format_load_avg(load_avg));
                                    info_row(
                                        ui,
                                        "Data Directory",
                                        self.data_dir_path.display().to_string(),
                                    );

                                    if let Some(stats) = self.data_dir_stats.as_ref() {
                                        info_row(ui, "Data Directory Size", format_bytes(stats.used_bytes));
                                        info_row(
                                            ui,
                                            "Data Directory File Count",
                                            stats.file_count.to_string(),
                                        );
                                        info_row(
                                            ui,
                                            "Data Directory Mount Point",
                                            stats
                                                .mount_point
                                                .as_ref()
                                                .map(|path| path.display().to_string())
                                                .unwrap_or_else(|| "N/A".to_string()),
                                        );
                                        info_row(
                                            ui,
                                            "Data Directory Disk Capacity",
                                            format_bytes(stats.disk_total_bytes),
                                        );
                                        info_row(
                                            ui,
                                            "Data Directory Disk Available",
                                            format_bytes(stats.disk_available_bytes),
                                        );
                                    } else {
                                        info_row(ui, "Data Directory Size", "Loading...".to_string());
                                        info_row(
                                            ui,
                                            "Data Directory File Count",
                                            "Loading...".to_string(),
                                        );
                                        info_row(
                                            ui,
                                            "Data Directory Mount Point",
                                            "Loading...".to_string(),
                                        );
                                        info_row(
                                            ui,
                                            "Data Directory Disk Capacity",
                                            "Loading...".to_string(),
                                        );
                                        info_row(
                                            ui,
                                            "Data Directory Disk Available",
                                            "Loading...".to_string(),
                                        );
                                    }
                                });
                        });
                });
            });
    }
}

#[derive(Debug, Clone, Default)]
struct DataDirStats {
    used_bytes: u64,
    file_count: u64,
    disk_total_bytes: u64,
    disk_available_bytes: u64,
    mount_point: Option<PathBuf>,
}

fn collect_data_dir_stats(data_dir_path: &Path) -> DataDirStats {
    let (used_bytes, file_count) = dir_size_and_file_count(data_dir_path);
    let (disk_total_bytes, disk_available_bytes, mount_point) = disk_space_for_path(data_dir_path);

    DataDirStats {
        used_bytes,
        file_count,
        disk_total_bytes,
        disk_available_bytes,
        mount_point,
    }
}

fn disk_space_for_path(path: &Path) -> (u64, u64, Option<PathBuf>) {
    let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());

    let mut best: Option<(usize, u64, u64, PathBuf)> = None;
    for disk in disks.list() {
        let mount_point = disk.mount_point();
        if !path.starts_with(mount_point) {
            continue;
        }
        let mount_len = mount_point.as_os_str().len();
        match best.as_ref() {
            Some((best_len, _, _, _)) if *best_len > mount_len => {}
            _ => {
                best = Some((
                    mount_len,
                    disk.total_space(),
                    disk.available_space(),
                    mount_point.to_path_buf(),
                ));
            }
        }
    }

    best.map(|(_, total, available, mount)| (total, available, Some(mount)))
        .unwrap_or((0, 0, None))
}

fn dir_size_and_file_count(path: &Path) -> (u64, u64) {
    let mut total_size = 0_u64;
    let mut file_count = 0_u64;
    let mut stack = vec![path.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };

        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_file() {
                total_size = total_size.saturating_add(metadata.len());
                file_count = file_count.saturating_add(1);
            } else if metadata.is_dir() {
                stack.push(entry.path());
            }
        }
    }

    (total_size, file_count)
}

fn resolve_data_dir_path() -> PathBuf {
    if let Ok(store) = ConfigStore::open(None) {
        let root_dir = store
            .snapshot()
            .config
            .storage
            .root_dir
            .map(|raw| raw.trim().to_string())
            .filter(|raw| !raw.is_empty());
        if let Some(root_dir) = root_dir {
            return PathBuf::from(root_dir);
        }
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".klaw"))
        .unwrap_or_else(|| PathBuf::from(".klaw"))
}

fn resource_card(
    ui: &mut egui::Ui,
    title: &str,
    percent: Option<f32>,
    value: String,
    subtitle: String,
) {
    // ui.set_min_width(ui.available_width());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(CARD_MIN_HEIGHT);
        ui.vertical(|ui| {
            ui.strong(title);
            ui.label(subtitle);
            if let Some(percent) = percent {
                // ui.add_space(6.0);
                let bar_width = (ui.available_width() * PROGRESS_BAR_WIDTH_RATIO).max(80.0);
                ui.add_sized(
                    [bar_width, 20.0],
                    egui::ProgressBar::new((percent / 100.0).clamp(0.0, 1.0)).show_percentage(),
                );
            }
            // ui.add_space(6.0);
            ui.monospace(value);
        });
    });
}

fn loading_card(ui: &mut egui::Ui, title: &str, subtitle: String, loading_text: &str) {
    ui.set_min_width(ui.available_width());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(CARD_MIN_HEIGHT);
        ui.vertical(|ui| {
            ui.strong(title);
            ui.label(subtitle);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(loading_text);
            });
        });
    });
}

fn info_row(ui: &mut egui::Ui, key: &str, value: String) {
    ui.label(key);
    ui.monospace(value);
    ui.end_row();
}

fn optional_text(value: Option<String>) -> String {
    value.unwrap_or_else(|| "N/A".to_string())
}

fn format_load_avg(value: LoadAvg) -> String {
    format!(
        "1m: {:.2}, 5m: {:.2}, 15m: {:.2}",
        value.one, value.five, value.fifteen
    )
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    if days > 0 {
        format!("{days}d {hours:02}h {minutes:02}m {secs:02}s")
    } else {
        format!("{hours:02}h {minutes:02}m {secs:02}s")
    }
}

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
