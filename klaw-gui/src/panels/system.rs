use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::RichText;
use egui_phosphor::regular;
use klaw_storage::StoragePaths;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const TASK_POLL_INTERVAL: Duration = Duration::from_millis(200);

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

    fn render_section(&mut self, ui: &mut egui::Ui, kind: DirKind) {
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
                .add_enabled(
                    !clear_loading && !usage_loading,
                    egui::Button::new(regular::TRASH)
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12)),
                )
                .on_hover_text(format!("Clear {} directory", kind.title()))
                .clicked()
            {
                self.clear_dir(kind);
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
        self.poll_tasks(notifications);

        if self.any_loading() {
            ui.ctx().request_repaint_after(TASK_POLL_INTERVAL);
        }

        ui.heading(ctx.tab_title);
        ui.label("Inspect and clear data under the Klaw data directory.");
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("system-panel-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.render_section(ui, DirKind::Tmp);
                ui.separator();
                self.render_section(ui, DirKind::Workspace);
                ui.separator();
                self.render_section(ui, DirKind::Sessions);
                ui.separator();
                self.render_section(ui, DirKind::Archives);
                ui.separator();
                self.render_section(ui, DirKind::Logs);
                ui.separator();
                self.render_section(ui, DirKind::Skills);
                ui.separator();
                self.render_section(ui, DirKind::SkillsRegistry);
            });
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
