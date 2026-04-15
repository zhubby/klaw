use crate::autostart;
use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::{
    AppSettings, ProxyMode, S3SyncConfig, SyncItem, SyncMode, SyncProvider, load_settings,
    save_settings,
};
use crate::state::persistence;
use crate::state::{DarkThemePreset, LightThemePreset, UiState};
use crate::sync_runtime::{
    SyncRuntimeProgress, SyncRuntimeSnapshot, SyncRuntimeTaskKind, sync_runtime_finish_task,
    sync_runtime_set_last_snapshot, sync_runtime_set_remote_snapshots,
    sync_runtime_set_remote_update, sync_runtime_set_task_progress, sync_runtime_snapshot,
    sync_runtime_sync_from_settings, sync_runtime_try_start_task,
};
use crate::theme;
use crate::time_format::format_optional_timestamp_millis;
use egui_extras::{Size, StripBuilder};
use klaw_storage::{
    BackupItem, BackupPlan, BackupProgress, BackupService, S3SnapshotStoreConfig, SnapshotListItem,
    SnapshotMode,
};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use tokio::runtime::Builder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    General,
    Privacy,
    Security,
    Network,
    Sync,
}

impl SettingsSection {
    fn title(&self) -> &'static str {
        match self {
            SettingsSection::General => "General",
            SettingsSection::Privacy => "Privacy",
            SettingsSection::Security => "Security",
            SettingsSection::Network => "Network",
            SettingsSection::Sync => "Sync",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            SettingsSection::General => "\u{2699}",
            SettingsSection::Privacy => "\u{1F512}",
            SettingsSection::Security => "\u{1F6E1}",
            SettingsSection::Network => "\u{1F310}",
            SettingsSection::Sync => "\u{1F504}",
        }
    }
}

enum SyncTaskMessage {
    BackupDone {
        manifest_id: String,
        created_at: i64,
    },
    ListDone {
        snapshots: Vec<SnapshotListItem>,
    },
    RestoreDone {
        manifest_id: String,
    },
    CleanupDone,
    Failed(String),
}

pub struct SettingPanel {
    settings: AppSettings,
    theme_state: UiState,
    active_section: SettingsSection,
    save_error: Option<String>,
    sync_task_rx: Option<Receiver<SyncTaskMessage>>,
    sync_task_kind: Option<SyncRuntimeTaskKind>,
    pending_restore_manifest_id: Option<String>,
}

impl Default for SettingPanel {
    fn default() -> Self {
        let settings = load_settings();
        Self {
            settings,
            theme_state: persistence::load_ui_state(),
            active_section: SettingsSection::General,
            save_error: None,
            sync_task_rx: None,
            sync_task_kind: None,
            pending_restore_manifest_id: None,
        }
    }
}

impl PanelRenderer for SettingPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.poll_sync_tasks(notifications);
        let runtime = self.refresh_settings_from_runtime();
        const MIN_CONTENT_HEIGHT: f32 = 320.0;

        let mut render_body = |ui: &mut egui::Ui, this: &mut SettingPanel| {
            ui.heading(ctx.tab_title);
            ui.label("Configure application preferences");
            ui.separator();

            if let Some(err) = &this.save_error {
                ui.colored_label(
                    ui.style().visuals.error_fg_color,
                    format!("Save error: {err}"),
                );
            }

            StripBuilder::new(ui)
                .size(Size::remainder().at_least(MIN_CONTENT_HEIGHT))
                .vertical(|mut strip| {
                    strip.cell(|ui| {
                        StripBuilder::new(ui)
                            .size(Size::exact(160.0))
                            .size(Size::exact(12.0))
                            .size(Size::remainder().at_least(420.0))
                            .horizontal(|mut strip| {
                                strip.cell(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_min_width(140.0);
                                        ui.set_max_width(160.0);
                                        for section in [
                                            SettingsSection::General,
                                            SettingsSection::Privacy,
                                            SettingsSection::Security,
                                            SettingsSection::Network,
                                            SettingsSection::Sync,
                                        ] {
                                            let is_active = this.active_section == section;
                                            let text =
                                                format!("{} {}", section.icon(), section.title());
                                            if ui.selectable_label(is_active, text).clicked() {
                                                this.active_section = section;
                                            }
                                        }
                                    });
                                });
                                strip.cell(|ui| {
                                    ui.add(egui::Separator::default().vertical());
                                });
                                strip.cell(|ui| {
                                    egui::ScrollArea::vertical()
                                        .id_salt("settings-section-scroll")
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| match this.active_section {
                                            SettingsSection::General => {
                                                this.render_general_section(ui, notifications)
                                            }
                                            SettingsSection::Privacy => {
                                                this.render_privacy_section(ui, notifications)
                                            }
                                            SettingsSection::Security => {
                                                this.render_security_section(ui)
                                            }
                                            SettingsSection::Network => {
                                                this.render_network_section(ui)
                                            }
                                            SettingsSection::Sync => this.render_sync_section(
                                                ui,
                                                notifications,
                                                &runtime,
                                            ),
                                        });
                                });
                            });
                    });
                });
        };

        let parent_height = ui.available_height();
        if parent_height < MIN_CONTENT_HEIGHT {
            egui::ScrollArea::vertical()
                .id_salt("settings-panel-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_height(MIN_CONTENT_HEIGHT);
                    render_body(ui, self);
                });
        } else {
            render_body(ui, self);
        }
    }
}

impl SettingPanel {
    fn sync_theme_state(&mut self) {
        let persisted = persistence::load_ui_state();
        self.theme_state.theme_mode = persisted.theme_mode;
        self.theme_state.light_theme = persisted.light_theme;
        self.theme_state.dark_theme = persisted.dark_theme;
    }

    fn save_theme_state(&mut self, ctx: &egui::Context) {
        match persistence::update_ui_state(|state| {
            state.light_theme = self.theme_state.light_theme;
            state.dark_theme = self.theme_state.dark_theme;
        }) {
            Ok(state) => {
                self.theme_state = state;
                theme::apply_theme(ctx, &self.theme_state);
                self.save_error = None;
            }
            Err(err) => {
                self.save_error = Some(err.to_string());
            }
        }
    }

    fn try_save(&mut self) -> bool {
        match save_settings(&self.settings) {
            Ok(()) => {
                self.save_error = None;
                sync_runtime_sync_from_settings(
                    self.settings.sync.last_manifest_id.clone(),
                    self.settings.sync.last_sync_at,
                );
                true
            }
            Err(err) => {
                self.save_error = Some(err.to_string());
                false
            }
        }
    }

    fn persist_launch_at_startup_change(
        &mut self,
        previous: bool,
        notifications: &mut NotificationCenter,
    ) {
        let desired = self.settings.general.launch_at_startup;
        if desired == previous {
            return;
        }

        if let Err(err) = autostart::apply(desired) {
            self.settings.general.launch_at_startup = previous;
            self.save_error = Some(err.to_string());
            notifications.error(format!("Failed to update launch at startup: {err}"));
            return;
        }

        if self.try_save() {
            let message = if desired {
                "Launch at startup enabled."
            } else {
                "Launch at startup disabled."
            };
            notifications.success(message);
            return;
        }

        let save_error = self
            .save_error
            .clone()
            .unwrap_or_else(|| "unknown settings save failure".to_string());
        self.settings.general.launch_at_startup = previous;

        match autostart::apply(previous) {
            Ok(()) => {
                notifications.error(format!(
                    "Failed to save launch at startup setting: {save_error}"
                ));
            }
            Err(rollback_err) => {
                let message = format!(
                    "{save_error}; also failed to restore the previous macOS login item state: {rollback_err}"
                );
                self.save_error = Some(message.clone());
                notifications.error(format!(
                    "Failed to save launch at startup setting and rollback macOS login item: {message}"
                ));
            }
        }
    }

    fn poll_sync_tasks(&mut self, notifications: &mut NotificationCenter) {
        let mut clear_task = false;
        while let Some(rx) = self.sync_task_rx.as_ref() {
            match rx.try_recv() {
                Ok(SyncTaskMessage::BackupDone {
                    manifest_id,
                    created_at,
                }) => {
                    self.settings.sync.last_manifest_id = Some(manifest_id.clone());
                    self.settings.sync.last_sync_at = Some(created_at);
                    sync_runtime_set_last_snapshot(Some(manifest_id.clone()), Some(created_at));
                    let _ = self.try_save();
                    notifications.success(format!("Manifest {manifest_id} uploaded to S3."));
                    clear_task = true;
                }
                Ok(SyncTaskMessage::ListDone { snapshots }) => {
                    sync_runtime_set_remote_snapshots(snapshots);
                    notifications.success("Remote manifests refreshed.");
                    clear_task = true;
                }
                Ok(SyncTaskMessage::RestoreDone { manifest_id }) => {
                    notifications.warning(format!(
                        "Manifest {manifest_id} restored. Restart Klaw before continuing."
                    ));
                    clear_task = true;
                }
                Ok(SyncTaskMessage::CleanupDone) => {
                    notifications.success("Remote manifest retention cleanup completed.");
                    clear_task = true;
                }
                Ok(SyncTaskMessage::Failed(err)) => {
                    notifications.error(err);
                    clear_task = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    clear_task = true;
                    break;
                }
            }
        }
        if clear_task {
            if let Some(kind) = self.sync_task_kind.take() {
                sync_runtime_finish_task(kind);
            }
            self.sync_task_rx = None;
        }
    }

    fn sync_busy(&self) -> bool {
        sync_runtime_snapshot().active_task.is_some()
    }

    fn sync_config(&self) -> S3SnapshotStoreConfig {
        let S3SyncConfig {
            endpoint,
            region,
            bucket,
            prefix,
            access_key,
            secret_key,
            session_token,
            access_key_env,
            secret_key_env,
            session_token_env,
            force_path_style,
        } = &self.settings.sync.s3;
        S3SnapshotStoreConfig {
            endpoint: endpoint.clone(),
            region: region.clone(),
            bucket: bucket.clone(),
            prefix: prefix.clone(),
            access_key: access_key.clone(),
            secret_key: secret_key.clone(),
            session_token: session_token.clone(),
            access_key_env: access_key_env.clone(),
            secret_key_env: secret_key_env.clone(),
            session_token_env: session_token_env.clone(),
            force_path_style: *force_path_style,
        }
    }

    fn sync_validation_error(&self) -> Option<String> {
        self.sync_config()
            .validate()
            .err()
            .map(|err| err.to_string())
    }

    fn backup_plan(&self) -> BackupPlan {
        BackupPlan {
            mode: match self.settings.sync.mode {
                SyncMode::ManifestVersioned => SnapshotMode::ManifestVersioned,
            },
            items: self
                .settings
                .sync
                .backup_items
                .iter()
                .copied()
                .filter_map(sync_item_to_backup_item)
                .collect(),
        }
    }

    fn spawn_sync_task<F>(&mut self, kind: SyncRuntimeTaskKind, label: &'static str, task: F)
    where
        F: FnOnce() -> Result<SyncTaskMessage, String> + Send + 'static,
    {
        if !sync_runtime_try_start_task(kind, label) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.sync_task_rx = Some(rx);
        self.sync_task_kind = Some(kind);
        thread::spawn(move || {
            let message = task().unwrap_or_else(SyncTaskMessage::Failed);
            let _ = tx.send(message);
        });
    }

    fn run_backup(&mut self) {
        let config = self.sync_config();
        let plan = self.backup_plan();
        let device_id = self.settings.sync.device_id.clone();
        let keep_last = self.settings.sync.retention.keep_last;
        self.spawn_sync_task(
            SyncRuntimeTaskKind::ManualBackup,
            "Uploading manifest sync",
            move || {
                let runtime = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;
                runtime.block_on(async move {
                    sync_runtime_set_task_progress(
                        SyncRuntimeTaskKind::ManualBackup,
                        Some(SyncRuntimeProgress {
                            fraction: 0.02,
                            stage: "Connecting to remote storage".to_string(),
                            detail: Some("Validating sync configuration".to_string()),
                        }),
                    );
                    let service = BackupService::open_s3_default(config, device_id)
                        .await
                        .map_err(|err| err.to_string())?;
                    let mut report = |progress: BackupProgress| {
                        sync_runtime_set_task_progress(
                            SyncRuntimeTaskKind::ManualBackup,
                            Some(runtime_progress_from_backup(progress)),
                        );
                    };
                    let result = service
                        .create_upload_and_cleanup_snapshot_with_progress(
                            &plan,
                            keep_last,
                            &mut report,
                        )
                        .await
                        .map_err(|err| err.to_string())?;
                    let snapshots = service
                        .list_remote_snapshots()
                        .await
                        .map_err(|err| err.to_string())?;
                    sync_runtime_set_remote_snapshots(snapshots);
                    sync_runtime_set_remote_update(None);
                    Ok(SyncTaskMessage::BackupDone {
                        manifest_id: result.manifest_id,
                        created_at: result.manifest.created_at,
                    })
                })
            },
        );
    }

    fn refresh_remote_snapshots(&mut self) {
        let config = self.sync_config();
        let device_id = self.settings.sync.device_id.clone();
        self.spawn_sync_task(
            SyncRuntimeTaskKind::RefreshRemoteSnapshots,
            "Loading manifests",
            move || {
                let runtime = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;
                runtime.block_on(async move {
                    let service = BackupService::open_s3_default(config, device_id)
                        .await
                        .map_err(|err| err.to_string())?;
                    let snapshots = service
                        .list_remote_snapshots()
                        .await
                        .map_err(|err| err.to_string())?;
                    sync_runtime_set_remote_update(None);
                    Ok(SyncTaskMessage::ListDone { snapshots })
                })
            },
        );
    }

    fn restore_snapshot(&mut self, manifest_id: String) {
        let config = self.sync_config();
        let device_id = self.settings.sync.device_id.clone();
        self.spawn_sync_task(
            SyncRuntimeTaskKind::RestoreSnapshot,
            "Restoring manifest",
            move || {
                let runtime = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;
                runtime.block_on(async move {
                    let service = BackupService::open_s3_default(config, device_id)
                        .await
                        .map_err(|err| err.to_string())?;
                    service
                        .restore_snapshot(&manifest_id)
                        .await
                        .map_err(|err| err.to_string())?;
                    Ok(SyncTaskMessage::RestoreDone { manifest_id })
                })
            },
        );
    }

    fn run_retention_cleanup(&mut self) {
        let config = self.sync_config();
        let device_id = self.settings.sync.device_id.clone();
        let keep_last = self.settings.sync.retention.keep_last;
        self.spawn_sync_task(
            SyncRuntimeTaskKind::RetentionCleanup,
            "Cleaning up remote manifests",
            move || {
                let runtime = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;
                runtime.block_on(async move {
                    let service = BackupService::open_s3_default(config, device_id)
                        .await
                        .map_err(|err| err.to_string())?;
                    service
                        .cleanup_remote_snapshots(keep_last)
                        .await
                        .map_err(|err| err.to_string())?;
                    let snapshots = service
                        .list_remote_snapshots()
                        .await
                        .map_err(|err| err.to_string())?;
                    sync_runtime_set_remote_snapshots(snapshots);
                    sync_runtime_set_remote_update(None);
                    Ok(SyncTaskMessage::CleanupDone)
                })
            },
        );
    }

    fn refresh_settings_from_runtime(&mut self) -> SyncRuntimeSnapshot {
        let runtime = sync_runtime_snapshot();
        if let Some(manifest_id) = runtime.last_manifest_id.clone() {
            self.settings.sync.last_manifest_id = Some(manifest_id);
        }
        if runtime.last_sync_at.is_some() {
            self.settings.sync.last_sync_at = runtime.last_sync_at;
        }
        runtime
    }

    fn render_general_section(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        self.sync_theme_state();
        ui.strong("General Settings");
        ui.add_space(8.0);

        let previous_launch_setting = self.settings.general.launch_at_startup;
        let enable_unavailable_reason = autostart::enable_availability()
            .unsupported_reason()
            .map(str::to_owned);
        let mut startup_setting_changed = false;
        ui.horizontal(|ui| {
            ui.label("Launch at startup:");
            ui.add_enabled_ui(enable_unavailable_reason.is_none(), |ui| {
                startup_setting_changed = ui
                    .radio_value(&mut self.settings.general.launch_at_startup, true, "Yes")
                    .changed();
            });
            startup_setting_changed |= ui
                .radio_value(&mut self.settings.general.launch_at_startup, false, "No")
                .changed();
        });
        if startup_setting_changed {
            self.persist_launch_at_startup_change(previous_launch_setting, notifications);
        }

        ui.add_space(8.0);
        ui.label("Automatically start Klaw when you log in to your computer.");
        if let Some(reason) = enable_unavailable_reason {
            ui.add_space(4.0);
            ui.label(format!(
                "{reason} You can still turn the setting off from here."
            ));
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);
        ui.label(format!(
            "Current theme mode: {} (change from the bottom status bar).",
            self.theme_state.theme_mode.label()
        ));
        ui.add_space(8.0);

        let mut theme_changed = false;
        egui::Grid::new("general-theme-grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                ui.label("Light Theme:");
                egui::ComboBox::from_id_salt("settings-light-theme")
                    .width(160.0)
                    .selected_text(self.theme_state.light_theme.label())
                    .show_ui(ui, |ui| {
                        for preset in [
                            LightThemePreset::Default,
                            LightThemePreset::Latte,
                            LightThemePreset::Crab,
                        ] {
                            if ui
                                .selectable_label(
                                    self.theme_state.light_theme == preset,
                                    preset.label(),
                                )
                                .clicked()
                            {
                                self.theme_state.light_theme = preset;
                                theme_changed = true;
                                ui.close();
                            }
                        }
                    });
                ui.end_row();

                ui.label("Dark Theme:");
                egui::ComboBox::from_id_salt("settings-dark-theme")
                    .width(160.0)
                    .selected_text(self.theme_state.dark_theme.label())
                    .show_ui(ui, |ui| {
                        for preset in [
                            DarkThemePreset::Default,
                            DarkThemePreset::Frappe,
                            DarkThemePreset::Macchiato,
                            DarkThemePreset::Mocha,
                            DarkThemePreset::Blackpink,
                        ] {
                            if ui
                                .selectable_label(
                                    self.theme_state.dark_theme == preset,
                                    preset.label(),
                                )
                                .clicked()
                            {
                                self.theme_state.dark_theme = preset;
                                theme_changed = true;
                                ui.close();
                            }
                        }
                    });
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.small("Default keeps the existing egui light/dark visuals.");

        if theme_changed {
            self.save_theme_state(ui.ctx());
        }
    }

    fn render_privacy_section(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        ui.strong("Privacy Settings");
        ui.add_space(8.0);

        let location_status = current_location_status();
        ui.group(|ui| {
            ui.strong("Location Services");
            ui.add_space(6.0);
            ui.label(format!(
                "System location services: {}",
                bool_status_label(location_status.services_enabled)
            ));
            ui.label(format!(
                "App authorization: {}",
                location_status.authorization_label()
            ));
            if let Some(detail) = location_status.detail_message() {
                ui.add_space(4.0);
                ui.small(detail);
            }
            ui.add_space(8.0);
            if ui.button("Open Location Settings").clicked() {
                match open_location_settings() {
                    Ok(()) => notifications.info("Opened macOS Location Services settings."),
                    Err(err) => notifications.error(format!(
                        "Failed to open macOS Location Services settings: {err}"
                    )),
                }
            }
        });
    }

    fn render_security_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Security Settings");
        ui.add_space(8.0);
        ui.label("Security settings are not yet configured.");
        ui.add_space(8.0);
        ui.label("Future options may include:");
        ui.label("\u{2022} API key encryption");
        ui.label("\u{2022} Session timeout");
        ui.label("\u{2022} Two-factor authentication");
    }

    fn render_network_section(&mut self, ui: &mut egui::Ui) {
        ui.strong("Network Settings");
        ui.add_space(8.0);

        ui.label("Proxy Configuration:");
        ui.add_space(4.0);

        if ui
            .radio_value(
                &mut self.settings.network.proxy_mode,
                ProxyMode::NoProxy,
                "No proxy",
            )
            .changed()
            || ui
                .radio_value(
                    &mut self.settings.network.proxy_mode,
                    ProxyMode::SystemProxy,
                    "Use system proxy",
                )
                .changed()
            || ui
                .radio_value(
                    &mut self.settings.network.proxy_mode,
                    ProxyMode::ManualProxy,
                    "Manual proxy configuration",
                )
                .changed()
        {
            self.try_save();
        }

        if self.settings.network.proxy_mode == ProxyMode::ManualProxy {
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            ui.group(|ui| {
                ui.strong("HTTP Proxy");
                if render_proxy_fields(ui, &mut self.settings.network.http_proxy) {
                    self.try_save();
                }
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.strong("HTTPS Proxy");
                if render_proxy_fields(ui, &mut self.settings.network.https_proxy) {
                    self.try_save();
                }
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.strong("SOCKS5 Proxy");
                if render_proxy_fields(ui, &mut self.settings.network.socks5_proxy) {
                    self.try_save();
                }
            });
        }
    }

    fn render_sync_section(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
        runtime: &SyncRuntimeSnapshot,
    ) {
        ui.strong("Sync Settings");
        ui.add_space(8.0);
        let sync_validation_error = self.sync_validation_error();

        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut self.settings.sync.enabled,
                "Enable manifest sync and S3 storage",
            )
            .changed();

        ui.add_space(8.0);
        egui::CollapsingHeader::new("General")
            .id_salt("sync-general")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("sync-general-grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Provider:");
                        changed |= ui
                            .radio_value(&mut self.settings.sync.provider, SyncProvider::S3, "S3")
                            .changed();
                        ui.end_row();

                        ui.label("Mode:");
                        changed |= ui
                            .radio_value(
                                &mut self.settings.sync.mode,
                                SyncMode::ManifestVersioned,
                                "Versioned manifest",
                            )
                            .changed();
                        ui.end_row();

                        ui.label("Device ID:");
                        changed |= ui
                            .text_edit_singleline(&mut self.settings.sync.device_id)
                            .changed();
                        ui.end_row();
                    });
            });

        egui::CollapsingHeader::new("Schedule And Retention")
            .id_salt("sync-schedule")
            .default_open(true)
            .show(ui, |ui| {
                changed |= ui
                    .checkbox(
                        &mut self.settings.sync.schedule.auto_backup,
                        "Enable automatic backup",
                    )
                    .changed();
                egui::Grid::new("sync-schedule-grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Interval (minutes):");
                        let mut interval = self.settings.sync.schedule.interval_minutes.to_string();
                        if ui.text_edit_singleline(&mut interval).changed() {
                            if let Ok(parsed) = interval.parse::<u32>() {
                                self.settings.sync.schedule.interval_minutes = parsed.max(1);
                                changed = true;
                            }
                        }
                        ui.end_row();

                        ui.label("Keep latest manifests:");
                        let mut keep_last = self.settings.sync.retention.keep_last.to_string();
                        if ui.text_edit_singleline(&mut keep_last).changed() {
                            if let Ok(parsed) = keep_last.parse::<u32>() {
                                self.settings.sync.retention.keep_last = parsed.max(1);
                                changed = true;
                            }
                        }
                        ui.end_row();
                    });
            });

        egui::CollapsingHeader::new("S3 Configuration")
            .id_salt("sync-s3")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("sync-s3-grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        changed |= render_sync_text_field(
                            ui,
                            "Endpoint:",
                            &mut self.settings.sync.s3.endpoint,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Region:",
                            &mut self.settings.sync.s3.region,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Bucket:",
                            &mut self.settings.sync.s3.bucket,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Prefix:",
                            &mut self.settings.sync.s3.prefix,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Access Key:",
                            &mut self.settings.sync.s3.access_key,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Secret Key:",
                            &mut self.settings.sync.s3.secret_key,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Session Token:",
                            &mut self.settings.sync.s3.session_token,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Access Key Env:",
                            &mut self.settings.sync.s3.access_key_env,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Secret Key Env:",
                            &mut self.settings.sync.s3.secret_key_env,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            "Session Token Env:",
                            &mut self.settings.sync.s3.session_token_env,
                        );
                        ui.end_row();
                    });
                changed |= ui
                    .checkbox(
                        &mut self.settings.sync.s3.force_path_style,
                        "Force path style",
                    )
                    .changed();
            });

        egui::CollapsingHeader::new("Backup Scope")
            .id_salt("sync-scope")
            .default_open(true)
            .show(ui, |ui| {
                for item in SyncItem::all() {
                    let index = self
                        .settings
                        .sync
                        .backup_items
                        .iter()
                        .position(|value| value == item);
                    let mut checked = index.is_some();
                    if ui.checkbox(&mut checked, item.label()).clicked() {
                        if checked && index.is_none() {
                            self.settings.sync.backup_items.push(*item);
                            changed = true;
                        } else if !checked {
                            if let Some(idx) = index {
                                self.settings.sync.backup_items.remove(idx);
                                changed = true;
                            }
                        }
                    }
                }
                ui.add_space(4.0);
                ui.label(
                    "Restore replays a selected manifest version. Temporary, logs, and observability data are excluded.",
                );
            });

        egui::CollapsingHeader::new("Manifest Actions")
            .id_salt("sync-actions")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(remote_update) = &runtime.remote_update {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!(
                            "Remote manifest {} from {} is newer than local.",
                            remote_update.manifest_id, remote_update.device_id
                        ),
                    );
                    ui.label(format!(
                        "Remote created: {}",
                        crate::time_format::format_timestamp_millis(remote_update.created_at)
                    ));
                    ui.add_space(6.0);
                }
                ui.label(format!(
                    "Last sync: {}",
                    format_optional_timestamp_millis(self.settings.sync.last_sync_at)
                ));
                ui.label(format!(
                    "Last manifest ID: {}",
                    self.settings
                        .sync
                        .last_manifest_id
                        .clone()
                        .unwrap_or_default()
                ));
                if let Some(task) = &runtime.active_task {
                    ui.label(format!("In progress: {}", task.label));
                    if let Some(progress) = &task.progress {
                        ui.add(
                            egui::ProgressBar::new(progress.fraction.clamp(0.0, 1.0))
                                .desired_width(ui.available_width().max(200.0))
                                .show_percentage()
                                .text(progress.stage.clone()),
                        );
                        if let Some(detail) = &progress.detail {
                            ui.small(detail);
                        }
                    }
                }
                if let Some(err) = &sync_validation_error {
                    ui.colored_label(ui.visuals().warn_fg_color, err);
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    let can_run = self.settings.sync.enabled
                        && !self.sync_busy()
                        && sync_validation_error.is_none();
                    if ui
                        .add_enabled(can_run, egui::Button::new("Run Sync Now"))
                        .clicked()
                    {
                        self.run_backup();
                    }
                    if ui
                        .add_enabled(
                            !self.sync_busy() && sync_validation_error.is_none(),
                            egui::Button::new("Refresh Remote Manifests"),
                        )
                        .clicked()
                    {
                        self.refresh_remote_snapshots();
                    }
                    if ui
                        .add_enabled(
                            self.settings.sync.enabled
                                && !self.sync_busy()
                                && sync_validation_error.is_none(),
                            egui::Button::new("Run Retention Cleanup"),
                        )
                        .clicked()
                    {
                        self.run_retention_cleanup();
                    }
                });
                if self.settings.sync.enabled && sync_validation_error.is_none() {
                    ui.small(
                        "Manual sync progress is shown below while reconciliation, blob upload, and manifest publish are running.",
                    );
                }
            });

        egui::CollapsingHeader::new("Remote Manifests")
            .id_salt("sync-remote")
            .default_open(true)
            .show(ui, |ui| {
                if runtime.remote_snapshots.is_empty() {
                    ui.label("No remote manifests loaded.");
                } else {
                    let mut restore_target = None;
                    for snapshot in &runtime.remote_snapshots {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(format!("Manifest: {}", snapshot.manifest_id));
                                ui.label(format!(
                                    "Created: {}",
                                    crate::time_format::format_timestamp_millis(
                                        snapshot.created_at
                                    )
                                ));
                                ui.label(format!("Device: {}", snapshot.device_id));
                            });
                            if ui
                                .add_enabled(
                                    !self.sync_busy() && sync_validation_error.is_none(),
                                    egui::Button::new("Restore"),
                                )
                                .clicked()
                            {
                                restore_target = Some(snapshot.manifest_id.clone());
                            }
                        });
                    }
                    if let Some(manifest_id) = restore_target {
                        self.pending_restore_manifest_id = Some(manifest_id);
                    }
                }
            });

        if changed {
            self.try_save();
        }

        if let Some(manifest_id) = self.pending_restore_manifest_id.clone() {
            let mut keep_open = true;
            egui::Window::new("Confirm Restore")
                .collapsible(false)
                .resizable(false)
                .open(&mut keep_open)
                .show(ui.ctx(), |ui| {
                    ui.label("Restore replaces the current local manifest-managed data.");
                    ui.label("Restore replays the selected manifest version.");
                    ui.label("Restart Klaw after restore completes.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.pending_restore_manifest_id = None;
                        }
                        if ui
                            .add_enabled(!self.sync_busy(), egui::Button::new("Restore Now"))
                            .clicked()
                        {
                            self.pending_restore_manifest_id = None;
                            self.restore_snapshot(manifest_id.clone());
                            notifications.info("Restore started.");
                        }
                    });
                });
            if !keep_open {
                self.pending_restore_manifest_id = None;
            }
        }
    }
}

fn bool_status_label(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocationAuthorizationState {
    NotDetermined,
    Restricted,
    Denied,
    AuthorizedAlways,
    AuthorizedWhenInUse,
    #[cfg(not(target_os = "macos"))]
    UnsupportedPlatform,
    Unknown(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocationStatus {
    services_enabled: bool,
    authorization: LocationAuthorizationState,
}

impl LocationStatus {
    fn authorization_label(self) -> &'static str {
        match self.authorization {
            LocationAuthorizationState::NotDetermined => "not determined",
            LocationAuthorizationState::Restricted => "restricted",
            LocationAuthorizationState::Denied => "denied",
            LocationAuthorizationState::AuthorizedAlways => "authorized always",
            LocationAuthorizationState::AuthorizedWhenInUse => "authorized when in use",
            #[cfg(not(target_os = "macos"))]
            LocationAuthorizationState::UnsupportedPlatform => "unsupported on this platform",
            LocationAuthorizationState::Unknown(_) => "unknown",
        }
    }

    fn detail_message(self) -> Option<&'static str> {
        match self.authorization {
            LocationAuthorizationState::NotDetermined => Some(
                "Authorization has not been granted yet. Open system settings to review Location Services access.",
            ),
            LocationAuthorizationState::Restricted => Some(
                "Location access is restricted by system policy or parental controls.",
            ),
            LocationAuthorizationState::Denied => Some(
                "Location access is currently denied for this app context. Open system settings to allow it.",
            ),
            LocationAuthorizationState::AuthorizedAlways
            | LocationAuthorizationState::AuthorizedWhenInUse => {
                (!self.services_enabled).then_some(
                    "Authorization exists, but system-wide Location Services are currently disabled.",
                )
            }
            #[cfg(not(target_os = "macos"))]
            LocationAuthorizationState::UnsupportedPlatform => Some(
                "Location Services privacy integration is currently implemented for macOS only.",
            ),
            LocationAuthorizationState::Unknown(_) => Some(
                "The system returned an unknown authorization state.",
            ),
        }
    }
}

#[cfg(target_os = "macos")]
fn current_location_status() -> LocationStatus {
    use objc2_core_location::{CLAuthorizationStatus, CLLocationManager};

    let services_enabled = unsafe { CLLocationManager::locationServicesEnabled_class() };
    let status = unsafe { CLLocationManager::new().authorizationStatus() };
    let authorization = if status == CLAuthorizationStatus::kCLAuthorizationStatusNotDetermined {
        LocationAuthorizationState::NotDetermined
    } else if status == CLAuthorizationStatus::kCLAuthorizationStatusRestricted {
        LocationAuthorizationState::Restricted
    } else if status == CLAuthorizationStatus::kCLAuthorizationStatusDenied {
        LocationAuthorizationState::Denied
    } else if status == CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedAlways {
        LocationAuthorizationState::AuthorizedAlways
    } else if status == CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedWhenInUse {
        LocationAuthorizationState::AuthorizedWhenInUse
    } else {
        LocationAuthorizationState::Unknown(status.0)
    };

    LocationStatus {
        services_enabled,
        authorization,
    }
}

#[cfg(not(target_os = "macos"))]
fn current_location_status() -> LocationStatus {
    LocationStatus {
        services_enabled: false,
        authorization: LocationAuthorizationState::UnsupportedPlatform,
    }
}

#[cfg(target_os = "macos")]
fn open_location_settings() -> std::io::Result<()> {
    Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices")
        .spawn()?
        .wait()?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn open_location_settings() -> std::io::Result<()> {
    Err(std::io::Error::other(
        "opening location settings is only supported on macOS",
    ))
}

fn sync_item_to_backup_item(item: SyncItem) -> Option<BackupItem> {
    match item {
        SyncItem::Session => Some(BackupItem::Session),
        SyncItem::Skills => Some(BackupItem::Skills),
        SyncItem::Mcp => None,
        SyncItem::SkillsRegistry => Some(BackupItem::SkillsRegistry),
        SyncItem::GuiSettings => Some(BackupItem::GuiSettings),
        SyncItem::Archive => Some(BackupItem::Archive),
        SyncItem::UserWorkspace => Some(BackupItem::UserWorkspace),
        SyncItem::Memory => Some(BackupItem::Memory),
        SyncItem::Config => Some(BackupItem::Config),
    }
}

fn runtime_progress_from_backup(progress: BackupProgress) -> SyncRuntimeProgress {
    SyncRuntimeProgress {
        fraction: progress.fraction.clamp(0.0, 1.0),
        stage: match progress.stage {
            klaw_storage::BackupProgressStage::ReconcilingRemote => "Reconciling remote manifest",
            klaw_storage::BackupProgressStage::PreparingManifest => "Preparing manifest",
            klaw_storage::BackupProgressStage::UploadingBlobs => "Uploading blobs",
            klaw_storage::BackupProgressStage::UploadingManifest => "Uploading manifest",
            klaw_storage::BackupProgressStage::UpdatingLatestPointer => {
                "Updating latest manifest pointer"
            }
            klaw_storage::BackupProgressStage::CleaningUpRemote => "Cleaning up old manifests",
            klaw_storage::BackupProgressStage::Completed => "Sync completed",
        }
        .to_string(),
        detail: Some(progress.detail),
    }
}

fn render_sync_text_field(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui.label(label);
    let changed = ui.text_edit_singleline(value).changed();
    changed
}

fn render_proxy_fields(ui: &mut egui::Ui, config: &mut crate::settings::ProxyConfig) -> bool {
    let mut changed = false;

    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Host:");
            if ui.text_edit_singleline(&mut config.host).changed() {
                changed = true;
            }
            ui.end_row();

            ui.label("Port:");
            let mut port_str = if config.port == 0 {
                String::new()
            } else {
                config.port.to_string()
            };
            if ui.text_edit_singleline(&mut port_str).changed() {
                if port_str.is_empty() {
                    config.port = 0;
                    changed = true;
                } else if let Ok(port) = port_str.parse::<u16>() {
                    config.port = port;
                    changed = true;
                }
            }
            ui.end_row();
        });

    changed
}
