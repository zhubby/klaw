use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::RichText;
use egui_phosphor::regular;
use klaw_storage::StoragePaths;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const TASK_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Default)]
pub struct SystemPanel {
    tmp_dir_path: Option<PathBuf>,
    usage_bytes: Option<u64>,
    usage_error: Option<String>,
    usage_rx: Option<Receiver<Result<u64, String>>>,
    clear_rx: Option<Receiver<Result<(), String>>>,
}

impl SystemPanel {
    fn ensure_tmp_dir_path(&mut self, notifications: &mut NotificationCenter) {
        if self.tmp_dir_path.is_some() {
            return;
        }

        match StoragePaths::from_home_dir() {
            Ok(paths) => {
                self.tmp_dir_path = Some(paths.tmp_dir);
            }
            Err(err) => {
                let message = format!("Failed to resolve temporary directory: {err}");
                self.usage_error = Some(message.clone());
                notifications.error(message);
            }
        }
    }

    fn usage_loading(&self) -> bool {
        self.usage_rx.is_some()
    }

    fn clear_loading(&self) -> bool {
        self.clear_rx.is_some()
    }

    fn refresh_usage(&mut self) {
        let Some(tmp_dir_path) = self.tmp_dir_path.clone() else {
            return;
        };

        let (tx, rx) = mpsc::channel();
        self.usage_rx = Some(rx);
        self.usage_error = None;

        thread::spawn(move || {
            let result =
                ensure_dir_exists(&tmp_dir_path).and_then(|()| collect_dir_usage(&tmp_dir_path));
            let _ = tx.send(result);
        });
    }

    fn clear_tmp_dir(&mut self) {
        let Some(tmp_dir_path) = self.tmp_dir_path.clone() else {
            return;
        };

        let (tx, rx) = mpsc::channel();
        self.clear_rx = Some(rx);

        thread::spawn(move || {
            let result = clear_directory(&tmp_dir_path);
            let _ = tx.send(result);
        });
    }

    fn ensure_initial_usage_loaded(&mut self) {
        if self.usage_bytes.is_some() || self.usage_loading() {
            return;
        }
        self.refresh_usage();
    }

    fn poll_tasks(&mut self, notifications: &mut NotificationCenter) {
        if let Some(rx) = self.usage_rx.as_ref() {
            match rx.try_recv() {
                Ok(result) => {
                    self.usage_rx = None;
                    match result {
                        Ok(usage_bytes) => {
                            self.usage_bytes = Some(usage_bytes);
                            self.usage_error = None;
                        }
                        Err(err) => {
                            self.usage_bytes = None;
                            self.usage_error = Some(err.clone());
                            notifications.error(format!("Failed to collect tmp usage: {err}"));
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.usage_rx = None;
                    let message = "Temporary directory usage task disconnected".to_string();
                    self.usage_error = Some(message.clone());
                    notifications.error(message);
                }
            }
        }

        if let Some(rx) = self.clear_rx.as_ref() {
            match rx.try_recv() {
                Ok(result) => {
                    self.clear_rx = None;
                    match result {
                        Ok(()) => {
                            self.usage_bytes = Some(0);
                            notifications.success("Temporary directory cleared");
                            self.refresh_usage();
                        }
                        Err(err) => {
                            notifications.error(format!("Failed to clear tmp directory: {err}"));
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.clear_rx = None;
                    notifications.error("Temporary directory cleanup task disconnected");
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
        self.ensure_tmp_dir_path(notifications);
        self.ensure_initial_usage_loaded();
        self.poll_tasks(notifications);

        if self.usage_loading() || self.clear_loading() {
            ui.ctx().request_repaint_after(TASK_POLL_INTERVAL);
        }

        ui.heading(ctx.tab_title);
        ui.label("Inspect and clear temporary data under the Klaw data directory.");
        ui.separator();

        let Some(tmp_dir_path) = self.tmp_dir_path.as_ref() else {
            ui.label("Temporary directory is unavailable.");
            return;
        };

        ui.label(format!("Path: {}", tmp_dir_path.display()));
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let usage_text = if self.usage_loading() {
                "Calculating...".to_string()
            } else if let Some(bytes) = self.usage_bytes {
                format!("Usage: {}", format_bytes(bytes))
            } else if let Some(err) = self.usage_error.as_ref() {
                format!("Usage: unavailable ({err})")
            } else {
                "Usage: unavailable".to_string()
            };

            ui.label(RichText::new(usage_text).strong());

            if ui
                .add_enabled(
                    !self.usage_loading() && !self.clear_loading(),
                    egui::Button::new(format!("{} Refresh", regular::ARROW_CLOCKWISE)),
                )
                .clicked()
            {
                self.refresh_usage();
            }

            if ui
                .add_enabled(
                    !self.clear_loading() && !self.usage_loading(),
                    egui::Button::new(regular::TRASH)
                        .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12)),
                )
                .on_hover_text("Clear temporary directory")
                .clicked()
            {
                self.clear_tmp_dir();
            }
        });

        ui.add_space(8.0);
        ui.label(
            "Clearing only removes files and folders inside `tmp/`; the directory itself is kept.",
        );
    }
}

fn ensure_dir_exists(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|err| format!("failed to create directory: {err}"))
}

fn collect_dir_usage(path: &Path) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| format!("failed to read directory metadata: {err}"))?;
    if !metadata.is_dir() {
        return Err("temporary path is not a directory".to_string());
    }

    collect_path_usage(path)
}

fn collect_path_usage(path: &Path) -> Result<u64, String> {
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

fn clear_directory(path: &Path) -> Result<(), String> {
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
