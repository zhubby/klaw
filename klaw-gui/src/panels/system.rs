use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::{RuntimeRequestHandle, begin_env_check_request};
use egui::RichText;
use egui_phosphor::regular;
use klaw_config::ConfigStore;
use klaw_storage::StoragePaths;
use klaw_util::{DependencyCategory, EnvironmentCheckReport, KLAW_DIR_NAME, default_data_dir};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System,
};

const TASK_POLL_INTERVAL: Duration = Duration::from_millis(200);
const HOST_INFO_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

struct HostInfoData {
    system: System,
    last_refreshed_at: Instant,
    app_started_at: Instant,
    data_dir_path: PathBuf,
    data_dir_stats: Option<HostDataDirStats>,
    data_dir_stats_rx: Option<Receiver<HostDataDirStats>>,
    data_dir_collect_started: bool,
}

impl Default for HostInfoData {
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

impl HostInfoData {
    fn refresh_if_due(&mut self) {
        if self.last_refreshed_at.elapsed() < HOST_INFO_REFRESH_INTERVAL {
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
            let stats = collect_host_data_dir_stats(&data_dir_path);
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

#[derive(Debug, Clone, Default)]
struct HostDataDirStats {
    used_bytes: u64,
    file_count: u64,
    disk_total_bytes: u64,
    disk_available_bytes: u64,
    mount_point: Option<PathBuf>,
}

fn collect_host_data_dir_stats(data_dir_path: &Path) -> HostDataDirStats {
    let (used_bytes, file_count) = host_dir_size_and_file_count(data_dir_path);
    let (disk_total_bytes, disk_available_bytes, mount_point) = host_disk_space_for_path(data_dir_path);

    HostDataDirStats {
        used_bytes,
        file_count,
        disk_total_bytes,
        disk_available_bytes,
        mount_point,
    }
}

fn host_disk_space_for_path(path: &Path) -> (u64, u64, Option<PathBuf>) {
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

fn host_dir_size_and_file_count(path: &Path) -> (u64, u64) {
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

    default_data_dir().unwrap_or_else(|| PathBuf::from(KLAW_DIR_NAME))
}

fn format_bytes_si(value: u64) -> String {
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

fn host_info_row(ui: &mut egui::Ui, key: &str, value: String) {
    ui.label(key);
    ui.monospace(value);
    ui.end_row();
}

fn host_optional_text(value: Option<String>) -> String {
    value.unwrap_or_else(|| "N/A".to_string())
}

fn format_host_duration(seconds: u64) -> String {
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

fn format_host_load_avg(value: sysinfo::LoadAvg) -> String {
    format!(
        "1m: {:.2}, 5m: {:.2}, 15m: {:.2}",
        value.one, value.five, value.fifteen
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SystemView {
    #[default]
    HostInformation,
    ProgramDiskUsage,
    Environment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirKind {
    Tmp,
    Workspace,
    Sessions,
    Archives,
    Logs,
    Skills,
    SkillsRegistry,
}

impl DirKind {
    fn title(self) -> &'static str {
        match self {
            DirKind::Tmp => "Temporary",
            DirKind::Workspace => "Workspace",
            DirKind::Sessions => "Sessions",
            DirKind::Archives => "Archives",
            DirKind::Logs => "Logs",
            DirKind::Skills => "Skills",
            DirKind::SkillsRegistry => "Skills Registry",
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            DirKind::Tmp => "tmp",
            DirKind::Workspace => "workspace",
            DirKind::Sessions => "sessions",
            DirKind::Archives => "archives",
            DirKind::Logs => "logs",
            DirKind::Skills => "skills",
            DirKind::SkillsRegistry => "skills-registry",
        }
    }

    fn path(self, paths: &StoragePaths) -> PathBuf {
        match self {
            DirKind::Tmp => paths.tmp_dir.clone(),
            DirKind::Workspace => paths.workspace_dir.clone(),
            DirKind::Sessions => paths.sessions_dir.clone(),
            DirKind::Archives => paths.archives_dir.clone(),
            DirKind::Logs => paths.logs_dir.clone(),
            DirKind::Skills => paths.skills_dir.clone(),
            DirKind::SkillsRegistry => paths.skills_registry_dir.clone(),
        }
    }
}

struct DirState {
    usage_bytes: Option<u64>,
    usage_error: Option<String>,
    usage_rx: Option<Receiver<Result<u64, String>>>,
    clear_rx: Option<Receiver<Result<(), String>>>,
}

impl Default for DirState {
    fn default() -> Self {
        Self {
            usage_bytes: None,
            usage_error: None,
            usage_rx: None,
            clear_rx: None,
        }
    }
}

impl DirState {
    fn is_loading(&self) -> bool {
        self.usage_rx.is_some() || self.clear_rx.is_some()
    }
}

#[derive(Default)]
pub struct SystemPanel {
    paths: Option<StoragePaths>,
    dirs: [DirState; 7],
    clear_confirm: Option<DirKind>,
    env_check: Option<EnvironmentCheckReport>,
    env_check_loaded: bool,
    env_check_request: Option<RuntimeRequestHandle<EnvironmentCheckReport>>,
    current_view: SystemView,
    host_info: HostInfoData,
}

impl SystemPanel {
    fn dir_index(kind: DirKind) -> usize {
        match kind {
            DirKind::Tmp => 0,
            DirKind::Workspace => 1,
            DirKind::Sessions => 2,
            DirKind::Archives => 3,
            DirKind::Logs => 4,
            DirKind::Skills => 5,
            DirKind::SkillsRegistry => 6,
        }
    }

    fn get_dir(&self, kind: DirKind) -> &DirState {
        &self.dirs[Self::dir_index(kind)]
    }

    fn get_dir_mut(&mut self, kind: DirKind) -> &mut DirState {
        &mut self.dirs[Self::dir_index(kind)]
    }

    fn ensure_paths(&mut self, notifications: &mut NotificationCenter) {
        if self.paths.is_some() {
            return;
        }

        match StoragePaths::from_home_dir() {
            Ok(paths) => {
                self.paths = Some(paths);
            }
            Err(err) => {
                let message = format!("Failed to resolve data directories: {err}");
                self.dirs[0].usage_error = Some(message.clone());
                notifications.error(message);
            }
        }
    }

    fn any_loading(&self) -> bool {
        self.dirs.iter().any(|d| d.is_loading())
    }

    fn refresh_usage(&mut self, kind: DirKind) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let path = kind.path(paths);

        let (tx, rx) = mpsc::channel();
        let dir = self.get_dir_mut(kind);
        dir.usage_rx = Some(rx);
        dir.usage_error = None;

        thread::spawn(move || {
            let result = ensure_dir_exists(&path).and_then(|()| collect_dir_usage(&path));
            let _ = tx.send(result);
        });
    }

    fn clear_dir(&mut self, kind: DirKind) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let path = kind.path(paths);

        let (tx, rx) = mpsc::channel();
        self.get_dir_mut(kind).clear_rx = Some(rx);

        thread::spawn(move || {
            let _ = tx.send(clear_directory(&path));
        });
    }

    fn ensure_initial_usage_loaded(&mut self) {
        for kind in [
            DirKind::Tmp,
            DirKind::Workspace,
            DirKind::Sessions,
            DirKind::Archives,
            DirKind::Logs,
            DirKind::Skills,
            DirKind::SkillsRegistry,
        ] {
            let dir = self.get_dir(kind);
            if dir.usage_bytes.is_none() && dir.usage_rx.is_none() {
                self.refresh_usage(kind);
            }
        }
    }

    fn load_env_check(&mut self) {
        if let Some(request) = self.env_check_request.as_mut()
            && let Some(result) = request.try_take_result()
        {
            self.env_check_request = None;
            self.env_check_loaded = true;
            match result {
                Ok(report) => {
                    self.env_check = Some(report);
                }
                Err(err) => {
                    tracing::warn!("Failed to get environment check: {err}");
                }
            }
        }

        if self.env_check_loaded || self.env_check_request.is_some() {
            return;
        }
        self.env_check_request = Some(begin_env_check_request());
    }

    fn render_host_information(&self, ui: &mut egui::Ui) {
        let cpu_usage = self.host_info.system.global_cpu_usage();
        let logical_cpus = self.host_info.system.cpus().len();
        let physical_cores = System::physical_core_count().unwrap_or_default();

        let total_memory = self.host_info.system.total_memory();
        let used_memory = self.host_info.system.used_memory();
        let free_memory = total_memory.saturating_sub(used_memory);
        let memory_usage = if total_memory == 0 {
            0.0
        } else {
            (used_memory as f32 / total_memory as f32) * 100.0
        };

        let uptime_secs = self.host_info.app_started_at.elapsed().as_secs();
        let system_uptime_secs = System::uptime();
        let load_avg = System::load_average();

        const PROGRESS_BAR_HEIGHT: f32 = 12.0;

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
                        format_bytes_si(used_memory),
                        format_bytes_si(total_memory),
                    ));
                });
                ui.label(format!("Free: {}", format_bytes_si(free_memory)));
            });
        });

        ui.separator();
        ui.strong("System Information");
        ui.add_space(6.0);

        egui::ScrollArea::vertical()
            .id_salt("system-host-info-scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("system-host-info-grid")
                    .num_columns(2)
                    .spacing([14.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        host_info_row(ui, "App Uptime", format_host_duration(uptime_secs));
                        host_info_row(ui, "Host Name", host_optional_text(System::host_name()));
                        host_info_row(ui, "OS Name", host_optional_text(System::name()));
                        host_info_row(ui, "OS Version", host_optional_text(System::os_version()));
                        host_info_row(
                            ui,
                            "Long OS Version",
                            host_optional_text(System::long_os_version()),
                        );
                        host_info_row(
                            ui,
                            "Kernel Version",
                            host_optional_text(System::kernel_version()),
                        );
                        host_info_row(ui, "CPU Architecture", std::env::consts::ARCH.to_string());
                        host_info_row(ui, "Logical CPU Count", logical_cpus.to_string());
                        host_info_row(ui, "Physical Core Count", physical_cores.to_string());
                        host_info_row(
                            ui,
                            "Primary CPU Brand",
                            self.host_info
                                .system
                                .cpus()
                                .first()
                                .map(|cpu| cpu.brand().to_string())
                                .filter(|value| !value.is_empty())
                                .unwrap_or_else(|| "N/A".to_string()),
                        );
                        host_info_row(
                            ui,
                            "Primary CPU Frequency",
                            self.host_info
                                .system
                                .cpus()
                                .first()
                                .map(|cpu| format!("{} MHz", cpu.frequency()))
                                .unwrap_or_else(|| "N/A".to_string()),
                        );
                        host_info_row(ui, "Total Memory", format_bytes_si(total_memory));
                        host_info_row(ui, "Used Memory", format_bytes_si(used_memory));
                        host_info_row(ui, "Free Memory", format_bytes_si(free_memory));
                        host_info_row(ui, "Total Swap", format_bytes_si(self.host_info.system.total_swap()));
                        host_info_row(ui, "Used Swap", format_bytes_si(self.host_info.system.used_swap()));
                        host_info_row(ui, "System Uptime", format_host_duration(system_uptime_secs));
                        host_info_row(
                            ui,
                            "System Boot Time",
                            crate::time_format::format_timestamp_seconds(System::boot_time()),
                        );
                        host_info_row(ui, "Load Average", format_host_load_avg(load_avg));
                        host_info_row(
                            ui,
                            "Data Directory",
                            self.host_info.data_dir_path.display().to_string(),
                        );

                        if let Some(stats) = self.host_info.data_dir_stats.as_ref() {
                            host_info_row(ui, "Data Directory Size", format_bytes_si(stats.used_bytes));
                            host_info_row(
                                ui,
                                "Data Directory File Count",
                                stats.file_count.to_string(),
                            );
                            host_info_row(
                                ui,
                                "Data Directory Mount Point",
                                stats
                                    .mount_point
                                    .as_ref()
                                    .map(|path| path.display().to_string())
                                    .unwrap_or_else(|| "N/A".to_string()),
                            );
                            host_info_row(
                                ui,
                                "Data Directory Disk Capacity",
                                format_bytes_si(stats.disk_total_bytes),
                            );
                            host_info_row(
                                ui,
                                "Data Directory Disk Available",
                                format_bytes_si(stats.disk_available_bytes),
                            );
                        } else {
                            host_info_row(ui, "Data Directory Size", "Loading...".to_string());
                            host_info_row(ui, "Data Directory File Count", "Loading...".to_string());
                            host_info_row(ui, "Data Directory Mount Point", "Loading...".to_string());
                            host_info_row(ui, "Data Directory Disk Capacity", "Loading...".to_string());
                            host_info_row(
                                ui,
                                "Data Directory Disk Available",
                                "Loading...".to_string(),
                            );
                        }
                    });
            });
    }

    fn render_env_check_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Environment Dependencies");
        ui.add_space(4.0);

        let Some(report) = &self.env_check else {
            ui.label("Loading...");
            return;
        };

        let all_required_ok = report.all_required_available();
        let preferred_ok = report.all_preferred_available();
        let tm_ok = report.terminal_multiplexer_available();
        let success_color = egui::Color32::from_rgb(0x22, 0xC5, 0x5E);
        let warn_color = ui.visuals().warn_fg_color;
        let error_color = ui.visuals().error_fg_color;

        for check in &report.checks {
            ui.horizontal(|ui| {
                let icon = if check.available {
                    regular::CHECK_CIRCLE
                } else {
                    regular::X_CIRCLE
                };
                let color = if check.available {
                    success_color
                } else if check.required {
                    error_color
                } else if matches!(check.category, DependencyCategory::Preferred) {
                    warn_color
                } else {
                    warn_color
                };
                ui.label(RichText::new(icon).color(color).size(16.0));

                ui.label(RichText::new(&check.name).strong());

                if let Some(version) = &check.version {
                    ui.label(RichText::new(version).weak());
                } else {
                    ui.label(RichText::new("not found").weak());
                }

                let label = match check.category {
                    DependencyCategory::Required => "Required",
                    DependencyCategory::Preferred => "Preferred",
                    DependencyCategory::OptionalWithFallback => "Optional",
                };
                ui.label(
                    RichText::new(label)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            });

            ui.label(RichText::new(&check.description).small().weak().italics());
            if let Some(project_url) = &check.project_url {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Project:").small().weak());
                    ui.hyperlink_to(RichText::new(project_url).small(), project_url);
                });
            }
            ui.add_space(4.0);
        }

        if all_required_ok && preferred_ok && tm_ok {
            ui.label(RichText::new("All dependencies available").color(success_color));
        } else if all_required_ok && preferred_ok {
            ui.label(
                RichText::new("Note: Terminal multiplexer (zellij/tmux) not available")
                    .color(warn_color),
            );
        } else if all_required_ok {
            ui.label(
                RichText::new("Note: Some preferred dependencies are missing").color(warn_color),
            );
        } else {
            ui.label(
                RichText::new("Warning: Some required dependencies are missing").color(error_color),
            );
        }
    }

    fn poll_tasks(&mut self, notifications: &mut NotificationCenter) {
        for kind in [
            DirKind::Tmp,
            DirKind::Workspace,
            DirKind::Sessions,
            DirKind::Archives,
            DirKind::Logs,
            DirKind::Skills,
            DirKind::SkillsRegistry,
        ] {
            let dir = self.get_dir_mut(kind);

            if let Some(rx) = dir.usage_rx.as_ref() {
                if let Ok(result) = rx.try_recv() {
                    dir.usage_rx = None;
                    match result {
                        Ok(bytes) => {
                            dir.usage_bytes = Some(bytes);
                            dir.usage_error = None;
                        }
                        Err(err) => {
                            dir.usage_bytes = None;
                            dir.usage_error = Some(err.clone());
                            notifications
                                .error(format!("Failed to collect {} usage: {err}", kind.title()));
                        }
                    }
                }
            }

            if let Some(rx) = dir.clear_rx.as_ref() {
                if let Ok(result) = rx.try_recv() {
                    dir.clear_rx = None;
                    match result {
                        Ok(()) => {
                            dir.usage_bytes = Some(0);
                            notifications.success(format!("{} directory cleared", kind.title()));
                            self.refresh_usage(kind);
                        }
                        Err(err) => {
                            notifications.error(format!(
                                "Failed to clear {} directory: {err}",
                                kind.title()
                            ));
                        }
                    }
                }
            }
        }
    }

    fn render_clear_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        _notifications: &mut NotificationCenter,
    ) {
        let Some(kind) = self.clear_confirm else {
            return;
        };

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(format!("Clear {} directory", kind.title()))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Are you sure you want to clear the {} directory?",
                    kind.title()
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Clear").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            self.clear_dir(kind);
            self.clear_confirm = None;
        }
        if cancelled {
            self.clear_confirm = None;
        }
    }

    fn render_section(
        &mut self,
        ui: &mut egui::Ui,
        kind: DirKind,
        notifications: &mut NotificationCenter,
    ) {
        ui.strong(kind.title());
        ui.add_space(4.0);

        let Some(paths) = self.paths.as_ref() else {
            ui.label("Path unavailable.");
            return;
        };

        let path = kind.path(paths);
        ui.label(format!("Path: {}", path.display()));
        ui.add_space(6.0);

        let dir = self.get_dir(kind);
        let usage_loading = dir.usage_rx.is_some();
        let clear_loading = dir.clear_rx.is_some();
        let usage_bytes = dir.usage_bytes;
        let usage_error = dir.usage_error.clone();

        ui.horizontal(|ui| {
            let usage_text = usage_text(usage_loading, usage_bytes, usage_error.as_deref());
            ui.label(RichText::new(usage_text).strong());

            if ui
                .add_enabled(
                    !usage_loading && !clear_loading,
                    egui::Button::new(format!("{} Refresh", regular::ARROW_CLOCKWISE)),
                )
                .clicked()
            {
                self.refresh_usage(kind);
            }

            if ui
                .button(regular::FOLDER_OPEN)
                .on_hover_text(format!("Open {} directory in Finder", kind.title()))
                .clicked()
            {
                if let Err(err) = open_directory_in_file_manager(&path) {
                    notifications
                        .error(format!("Failed to open {} directory: {err}", kind.title()));
                }
            }

            if ui
                .add_enabled(
                    !clear_loading && !usage_loading,
                    egui::Button::new(regular::TRASH)
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12)),
                )
                .on_hover_text(format!("Clear {} directory", kind.title()))
                .clicked()
            {
                self.clear_confirm = Some(kind);
            }
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new(format!(
                "Clearing removes files inside `{}/`; the directory itself is kept.",
                kind.dir_name()
            ))
            .weak()
            .small(),
        );
    }
}

impl PanelRenderer for SystemPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_paths(notifications);
        self.ensure_initial_usage_loaded();
        self.load_env_check();
        self.poll_tasks(notifications);

        if self.any_loading() {
            ui.ctx().request_repaint_after(TASK_POLL_INTERVAL);
        }

        self.host_info.refresh_if_due();
        self.host_info.ensure_data_dir_stats_collection_started();
        self.host_info.poll_data_dir_stats();
        ui.ctx().request_repaint_after(HOST_INFO_REFRESH_INTERVAL);

        ui.heading(ctx.tab_title);

        ui.horizontal(|ui| {
            let host_selected = self.current_view == SystemView::HostInformation;
            let disk_selected = self.current_view == SystemView::ProgramDiskUsage;
            let env_selected = self.current_view == SystemView::Environment;

            if ui.selectable_label(host_selected, "Host Information").clicked() {
                self.current_view = SystemView::HostInformation;
            }
            if ui.selectable_label(disk_selected, "Program Disk Usage").clicked() {
                self.current_view = SystemView::ProgramDiskUsage;
            }
            if ui.selectable_label(env_selected, "Environment").clicked() {
                self.current_view = SystemView::Environment;
            }
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("system-panel-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.current_view {
                SystemView::HostInformation => {
                    self.render_host_information(ui);
                }
                SystemView::ProgramDiskUsage => {
                    ui.label("Inspect and clear data under the Klaw data directory.");
                    ui.add_space(8.0);
                    self.render_section(ui, DirKind::Tmp, notifications);
                    ui.separator();
                    self.render_section(ui, DirKind::Workspace, notifications);
                    ui.separator();
                    self.render_section(ui, DirKind::Sessions, notifications);
                    ui.separator();
                    self.render_section(ui, DirKind::Archives, notifications);
                    ui.separator();
                    self.render_section(ui, DirKind::Logs, notifications);
                    ui.separator();
                    self.render_section(ui, DirKind::Skills, notifications);
                    ui.separator();
                    self.render_section(ui, DirKind::SkillsRegistry, notifications);
                }
                SystemView::Environment => {
                    self.render_env_check_section(ui);
                }
            });

        self.render_clear_confirm_dialog(ui.ctx(), notifications);
    }
}

fn usage_text(loading: bool, bytes: Option<u64>, error: Option<&str>) -> String {
    if loading {
        "Calculating...".to_string()
    } else if let Some(b) = bytes {
        format!("Usage: {}", format_bytes(b))
    } else if let Some(err) = error {
        format!("Usage: unavailable ({err})")
    } else {
        "Usage: unavailable".to_string()
    }
}

#[cfg(target_os = "macos")]
fn open_directory_in_file_manager(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    Command::new("open").arg(path).spawn()?.wait()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn open_directory_in_file_manager(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    Command::new("explorer").arg(path).spawn()?.wait()?;
    Ok(())
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn open_directory_in_file_manager(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    Command::new("xdg-open").arg(path).spawn()?.wait()?;
    Ok(())
}

fn ensure_dir_exists(path: &PathBuf) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("failed to create directory: {err}"))
}

fn collect_dir_usage(path: &PathBuf) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| format!("failed to read directory metadata: {err}"))?;
    if !metadata.is_dir() {
        return Err("path is not a directory".to_string());
    }
    collect_path_usage(path)
}

fn collect_path_usage(path: &PathBuf) -> Result<u64, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|err| format!("failed to read metadata: {err}"))?;

    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }

    let mut total = 0_u64;
    let entries =
        fs::read_dir(path).map_err(|err| format!("failed to read directory entries: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        total = total.saturating_add(collect_path_usage(&entry.path())?);
    }
    Ok(total)
}

fn clear_directory(path: &PathBuf) -> Result<(), String> {
    ensure_dir_exists(path)?;

    let entries =
        fs::read_dir(path).map_err(|err| format!("failed to read directory entries: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {err}"))?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)
            .map_err(|err| format!("failed to read metadata: {err}"))?;

        if metadata.is_dir() {
            fs::remove_dir_all(&entry_path)
                .map_err(|err| format!("failed to remove directory: {err}"))?;
        } else {
            fs::remove_file(&entry_path).map_err(|err| format!("failed to remove file: {err}"))?;
        }
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    let mut value = bytes as f64;
    let mut unit_idx = 0_usize;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{bytes} {}", UNITS[unit_idx])
    } else {
        format!("{value:.2} {}", UNITS[unit_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::{clear_directory, collect_dir_usage};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("klaw-gui-system-panel-{name}-{suffix}"))
    }

    #[test]
    fn collect_dir_usage_sums_nested_file_sizes() {
        let root = temp_dir("usage");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create nested dir");
        fs::write(root.join("a.bin"), vec![0_u8; 10]).expect("write root file");
        fs::write(nested.join("b.bin"), vec![0_u8; 20]).expect("write nested file");

        let usage = collect_dir_usage(&root).expect("collect usage");
        assert_eq!(usage, 30);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn clear_directory_removes_children_but_keeps_root() {
        let root = temp_dir("clear");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create nested dir");
        fs::write(root.join("a.bin"), vec![0_u8; 10]).expect("write root file");
        fs::write(nested.join("b.bin"), vec![0_u8; 20]).expect("write nested file");

        clear_directory(&root).expect("clear directory");

        assert!(root.is_dir());
        assert_eq!(
            fs::read_dir(&root).expect("read root after clear").count(),
            0
        );

        fs::remove_dir_all(root).expect("cleanup");
    }
}
