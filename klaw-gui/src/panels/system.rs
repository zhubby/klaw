use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::current_ui_language;
use crate::{RuntimeRequestHandle, begin_env_check_request};
use egui::RichText;
use egui_dock::{AllowedSplits, DockArea, DockState, NodeIndex, Style, SurfaceIndex, TabIndex};
use egui_phosphor::regular;
use klaw_config::ConfigStore;
use klaw_storage::StoragePaths;
use klaw_ui_kit::{LocaleDomain, PieChart, PieChartPalette, PieSlice, Translator};
use klaw_util::{DependencyCategory, EnvironmentCheckReport, KLAW_DIR_NAME, default_data_dir};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

const TASK_POLL_INTERVAL: Duration = Duration::from_millis(200);
const HOST_INFO_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const DISK_USAGE_DIRS: [DirKind; 8] = [
    DirKind::Tmp,
    DirKind::Workspace,
    DirKind::Sessions,
    DirKind::Archives,
    DirKind::Logs,
    DirKind::Skills,
    DirKind::SkillsRegistry,
    DirKind::Models,
];

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
    let (disk_total_bytes, disk_available_bytes, mount_point) =
        host_disk_space_for_path(data_dir_path);

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

fn host_info_row(ui: &mut egui::Ui, row_index: usize, key: &str, value: String) {
    let column_spacing = 14.0;
    // Inside ScrollArea, available_width can describe the scroll content width rather
    // than the visible viewport. Use the clip rect so the split tracks what users see.
    let row_width = ui.clip_rect().width();
    let row_height = ui.spacing().interact_size.y;
    let col_width = ((row_width - column_spacing) / 2.0).max(0.0);
    let (row_rect, _) =
        ui.allocate_exact_size(egui::vec2(row_width, row_height), egui::Sense::hover());

    if row_index % 2 == 0 {
        ui.painter()
            .rect_filled(row_rect, 0.0, ui.visuals().faint_bg_color);
    }

    let key_rect = egui::Rect::from_min_size(row_rect.min, egui::vec2(col_width, row_height));
    let value_rect = egui::Rect::from_min_size(
        egui::pos2(row_rect.min.x + col_width + column_spacing, row_rect.min.y),
        egui::vec2(col_width, row_height),
    );

    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(key_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(egui::Label::new(key).truncate());
        },
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(value_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(egui::Label::new(RichText::new(value).monospace()).truncate());
        },
    );
}

#[allow(dead_code)]
fn host_optional_text(value: Option<String>) -> String {
    value.unwrap_or_else(|| "N/A".to_string())
}

fn host_optional_text_with_na(value: Option<String>, na: &str) -> String {
    value.unwrap_or_else(|| na.to_string())
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

impl SystemView {
    const ALL: [Self; 3] = [
        Self::HostInformation,
        Self::ProgramDiskUsage,
        Self::Environment,
    ];

    fn title_key(self) -> &'static str {
        match self {
            Self::HostInformation => "system-view-host-information",
            Self::ProgramDiskUsage => "system-view-program-disk-usage",
            Self::Environment => "system-view-environment",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::HostInformation => regular::DESKTOP_TOWER,
            Self::ProgramDiskUsage => regular::HARD_DRIVES,
            Self::Environment => regular::TERMINAL_WINDOW,
        }
    }

    fn tab_id(self) -> &'static str {
        match self {
            Self::HostInformation => "host-information",
            Self::ProgramDiskUsage => "program-disk-usage",
            Self::Environment => "environment",
        }
    }
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
    Models,
}

impl DirKind {
    #[allow(dead_code)]
    fn title(self) -> &'static str {
        match self {
            DirKind::Tmp => "Temporary",
            DirKind::Workspace => "Workspace",
            DirKind::Sessions => "Sessions",
            DirKind::Archives => "Archives",
            DirKind::Logs => "Logs",
            DirKind::Skills => "Skills",
            DirKind::SkillsRegistry => "Skills Registry",
            DirKind::Models => "Models",
        }
    }

    fn title_with_translator(self, t: &Translator) -> String {
        match self {
            DirKind::Tmp => t.text("system-dir-tmp"),
            DirKind::Workspace => t.text("system-dir-workspace"),
            DirKind::Sessions => t.text("system-dir-sessions"),
            DirKind::Archives => t.text("system-dir-archives"),
            DirKind::Logs => t.text("system-dir-logs"),
            DirKind::Skills => t.text("system-dir-skills"),
            DirKind::SkillsRegistry => t.text("system-dir-skills-registry"),
            DirKind::Models => t.text("system-dir-models"),
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
            DirKind::Models => "models",
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
            DirKind::Models => paths.models_dir.clone(),
        }
    }
}

#[derive(Default)]
struct DirState {
    usage_bytes: Option<u64>,
    usage_error: Option<String>,
    usage_rx: Option<Receiver<Result<u64, String>>>,
    clear_rx: Option<Receiver<Result<(), String>>>,
}

impl DirState {
    fn is_loading(&self) -> bool {
        self.usage_rx.is_some() || self.clear_rx.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DiskUsageChartEntry {
    label: String,
    bytes: u64,
}

pub struct SystemPanel {
    paths: Option<StoragePaths>,
    dirs: [DirState; 8],
    clear_confirm: Option<DirKind>,
    env_check: Option<EnvironmentCheckReport>,
    env_check_loaded: bool,
    env_check_request: Option<RuntimeRequestHandle<EnvironmentCheckReport>>,
    current_view: SystemView,
    view_dock_state: DockState<SystemView>,
    host_info: HostInfoData,
}

impl Default for SystemPanel {
    fn default() -> Self {
        let current_view = SystemView::HostInformation;
        Self {
            paths: None,
            dirs: std::array::from_fn(|_| DirState::default()),
            clear_confirm: None,
            env_check: None,
            env_check_loaded: false,
            env_check_request: None,
            current_view,
            view_dock_state: Self::view_dock_state(current_view),
            host_info: HostInfoData::default(),
        }
    }
}

impl SystemPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn view_dock_state(current_view: SystemView) -> DockState<SystemView> {
        let mut dock_state = DockState::new(SystemView::ALL.to_vec());
        let active_index = SystemView::ALL
            .iter()
            .position(|view| *view == current_view)
            .unwrap_or_default();
        dock_state.set_active_tab((
            SurfaceIndex::main(),
            NodeIndex::root(),
            TabIndex(active_index),
        ));
        dock_state
    }

    fn dir_index(kind: DirKind) -> usize {
        match kind {
            DirKind::Tmp => 0,
            DirKind::Workspace => 1,
            DirKind::Sessions => 2,
            DirKind::Archives => 3,
            DirKind::Logs => 4,
            DirKind::Skills => 5,
            DirKind::SkillsRegistry => 6,
            DirKind::Models => 7,
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

        let t = Self::translator();
        match StoragePaths::from_home_dir() {
            Ok(paths) => {
                self.paths = Some(paths);
            }
            Err(err) => {
                let message = t.text_args(
                    "system-notify-failed-resolve",
                    HashMap::from([("error", err.to_string())]),
                );
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
        for kind in DISK_USAGE_DIRS {
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
        let t = Self::translator();
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
                ui.strong(t.text("system-cpu-usage"));
                ui.horizontal(|ui| {
                    let bar = egui::ProgressBar::new((cpu_usage / 100.0).clamp(0.0, 1.0))
                        .show_percentage()
                        .desired_height(PROGRESS_BAR_HEIGHT);
                    ui.add(bar);
                    ui.monospace(format!("{cpu_usage:.1}%"));
                });
                ui.label(t.text_args(
                    "system-cpu-cores-info",
                    HashMap::from([
                        ("logical", logical_cpus.to_string()),
                        ("physical", physical_cores.to_string()),
                    ]),
                ));
            });

            cols[1].vertical(|ui| {
                ui.strong(t.text("system-memory-usage"));
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
                ui.label(t.text_args(
                    "system-memory-free",
                    HashMap::from([("free", format_bytes_si(free_memory))]),
                ));
            });
        });

        ui.separator();
        ui.strong(t.text("system-system-information"));
        ui.add_space(6.0);

        let na = t.text("system-host-na");
        let loading = t.text("system-host-loading");

        egui::ScrollArea::vertical()
            .id_salt("system-host-info-scroll")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut row_index = 0;
                let mut row = |ui: &mut egui::Ui, key: &str, value: String| {
                    host_info_row(ui, row_index, key, value);
                    row_index += 1;
                };

                row(
                    ui,
                    &t.text("system-host-app-uptime"),
                    format_host_duration(uptime_secs),
                );
                row(
                    ui,
                    &t.text("system-host-name"),
                    host_optional_text_with_na(System::host_name(), &na),
                );
                row(
                    ui,
                    &t.text("system-host-os-name"),
                    host_optional_text_with_na(System::name(), &na),
                );
                row(
                    ui,
                    &t.text("system-host-os-version"),
                    host_optional_text_with_na(System::os_version(), &na),
                );
                row(
                    ui,
                    &t.text("system-host-long-os-version"),
                    host_optional_text_with_na(System::long_os_version(), &na),
                );
                row(
                    ui,
                    &t.text("system-host-kernel-version"),
                    host_optional_text_with_na(System::kernel_version(), &na),
                );
                row(
                    ui,
                    &t.text("system-host-cpu-architecture"),
                    std::env::consts::ARCH.to_string(),
                );
                row(
                    ui,
                    &t.text("system-host-logical-cpu-count"),
                    logical_cpus.to_string(),
                );
                row(
                    ui,
                    &t.text("system-host-physical-core-count"),
                    physical_cores.to_string(),
                );
                row(
                    ui,
                    &t.text("system-host-primary-cpu-brand"),
                    self.host_info
                        .system
                        .cpus()
                        .first()
                        .map(|cpu| cpu.brand().to_string())
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| na.clone()),
                );
                row(
                    ui,
                    &t.text("system-host-primary-cpu-frequency"),
                    self.host_info
                        .system
                        .cpus()
                        .first()
                        .map(|cpu| {
                            t.text_args(
                                "system-cpu-frequency-mhz",
                                HashMap::from([("freq", cpu.frequency().to_string())]),
                            )
                        })
                        .unwrap_or_else(|| na.clone()),
                );
                row(
                    ui,
                    &t.text("system-host-total-memory"),
                    format_bytes_si(total_memory),
                );
                row(
                    ui,
                    &t.text("system-host-used-memory"),
                    format_bytes_si(used_memory),
                );
                row(
                    ui,
                    &t.text("system-host-free-memory"),
                    format_bytes_si(free_memory),
                );
                row(
                    ui,
                    &t.text("system-host-total-swap"),
                    format_bytes_si(self.host_info.system.total_swap()),
                );
                row(
                    ui,
                    &t.text("system-host-used-swap"),
                    format_bytes_si(self.host_info.system.used_swap()),
                );
                row(
                    ui,
                    &t.text("system-host-system-uptime"),
                    format_host_duration(system_uptime_secs),
                );
                row(
                    ui,
                    &t.text("system-host-system-boot-time"),
                    crate::time_format::format_timestamp_seconds(System::boot_time()),
                );
                row(
                    ui,
                    &t.text("system-host-load-average"),
                    format_host_load_avg(load_avg),
                );
                row(
                    ui,
                    &t.text("system-host-data-directory"),
                    self.host_info.data_dir_path.display().to_string(),
                );

                if let Some(stats) = self.host_info.data_dir_stats.as_ref() {
                    row(
                        ui,
                        &t.text("system-host-data-dir-size"),
                        format_bytes_si(stats.used_bytes),
                    );
                    row(
                        ui,
                        &t.text("system-host-data-dir-file-count"),
                        stats.file_count.to_string(),
                    );
                    row(
                        ui,
                        &t.text("system-host-data-dir-mount-point"),
                        stats
                            .mount_point
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| na.clone()),
                    );
                    row(
                        ui,
                        &t.text("system-host-data-dir-disk-capacity"),
                        format_bytes_si(stats.disk_total_bytes),
                    );
                    row(
                        ui,
                        &t.text("system-host-data-dir-disk-available"),
                        format_bytes_si(stats.disk_available_bytes),
                    );
                } else {
                    row(ui, &t.text("system-host-data-dir-size"), loading.clone());
                    row(
                        ui,
                        &t.text("system-host-data-dir-file-count"),
                        loading.clone(),
                    );
                    row(
                        ui,
                        &t.text("system-host-data-dir-mount-point"),
                        loading.clone(),
                    );
                    row(
                        ui,
                        &t.text("system-host-data-dir-disk-capacity"),
                        loading.clone(),
                    );
                    row(
                        ui,
                        &t.text("system-host-data-dir-disk-available"),
                        loading.clone(),
                    );
                }
            });
    }

    fn render_env_check_section(&mut self, ui: &mut egui::Ui) {
        let t = Self::translator();
        ui.strong(t.text("system-env-dependencies"));
        ui.add_space(4.0);

        let Some(report) = &self.env_check else {
            ui.label(t.text("system-env-loading"));
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
                } else {
                    warn_color
                };
                ui.label(RichText::new(icon).color(color).size(16.0));

                ui.label(RichText::new(&check.name).strong());

                if let Some(version) = &check.version {
                    ui.label(RichText::new(version).weak());
                } else {
                    ui.label(RichText::new(t.text("system-env-not-found")).weak());
                }

                let label = match check.category {
                    DependencyCategory::Required => t.text("system-env-required"),
                    DependencyCategory::Preferred => t.text("system-env-preferred"),
                    DependencyCategory::OptionalWithFallback => t.text("system-env-optional"),
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
                    ui.label(RichText::new(t.text("system-env-project")).small().weak());
                    ui.hyperlink_to(RichText::new(project_url).small(), project_url);
                });
            }
            ui.add_space(4.0);
        }

        if all_required_ok && preferred_ok && tm_ok {
            ui.label(RichText::new(t.text("system-env-all-available")).color(success_color));
        } else if all_required_ok && preferred_ok {
            ui.label(RichText::new(t.text("system-env-tm-missing")).color(warn_color));
        } else if all_required_ok {
            ui.label(RichText::new(t.text("system-env-preferred-missing")).color(warn_color));
        } else {
            ui.label(RichText::new(t.text("system-env-required-missing")).color(error_color));
        }
    }

    fn poll_tasks(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
        for kind in DISK_USAGE_DIRS {
            let dir = self.get_dir_mut(kind);
            let title = kind.title_with_translator(&t);

            if let Some(rx) = dir.usage_rx.as_ref()
                && let Ok(result) = rx.try_recv()
            {
                dir.usage_rx = None;
                match result {
                    Ok(bytes) => {
                        dir.usage_bytes = Some(bytes);
                        dir.usage_error = None;
                    }
                    Err(err) => {
                        dir.usage_bytes = None;
                        dir.usage_error = Some(err.clone());
                        notifications.error(t.text_args(
                            "system-notify-failed-collect-usage",
                            HashMap::from([("title", title.clone()), ("error", err.to_string())]),
                        ));
                    }
                }
            }

            if let Some(rx) = dir.clear_rx.as_ref()
                && let Ok(result) = rx.try_recv()
            {
                dir.clear_rx = None;
                match result {
                    Ok(()) => {
                        dir.usage_bytes = Some(0);
                        notifications.success(t.text_args(
                            "system-notify-dir-cleared",
                            HashMap::from([("title", title.clone())]),
                        ));
                        self.refresh_usage(kind);
                    }
                    Err(err) => {
                        notifications.error(t.text_args(
                            "system-notify-failed-clear-dir",
                            HashMap::from([("title", title.clone()), ("error", err.to_string())]),
                        ));
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

        let t = Self::translator();
        let title = kind.title_with_translator(&t);

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(t.text_args(
            "system-confirm-clear-title",
            HashMap::from([("title", title.clone())]),
        ))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(t.text_args(
                "system-confirm-clear-message",
                HashMap::from([("title", title.clone())]),
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t.text("system-clear")).clicked() {
                    confirmed = true;
                }
                if ui.button(t.text("system-cancel")).clicked() {
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
        let t = Self::translator();
        let title = kind.title_with_translator(&t);
        ui.strong(&title);
        ui.add_space(4.0);

        let Some(paths) = self.paths.as_ref() else {
            ui.label(t.text("system-dir-path-unavailable"));
            return;
        };

        let path = kind.path(paths);
        ui.label(t.text_args(
            "system-dir-path",
            HashMap::from([("path", path.display().to_string())]),
        ));
        ui.add_space(6.0);

        let dir = self.get_dir(kind);
        let usage_loading = dir.usage_rx.is_some();
        let clear_loading = dir.clear_rx.is_some();
        let usage_bytes = dir.usage_bytes;
        let usage_error = dir.usage_error.clone();

        ui.horizontal(|ui| {
            let usage_str =
                usage_text_with_translator(&t, usage_loading, usage_bytes, usage_error.as_deref());
            ui.label(RichText::new(usage_str).strong());

            if ui
                .add_enabled(
                    !usage_loading && !clear_loading,
                    egui::Button::new(format!(
                        "{} {}",
                        regular::ARROW_CLOCKWISE,
                        t.text("system-refresh")
                    )),
                )
                .clicked()
            {
                self.refresh_usage(kind);
            }

            if ui
                .button(regular::FOLDER_OPEN)
                .on_hover_text(t.text_args(
                    "system-open-dir-hint",
                    HashMap::from([("title", title.clone())]),
                ))
                .clicked()
                && let Err(err) = open_directory_in_file_manager(&path)
            {
                notifications.error(t.text_args(
                    "system-notify-failed-open-dir",
                    HashMap::from([("title", title.clone()), ("error", err.to_string())]),
                ));
            }

            if ui
                .add_enabled(
                    !clear_loading && !usage_loading,
                    egui::Button::new(regular::TRASH)
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12)),
                )
                .on_hover_text(t.text_args(
                    "system-clear-dir-hint",
                    HashMap::from([("title", title.clone())]),
                ))
                .clicked()
            {
                self.clear_confirm = Some(kind);
            }
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new(t.text_args(
                "system-dir-clearing-hint",
                HashMap::from([("dir", kind.dir_name().to_string())]),
            ))
            .weak()
            .small(),
        );
    }

    fn render_program_disk_usage(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        t: &Translator,
    ) {
        ui.columns(2, |cols| {
            cols[0].vertical(|ui| {
                ui.label(t.text("system-disk-usage-description"));
                ui.add_space(8.0);

                for (index, kind) in DISK_USAGE_DIRS.into_iter().enumerate() {
                    if index > 0 {
                        ui.separator();
                    }
                    self.render_section(ui, kind, notifications);
                }
            });

            cols[1].vertical(|ui| {
                self.render_disk_usage_chart(ui, t);
            });
        });
    }

    fn disk_usage_chart_entries(&self, t: &Translator) -> Vec<DiskUsageChartEntry> {
        DISK_USAGE_DIRS
            .into_iter()
            .filter_map(|kind| {
                let bytes = self.get_dir(kind).usage_bytes?;
                (bytes > 0).then(|| DiskUsageChartEntry {
                    label: kind.title_with_translator(t),
                    bytes,
                })
            })
            .collect()
    }

    fn disk_usage_pie_slices(&self, t: &Translator) -> Vec<PieSlice> {
        self.disk_usage_chart_entries(t)
            .into_iter()
            .map(|entry| PieSlice::new(entry.label, entry.bytes as f32))
            .collect()
    }

    fn disk_usage_chart_total_bytes(&self) -> u64 {
        DISK_USAGE_DIRS
            .into_iter()
            .filter_map(|kind| self.get_dir(kind).usage_bytes)
            .filter(|bytes| *bytes > 0)
            .fold(0_u64, u64::saturating_add)
    }

    fn render_disk_usage_chart(&self, ui: &mut egui::Ui, t: &Translator) {
        ui.strong(t.text("system-disk-usage-chart-title"));
        ui.add_space(8.0);

        let entries = self.disk_usage_chart_entries(t);
        let total_bytes = self.disk_usage_chart_total_bytes();

        if entries.is_empty() {
            let message = if self.dirs.iter().any(|dir| dir.usage_rx.is_some()) {
                t.text("system-disk-usage-chart-loading")
            } else {
                t.text("system-disk-usage-chart-empty")
            };
            ui.label(RichText::new(message).weak());
            return;
        }

        let slices = self.disk_usage_pie_slices(t);
        let chart_side = ui.available_width().min(320.0).max(160.0);
        ui.add(
            PieChart::new(&slices)
                .palette(PieChartPalette::Tableau)
                .show_labels(true)
                .show_separators(true)
                .desired_size(egui::vec2(chart_side, chart_side)),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(t.text_args(
            "system-disk-usage-chart-total",
            HashMap::from([("total", format_bytes(total_bytes))]),
        )));
        ui.add_space(8.0);

        for (index, entry) in entries.iter().enumerate() {
            let color = PieChartPalette::Tableau.slice_color(index, entries.len());
            let percentage = if total_bytes == 0 {
                0.0
            } else {
                (entry.bytes as f64 / total_bytes as f64) * 100.0
            };

            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, color);
                ui.label(&entry.label);
                ui.label(
                    RichText::new(format!("{} ({percentage:.1}%)", format_bytes(entry.bytes)))
                        .monospace()
                        .weak(),
                );
            });
        }
    }

    fn render_view_dock(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        t: &Translator,
    ) {
        let mut dock_state = std::mem::replace(
            &mut self.view_dock_state,
            Self::view_dock_state(self.current_view),
        );
        let mut style = Style::from_egui(ui.style().as_ref());
        style.tab_bar.show_scroll_bar_on_overflow = false;

        DockArea::new(&mut dock_state)
            .id(egui::Id::new("system-view-dock"))
            .style(style)
            .show_add_buttons(false)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .tab_context_menus(false)
            .draggable_tabs(false)
            .allowed_splits(AllowedSplits::None)
            .show_inside(
                ui,
                &mut SystemViewTabViewer {
                    panel: self,
                    notifications,
                    translator: t,
                },
            );

        self.view_dock_state = dock_state;
    }
}

struct SystemViewTabViewer<'a> {
    panel: &'a mut SystemPanel,
    notifications: &'a mut NotificationCenter,
    translator: &'a Translator,
}

impl egui_dock::TabViewer for SystemViewTabViewer<'_> {
    type Tab = SystemView;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        format!("{} {}", tab.icon(), self.translator.text(tab.title_key())).into()
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("system-view-tab", tab.tab_id()))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        self.panel.current_view = *tab;
        egui::ScrollArea::vertical()
            .id_salt(("system-view-scroll", tab.tab_id()))
            .auto_shrink([false, false])
            .show(ui, |ui| match *tab {
                SystemView::HostInformation => {
                    self.panel.render_host_information(ui);
                }
                SystemView::ProgramDiskUsage => {
                    self.panel
                        .render_program_disk_usage(ui, self.notifications, self.translator);
                }
                SystemView::Environment => {
                    self.panel.render_env_check_section(ui);
                }
            });
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        if response.clicked() {
            self.panel.current_view = *tab;
        }
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
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

        let t = Self::translator();
        ui.heading(ctx.tab_title);
        ui.separator();

        self.render_view_dock(ui, notifications, &t);

        self.render_clear_confirm_dialog(ui.ctx(), notifications);
    }
}

fn usage_text_with_translator(
    t: &Translator,
    loading: bool,
    bytes: Option<u64>,
    error: Option<&str>,
) -> String {
    if loading {
        t.text("system-usage-calculating")
    } else if let Some(b) = bytes {
        t.text_args("system-usage", HashMap::from([("usage", format_bytes(b))]))
    } else if let Some(err) = error {
        t.text_args(
            "system-usage-unavailable-error",
            HashMap::from([("error", err.to_string())]),
        )
    } else {
        t.text("system-usage-unavailable")
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
    use super::{DirKind, SystemPanel, clear_directory, collect_dir_usage};
    use klaw_ui_kit::{LocaleDomain, Translator, UiLanguage};
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

    #[test]
    fn disk_usage_pie_slices_filter_missing_and_zero_usage() {
        let mut panel = SystemPanel::default();
        panel.dirs[SystemPanel::dir_index(DirKind::Tmp)].usage_bytes = Some(0);
        panel.dirs[SystemPanel::dir_index(DirKind::Workspace)].usage_bytes = Some(128);
        panel.dirs[SystemPanel::dir_index(DirKind::Sessions)].usage_bytes = None;
        panel.dirs[SystemPanel::dir_index(DirKind::Logs)].usage_bytes = Some(256);
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);

        let slices = panel.disk_usage_pie_slices(&translator);

        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].label, "Workspace");
        assert_eq!(slices[0].value, 128.0);
        assert_eq!(slices[1].label, "Logs");
        assert_eq!(slices[1].value, 256.0);
    }

    #[test]
    fn disk_usage_pie_slices_keep_directory_display_order() {
        let mut panel = SystemPanel::default();
        panel.dirs[SystemPanel::dir_index(DirKind::Models)].usage_bytes = Some(1);
        panel.dirs[SystemPanel::dir_index(DirKind::Tmp)].usage_bytes = Some(2);
        panel.dirs[SystemPanel::dir_index(DirKind::Archives)].usage_bytes = Some(3);
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);

        let labels = panel
            .disk_usage_pie_slices(&translator)
            .into_iter()
            .map(|slice| slice.label)
            .collect::<Vec<_>>();

        assert_eq!(labels, vec!["Temporary", "Archives", "Models"]);
    }

    #[test]
    fn disk_usage_chart_total_bytes_counts_only_positive_usage() {
        let mut panel = SystemPanel::default();
        panel.dirs[SystemPanel::dir_index(DirKind::Tmp)].usage_bytes = Some(0);
        panel.dirs[SystemPanel::dir_index(DirKind::Workspace)].usage_bytes = Some(10);
        panel.dirs[SystemPanel::dir_index(DirKind::Sessions)].usage_bytes = None;
        panel.dirs[SystemPanel::dir_index(DirKind::Archives)].usage_bytes = Some(20);

        assert_eq!(panel.disk_usage_chart_total_bytes(), 30);
    }
}
