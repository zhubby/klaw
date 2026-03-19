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

#[derive(Default)]
pub struct SystemPanel {
    paths: Option<StoragePaths>,

    tmp_usage_bytes: Option<u64>,
    tmp_usage_error: Option<String>,
    tmp_usage_rx: Option<Receiver<Result<u64, String>>>,
    tmp_clear_rx: Option<Receiver<Result<(), String>>>,

    workspace_usage_bytes: Option<u64>,
    workspace_usage_error: Option<String>,
    workspace_usage_rx: Option<Receiver<Result<u64, String>>>,
    workspace_clear_rx: Option<Receiver<Result<(), String>>>,

    sessions_usage_bytes: Option<u64>,
    sessions_usage_error: Option<String>,
    sessions_usage_rx: Option<Receiver<Result<u64, String>>>,
    sessions_clear_rx: Option<Receiver<Result<(), String>>>,
}

impl SystemPanel {
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
                self.tmp_usage_error = Some(message.clone());
                notifications.error(message);
            }
        }
    }

    fn any_loading(&self) -> bool {
        self.tmp_usage_rx.is_some()
            || self.tmp_clear_rx.is_some()
            || self.workspace_usage_rx.is_some()
            || self.workspace_clear_rx.is_some()
            || self.sessions_usage_rx.is_some()
            || self.sessions_clear_rx.is_some()
    }

    fn refresh_tmp_usage(&mut self) {
        let Some(paths) = self.paths.as_ref() else { return };
        let path = paths.tmp_dir.clone();

        let (tx, rx) = mpsc::channel();
        self.tmp_usage_rx = Some(rx);
        self.tmp_usage_error = None;

        thread::spawn(move || {
            let result = ensure_dir_exists(&path).and_then(|()| collect_dir_usage(&path));
            let _ = tx.send(result);
        });
    }

    fn clear_tmp_dir(&mut self) {
        let Some(paths) = self.paths.as_ref() else { return };
        let path = paths.tmp_dir.clone();

        let (tx, rx) = mpsc::channel();
        self.tmp_clear_rx = Some(rx);

        thread::spawn(move || {
            let _ = tx.send(clear_directory(&path));
        });
    }

    fn refresh_workspace_usage(&mut self) {
        let Some(paths) = self.paths.as_ref() else { return };
        let path = paths.workspace_dir.clone();

        let (tx, rx) = mpsc::channel();
        self.workspace_usage_rx = Some(rx);
        self.workspace_usage_error = None;

        thread::spawn(move || {
            let result = ensure_dir_exists(&path).and_then(|()| collect_dir_usage(&path));
            let _ = tx.send(result);
        });
    }

    fn clear_workspace_dir(&mut self) {
        let Some(paths) = self.paths.as_ref() else { return };
        let path = paths.workspace_dir.clone();

        let (tx, rx) = mpsc::channel();
        self.workspace_clear_rx = Some(rx);

        thread::spawn(move || {
            let _ = tx.send(clear_directory(&path));
        });
    }

    fn refresh_sessions_usage(&mut self) {
        let Some(paths) = self.paths.as_ref() else { return };
        let path = paths.sessions_dir.clone();

        let (tx, rx) = mpsc::channel();
        self.sessions_usage_rx = Some(rx);
        self.sessions_usage_error = None;

        thread::spawn(move || {
            let result = ensure_dir_exists(&path).and_then(|()| collect_dir_usage(&path));
            let _ = tx.send(result);
        });
    }

    fn clear_sessions_dir(&mut self) {
        let Some(paths) = self.paths.as_ref() else { return };
        let path = paths.sessions_dir.clone();

        let (tx, rx) = mpsc::channel();
        self.sessions_clear_rx = Some(rx);

        thread::spawn(move || {
            let _ = tx.send(clear_directory(&path));
        });
    }

    fn ensure_initial_usage_loaded(&mut self) {
        if self.tmp_usage_bytes.is_none() && self.tmp_usage_rx.is_none() {
            self.refresh_tmp_usage();
        }
        if self.workspace_usage_bytes.is_none() && self.workspace_usage_rx.is_none() {
            self.refresh_workspace_usage();
        }
        if self.sessions_usage_bytes.is_none() && self.sessions_usage_rx.is_none() {
            self.refresh_sessions_usage();
        }
    }

    fn poll_tasks(&mut self, notifications: &mut NotificationCenter) {
        // Poll tmp
        if let Some(rx) = self.tmp_usage_rx.as_ref() {
            if let Ok(result) = rx.try_recv() {
                self.tmp_usage_rx = None;
                match result {
                    Ok(bytes) => {
                        self.tmp_usage_bytes = Some(bytes);
                        self.tmp_usage_error = None;
                    }
                    Err(err) => {
                        self.tmp_usage_bytes = None;
                        self.tmp_usage_error = Some(err.clone());
                        notifications.error(format!("Failed to collect tmp usage: {err}"));
                    }
                }
            }
        }
        if let Some(rx) = self.tmp_clear_rx.as_ref() {
            if let Ok(result) = rx.try_recv() {
                self.tmp_clear_rx = None;
                match result {
                    Ok(()) => {
                        self.tmp_usage_bytes = Some(0);
                        notifications.success("Temporary directory cleared");
                        self.refresh_tmp_usage();
                    }
                    Err(err) => {
                        notifications.error(format!("Failed to clear tmp directory: {err}"));
                    }
                }
            }
        }

        // Poll workspace
        if let Some(rx) = self.workspace_usage_rx.as_ref() {
            if let Ok(result) = rx.try_recv() {
                self.workspace_usage_rx = None;
                match result {
                    Ok(bytes) => {
                        self.workspace_usage_bytes = Some(bytes);
                        self.workspace_usage_error = None;
                    }
                    Err(err) => {
                        self.workspace_usage_bytes = None;
                        self.workspace_usage_error = Some(err.clone());
                        notifications.error(format!("Failed to collect workspace usage: {err}"));
                    }
                }
            }
        }
        if let Some(rx) = self.workspace_clear_rx.as_ref() {
            if let Ok(result) = rx.try_recv() {
                self.workspace_clear_rx = None;
                match result {
                    Ok(()) => {
                        self.workspace_usage_bytes = Some(0);
                        notifications.success("Workspace directory cleared");
                        self.refresh_workspace_usage();
                    }
                    Err(err) => {
                        notifications.error(format!("Failed to clear workspace directory: {err}"));
                    }
                }
            }
        }

        // Poll sessions
        if let Some(rx) = self.sessions_usage_rx.as_ref() {
            if let Ok(result) = rx.try_recv() {
                self.sessions_usage_rx = None;
                match result {
                    Ok(bytes) => {
                        self.sessions_usage_bytes = Some(bytes);
                        self.sessions_usage_error = None;
                    }
                    Err(err) => {
                        self.sessions_usage_bytes = None;
                        self.sessions_usage_error = Some(err.clone());
                        notifications.error(format!("Failed to collect sessions usage: {err}"));
                    }
                }
            }
        }
        if let Some(rx) = self.sessions_clear_rx.as_ref() {
            if let Ok(result) = rx.try_recv() {
                self.sessions_clear_rx = None;
                match result {
                    Ok(()) => {
                        self.sessions_usage_bytes = Some(0);
                        notifications.success("Sessions directory cleared");
                        self.refresh_sessions_usage();
                    }
                    Err(err) => {
                        notifications.error(format!("Failed to clear sessions directory: {err}"));
                    }
                }
            }
        }
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

        self.render_tmp_section(ui);
        ui.separator();
        self.render_workspace_section(ui);
        ui.separator();
        self.render_sessions_section(ui);
    }
}

impl SystemPanel {
    fn render_tmp_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Temporary Directory");
        ui.add_space(4.0);

        let Some(paths) = self.paths.as_ref() else {
            ui.label("Path unavailable.");
            return;
        };

        ui.label(format!("Path: {}", paths.tmp_dir.display()));
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let usage_text = usage_text(
                self.tmp_usage_rx.is_some(),
                self.tmp_usage_bytes,
                self.tmp_usage_error.as_deref(),
            );
            ui.label(RichText::new(usage_text).strong());

            if ui
                .add_enabled(
                    !self.tmp_usage_rx.is_some() && !self.tmp_clear_rx.is_some(),
                    egui::Button::new(format!("{} Refresh", regular::ARROW_CLOCKWISE)),
                )
                .clicked()
            {
                self.refresh_tmp_usage();
            }

            if ui
                .add_enabled(
                    !self.tmp_clear_rx.is_some() && !self.tmp_usage_rx.is_some(),
                    egui::Button::new(regular::TRASH)
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12)),
                )
                .on_hover_text("Clear temporary directory")
                .clicked()
            {
                self.clear_tmp_dir();
            }
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new("Clearing removes files inside `tmp/`; the directory itself is kept.")
                .weak()
                .small(),
        );
    }

    fn render_workspace_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Workspace");
        ui.add_space(4.0);

        let Some(paths) = self.paths.as_ref() else {
            ui.label("Path unavailable.");
            return;
        };

        ui.label(format!("Path: {}", paths.workspace_dir.display()));
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let usage_text = usage_text(
                self.workspace_usage_rx.is_some(),
                self.workspace_usage_bytes,
                self.workspace_usage_error.as_deref(),
            );
            ui.label(RichText::new(usage_text).strong());

            if ui
                .add_enabled(
                    !self.workspace_usage_rx.is_some() && !self.workspace_clear_rx.is_some(),
                    egui::Button::new(format!("{} Refresh", regular::ARROW_CLOCKWISE)),
                )
                .clicked()
            {
                self.refresh_workspace_usage();
            }

            if ui
                .add_enabled(
                    !self.workspace_clear_rx.is_some() && !self.workspace_usage_rx.is_some(),
                    egui::Button::new(regular::TRASH)
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12)),
                )
                .on_hover_text("Clear workspace directory")
                .clicked()
            {
                self.clear_workspace_dir();
            }
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new("Clearing removes files inside `workspace/`; the directory itself is kept.")
                .weak()
                .small(),
        );
    }

    fn render_sessions_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Sessions");
        ui.add_space(4.0);

        let Some(paths) = self.paths.as_ref() else {
            ui.label("Path unavailable.");
            return;
        };

        ui.label(format!("Path: {}", paths.sessions_dir.display()));
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let usage_text = usage_text(
                self.sessions_usage_rx.is_some(),
                self.sessions_usage_bytes,
                self.sessions_usage_error.as_deref(),
            );
            ui.label(RichText::new(usage_text).strong());

            if ui
                .add_enabled(
                    !self.sessions_usage_rx.is_some() && !self.sessions_clear_rx.is_some(),
                    egui::Button::new(format!("{} Refresh", regular::ARROW_CLOCKWISE)),
                )
                .clicked()
            {
                self.refresh_sessions_usage();
            }

            if ui
                .add_enabled(
                    !self.sessions_clear_rx.is_some() && !self.sessions_usage_rx.is_some(),
                    egui::Button::new(regular::TRASH)
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12)),
                )
                .on_hover_text("Clear sessions directory")
                .clicked()
            {
                self.clear_sessions_dir();
            }
        });

        ui.add_space(2.0);
        ui.label(
            RichText::new("Clearing removes files inside `sessions/`; the directory itself is kept.")
                .weak()
                .small(),
        );
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