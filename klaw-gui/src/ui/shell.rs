use crate::autostart::{self, ReconcileOutcome};
use crate::notifications::NotificationCenter;
use crate::panels::PanelRegistry;
use crate::runtime_bridge::{ProviderRuntimeSnapshot, request_provider_status};
use crate::settings::{AppSettings, SyncMode, load_settings, save_settings};
use crate::state::{ThemeMode, UiAction, UiState};
use crate::sync_runtime::{
    SyncRuntimeTaskKind, sync_runtime_finish_task, sync_runtime_set_last_snapshot,
    sync_runtime_set_remote_snapshots, sync_runtime_set_remote_update,
    sync_runtime_sync_from_settings, sync_runtime_try_start_task,
};
use crate::ui::{sidebar, workbench};
use egui_phosphor::regular;
use klaw_storage::{
    BackupItem, BackupPlan, BackupService, S3SnapshotStoreConfig, SnapshotListItem, SnapshotMode,
};
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

pub struct ShellUi {
    panels: PanelRegistry,
    notifications: NotificationCenter,
    provider_ids: Vec<String>,
    config_default_provider: String,
    provider_default_models: BTreeMap<String, String>,
    runtime_provider_override: Option<String>,
    last_provider_sync_at: Instant,
    sync_supervisor: SyncSupervisor,
}

const PROVIDER_SYNC_INTERVAL: Duration = Duration::from_secs(2);
const SYNC_POLL_INTERVAL: Duration = Duration::from_secs(5);

impl Default for ShellUi {
    fn default() -> Self {
        let mut notifications = NotificationCenter::default();
        let settings = load_settings();
        match autostart::reconcile(settings.general.launch_at_startup) {
            Ok(ReconcileOutcome::Unchanged) => {}
            Ok(ReconcileOutcome::Enabled) => {
                notifications.info("Launch at startup was re-synced with macOS login items.");
            }
            Ok(ReconcileOutcome::Disabled) => {
                notifications.info("Removed stale macOS login item for launch at startup.");
            }
            Err(err) if settings.general.launch_at_startup => {
                notifications.warning(format!(
                    "Launch at startup is enabled in settings but could not be refreshed: {err}"
                ));
            }
            Err(err) => {
                notifications.error(format!("Failed to sync launch at startup: {err}"));
            }
        }

        Self {
            panels: PanelRegistry::default(),
            notifications,
            provider_ids: Vec::new(),
            config_default_provider: String::new(),
            provider_default_models: BTreeMap::new(),
            runtime_provider_override: None,
            last_provider_sync_at: Instant::now() - PROVIDER_SYNC_INTERVAL,
            sync_supervisor: SyncSupervisor::default(),
        }
    }
}

impl ShellUi {
    pub fn show_info(&mut self, message: impl Into<String>) {
        self.notifications.info(message);
    }

    pub fn show_error(&mut self, message: impl Into<String>) {
        self.notifications.error(message);
    }

    pub fn set_runtime_provider_override(&mut self, provider_id: Option<String>) {
        self.runtime_provider_override = provider_id;
    }

    fn sync_provider_choices(&mut self) {
        if self.last_provider_sync_at.elapsed() < PROVIDER_SYNC_INTERVAL {
            return;
        }
        self.last_provider_sync_at = Instant::now();
        if let Ok(snapshot) = request_provider_status() {
            self.apply_provider_snapshot(snapshot);
        }
    }

    fn apply_provider_snapshot(&mut self, snapshot: ProviderRuntimeSnapshot) {
        self.config_default_provider = snapshot.default_provider_id;
        self.runtime_provider_override = snapshot.runtime_provider_override;
        self.provider_ids = snapshot
            .provider_default_models
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        self.provider_ids.sort();
        self.provider_default_models = snapshot.provider_default_models;
    }

    pub fn render(&mut self, ctx: &egui::Context, state: &UiState) -> Vec<UiAction> {
        let mut actions = Vec::new();
        self.panels.tick(ctx);
        self.sync_provider_choices();
        if self.runtime_provider_override != state.runtime_provider_override {
            actions.push(UiAction::SetRuntimeProviderOverride(
                self.runtime_provider_override.clone(),
            ));
        }
        self.sync_supervisor.tick(&mut self.notifications);
        ctx.request_repaint_after(SYNC_POLL_INTERVAL);

        egui::TopBottomPanel::top("klaw-menu-bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Force Persist Layout").clicked() {
                        actions.push(UiAction::ForcePersistLayout);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Hide Window").clicked() {
                        actions.push(UiAction::HideWindow);
                        ui.close();
                    }
                });

                ui.menu_button("View", |ui| {
                    let label = if state.fullscreen {
                        "Exit Full Windows"
                    } else {
                        "Toggle Full Windows"
                    };
                    if ui.button(label).clicked() {
                        actions.push(UiAction::ToggleFullscreen);
                        ui.close();
                    }
                });

                ui.menu_button("Windows", |ui| {
                    if ui.button("Minimize").clicked() {
                        actions.push(UiAction::MinimizeWindow);
                        ui.close();
                    }
                    if ui.button("Zoom").clicked() {
                        actions.push(UiAction::ZoomWindow);
                        ui.close();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        actions.push(UiAction::ShowAbout);
                        ui.close();
                    }
                });

                let row_height = ui.spacing().interact_size.y;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_height),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui.button(regular::X).on_hover_text("Hide Window").clicked() {
                            actions.push(UiAction::HideWindow);
                        }

                        let zoom_icon = if state.fullscreen {
                            regular::ARROWS_IN
                        } else {
                            regular::ARROWS_OUT
                        };
                        if ui.button(zoom_icon).on_hover_text("Zoom Window").clicked() {
                            actions.push(UiAction::ZoomWindow);
                        }

                        if ui
                            .button(regular::MINUS)
                            .on_hover_text("Minimize Window")
                            .clicked()
                        {
                            actions.push(UiAction::MinimizeWindow);
                        }

                        let drag_size = egui::vec2(ui.available_width().max(0.0), row_height);
                        if drag_size.x > 0.0 {
                            let (_rect, drag_response) =
                                ui.allocate_exact_size(drag_size, egui::Sense::click_and_drag());
                            let pointer_pressed_on_region = drag_response.hovered()
                                && ui.input(|i| {
                                    i.pointer.button_pressed(egui::PointerButton::Primary)
                                });
                            if pointer_pressed_on_region {
                                actions.push(UiAction::StartWindowDrag);
                            }
                        }
                    },
                );
            });
        });

        egui::TopBottomPanel::bottom("klaw-status-bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let theme_icon = match state.theme_mode {
                    ThemeMode::System => regular::CIRCLE_HALF,
                    ThemeMode::Light => regular::SUN,
                    ThemeMode::Dark => regular::MOON,
                };

                ui.label(theme_icon);
                ui.label("Theme Mode:");
                egui::ComboBox::from_id_salt("status-theme-mode")
                    .width(110.0)
                    .selected_text(state.theme_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
                            if ui
                                .selectable_label(state.theme_mode == mode, mode.label())
                                .clicked()
                            {
                                actions.push(UiAction::SetThemeMode(mode));
                                ui.close();
                            }
                        }
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let version_label = format!("{} v{}", regular::INFO, env!("CARGO_PKG_VERSION"));
                    ui.label(version_label);

                    ui.separator();
                    if self.provider_ids.is_empty() {
                        ui.label("Model Provider: N/A");
                    } else {
                        let default_provider = if self.config_default_provider.is_empty() {
                            "unknown"
                        } else {
                            self.config_default_provider.as_str()
                        };
                        let selected_provider_id = state
                            .runtime_provider_override
                            .as_deref()
                            .unwrap_or(default_provider);
                        let selected_text = selected_provider_id.to_string();

                        egui::ComboBox::from_id_salt("runtime-provider-override")
                            .width(180.0)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for provider_id in &self.provider_ids {
                                    let selected = selected_provider_id == provider_id;
                                    if ui.selectable_label(selected, provider_id).clicked() {
                                        if provider_id == default_provider {
                                            actions
                                                .push(UiAction::SetRuntimeProviderOverride(None));
                                        } else {
                                            actions.push(UiAction::SetRuntimeProviderOverride(
                                                Some(provider_id.clone()),
                                            ));
                                        }
                                        ui.close();
                                    }
                                }
                            });

                        ui.label("Model Provider:");

                        ui.separator();

                        let default_model = self
                            .provider_default_models
                            .get(selected_provider_id)
                            .map(String::as_str)
                            .unwrap_or("N/A");
                        ui.label(format!("Default Model: {default_model}"));
                    }
                });
            });
        });

        egui::SidePanel::left("klaw-sidebar")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                actions.extend(sidebar::show_sidebar(ui, state));
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            actions.extend(workbench::show_workbench(
                ui,
                state,
                &mut self.panels,
                &mut self.notifications,
            ));
        });

        if state.show_about {
            egui::Window::new("About Klaw")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("{} Klaw", regular::INFO));
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Desktop UI shell built with egui.");
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        actions.push(UiAction::HideAbout);
                    }
                });
        }

        self.notifications.show(ctx);

        actions
    }
}

#[derive(Default)]
struct SyncSupervisor {
    last_poll_at: Option<Instant>,
    startup_check_completed: bool,
    startup_check_running: bool,
    task_rx: Option<Receiver<SyncSupervisorMessage>>,
}

enum SyncSupervisorMessage {
    StartupCheckFinished {
        latest_snapshot: Option<SnapshotListItem>,
        local_last_id: Option<String>,
        local_last_at: Option<i64>,
    },
    AutoBackupFinished {
        manifest_id: String,
        created_at: i64,
    },
    Failed {
        kind: SyncRuntimeTaskKind,
        message: String,
    },
}

impl SyncSupervisor {
    fn tick(&mut self, notifications: &mut NotificationCenter) {
        self.poll_task_result(notifications);

        if self
            .last_poll_at
            .is_some_and(|last| last.elapsed() < SYNC_POLL_INTERVAL)
        {
            return;
        }
        self.last_poll_at = Some(Instant::now());

        let settings = load_settings();
        sync_runtime_sync_from_settings(
            settings.sync.last_manifest_id.clone(),
            settings.sync.last_sync_at,
        );
        let now_ms = OffsetDateTime::now_utc().unix_timestamp() * 1000;
        if let Some(kind) = self.next_task(&settings, now_ms) {
            if kind == SyncRuntimeTaskKind::StartupCheck {
                self.startup_check_running = true;
            }
            self.spawn_task(kind, settings);
        }
    }

    fn next_task(&self, settings: &AppSettings, now_ms: i64) -> Option<SyncRuntimeTaskKind> {
        if self.task_in_progress() || !sync_ready(settings) {
            return None;
        }
        if !self.startup_check_completed && !self.startup_check_running {
            return Some(SyncRuntimeTaskKind::StartupCheck);
        }
        if !settings.sync.schedule.auto_backup {
            return None;
        }
        let interval_ms = i64::from(settings.sync.schedule.interval_minutes.max(1)) * 60 * 1000;
        let should_backup = settings
            .sync
            .last_sync_at
            .map(|last| now_ms.saturating_sub(last) >= interval_ms)
            .unwrap_or(true);
        should_backup.then_some(SyncRuntimeTaskKind::AutoBackup)
    }

    fn task_in_progress(&self) -> bool {
        self.task_rx.is_some()
    }

    fn spawn_task(&mut self, kind: SyncRuntimeTaskKind, settings: AppSettings) {
        let label = match kind {
            SyncRuntimeTaskKind::StartupCheck => "Checking remote manifests",
            SyncRuntimeTaskKind::AutoBackup => "Automatic manifest sync",
            SyncRuntimeTaskKind::ManualBackup
            | SyncRuntimeTaskKind::RetentionCleanup
            | SyncRuntimeTaskKind::RefreshRemoteSnapshots
            | SyncRuntimeTaskKind::RestoreSnapshot => return,
        };
        if !sync_runtime_try_start_task(kind, label) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.task_rx = Some(rx);
        thread::spawn(move || {
            let result = match kind {
                SyncRuntimeTaskKind::StartupCheck => run_startup_check_task(&settings),
                SyncRuntimeTaskKind::AutoBackup => run_auto_backup_task(&settings),
                SyncRuntimeTaskKind::ManualBackup
                | SyncRuntimeTaskKind::RetentionCleanup
                | SyncRuntimeTaskKind::RefreshRemoteSnapshots
                | SyncRuntimeTaskKind::RestoreSnapshot => {
                    Err("unsupported sync supervisor task".to_string())
                }
            };
            let message =
                result.unwrap_or_else(|message| SyncSupervisorMessage::Failed { kind, message });
            let _ = tx.send(message);
        });
    }

    fn poll_task_result(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.task_rx.as_ref() else {
            return;
        };
        let message = match rx.try_recv() {
            Ok(message) => message,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.task_rx = None;
                self.startup_check_running = false;
                sync_runtime_finish_task(SyncRuntimeTaskKind::StartupCheck);
                sync_runtime_finish_task(SyncRuntimeTaskKind::AutoBackup);
                return;
            }
        };
        self.task_rx = None;

        match message {
            SyncSupervisorMessage::StartupCheckFinished {
                latest_snapshot,
                local_last_id,
                local_last_at,
            } => {
                self.startup_check_running = false;
                self.startup_check_completed = true;
                sync_runtime_finish_task(SyncRuntimeTaskKind::StartupCheck);
                if let Some(remote) = latest_snapshot {
                    let remote_id = remote.manifest_id.clone();
                    let remote_at = remote.created_at;
                    let remote_is_newer = match local_last_at {
                        Some(local_at) => remote_at > local_at,
                        None => true,
                    };
                    let remote_is_different = local_last_id.as_deref() != Some(remote_id.as_str());
                    if remote_is_newer && remote_is_different {
                        sync_runtime_set_remote_update(Some(remote.clone()));
                        notifications.info(format!(
                            "Remote manifest available: {remote_id}. Open Setting > Sync to restore."
                        ));
                    } else {
                        sync_runtime_set_remote_update(None);
                    }
                } else {
                    sync_runtime_set_remote_update(None);
                }
            }
            SyncSupervisorMessage::AutoBackupFinished {
                manifest_id,
                created_at,
            } => {
                sync_runtime_finish_task(SyncRuntimeTaskKind::AutoBackup);
                let mut settings = load_settings();
                settings.sync.last_manifest_id = Some(manifest_id.clone());
                settings.sync.last_sync_at = Some(created_at);
                let _ = save_settings(&settings);
                sync_runtime_set_last_snapshot(Some(manifest_id.clone()), Some(created_at));
                sync_runtime_set_remote_update(None);
                notifications.success(format!("Automatic manifest sync completed: {manifest_id}."));
            }
            SyncSupervisorMessage::Failed { kind, message } => {
                sync_runtime_finish_task(kind);
                if kind == SyncRuntimeTaskKind::StartupCheck {
                    self.startup_check_running = false;
                    self.startup_check_completed = true;
                }
                notifications.error(message);
            }
        }
    }
}

fn sync_ready(settings: &AppSettings) -> bool {
    settings.sync.enabled && build_sync_store_config(settings).validate().is_ok()
}

fn build_sync_store_config(settings: &AppSettings) -> S3SnapshotStoreConfig {
    S3SnapshotStoreConfig {
        endpoint: settings.sync.s3.endpoint.clone(),
        region: settings.sync.s3.region.clone(),
        bucket: settings.sync.s3.bucket.clone(),
        prefix: settings.sync.s3.prefix.clone(),
        access_key: settings.sync.s3.access_key.clone(),
        secret_key: settings.sync.s3.secret_key.clone(),
        session_token: settings.sync.s3.session_token.clone(),
        access_key_env: settings.sync.s3.access_key_env.clone(),
        secret_key_env: settings.sync.s3.secret_key_env.clone(),
        session_token_env: settings.sync.s3.session_token_env.clone(),
        force_path_style: settings.sync.s3.force_path_style,
    }
}

fn build_backup_plan(settings: &AppSettings) -> BackupPlan {
    BackupPlan {
        mode: match settings.sync.mode {
            SyncMode::ManifestVersioned => SnapshotMode::ManifestVersioned,
        },
        items: settings
            .sync
            .backup_items
            .iter()
            .copied()
            .filter_map(|item| match item {
                crate::settings::SyncItem::Session => Some(BackupItem::Session),
                crate::settings::SyncItem::Skills => Some(BackupItem::Skills),
                crate::settings::SyncItem::Mcp => None,
                crate::settings::SyncItem::SkillsRegistry => Some(BackupItem::SkillsRegistry),
                crate::settings::SyncItem::GuiSettings => Some(BackupItem::GuiSettings),
                crate::settings::SyncItem::Archive => Some(BackupItem::Archive),
                crate::settings::SyncItem::UserWorkspace => Some(BackupItem::UserWorkspace),
                crate::settings::SyncItem::Memory => Some(BackupItem::Memory),
                crate::settings::SyncItem::Config => Some(BackupItem::Config),
            })
            .collect(),
    }
}

fn run_startup_check_task(settings: &AppSettings) -> Result<SyncSupervisorMessage, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| err.to_string())?;
    let config = build_sync_store_config(settings);
    let device_id = settings.sync.device_id.clone();
    let local_last_id = settings.sync.last_manifest_id.clone();
    let local_last_at = settings.sync.last_sync_at;
    runtime.block_on(async move {
        let service = BackupService::open_s3_default(config, device_id)
            .await
            .map_err(|err| err.to_string())?;
        let latest_snapshot = service
            .latest_remote_snapshot()
            .await
            .map_err(|err| err.to_string())?;
        Ok(SyncSupervisorMessage::StartupCheckFinished {
            latest_snapshot,
            local_last_id,
            local_last_at,
        })
    })
}

fn run_auto_backup_task(settings: &AppSettings) -> Result<SyncSupervisorMessage, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| err.to_string())?;
    let config = build_sync_store_config(settings);
    let device_id = settings.sync.device_id.clone();
    let keep_last = settings.sync.retention.keep_last;
    let plan = build_backup_plan(settings);
    runtime.block_on(async move {
        let service = BackupService::open_s3_default(config, device_id)
            .await
            .map_err(|err| err.to_string())?;
        let result = service
            .create_upload_and_cleanup_snapshot(&plan, keep_last)
            .await
            .map_err(|err| err.to_string())?;
        let snapshots = service
            .list_remote_snapshots()
            .await
            .map_err(|err| err.to_string())?;
        sync_runtime_set_remote_snapshots(snapshots);
        Ok(SyncSupervisorMessage::AutoBackupFinished {
            manifest_id: result.manifest_id,
            created_at: result.manifest.created_at,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_settings(auto_backup: bool, last_sync_at: Option<i64>) -> AppSettings {
        let mut settings = AppSettings::default();
        settings.sync.enabled = true;
        settings.sync.schedule.auto_backup = auto_backup;
        settings.sync.last_sync_at = last_sync_at;
        settings.sync.s3.bucket = "demo".to_string();
        settings.sync.s3.access_key = "ak".to_string();
        settings.sync.s3.secret_key = "sk".to_string();
        settings
    }

    #[test]
    fn next_task_runs_startup_check_before_other_work() {
        let supervisor = SyncSupervisor::default();

        assert_eq!(
            supervisor.next_task(&ready_settings(false, None), 0),
            Some(SyncRuntimeTaskKind::StartupCheck)
        );
    }

    #[test]
    fn next_task_skips_startup_maintenance_when_auto_backup_is_disabled() {
        let mut supervisor = SyncSupervisor::default();
        supervisor.startup_check_completed = true;

        assert_eq!(supervisor.next_task(&ready_settings(false, None), 0), None);
    }

    #[test]
    fn next_task_runs_auto_backup_after_interval_elapses() {
        let mut supervisor = SyncSupervisor::default();
        supervisor.startup_check_completed = true;
        let mut settings = ready_settings(true, Some(1_000));
        settings.sync.schedule.interval_minutes = 1;

        assert_eq!(
            supervisor.next_task(&settings, 62_000),
            Some(SyncRuntimeTaskKind::AutoBackup)
        );
    }
}
