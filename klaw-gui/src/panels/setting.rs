use crate::autostart;
use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::settings::{
    AppSettings, ProxyMode, S3SyncConfig, SyncItem, SyncMode, SyncProvider, current_ui_language,
    load_settings, save_settings, save_ui_language,
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
use klaw_ui_kit::{LocaleDomain, Translator, UiLanguage, label_with_hint};
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use tokio::runtime::Builder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    General,
    SecurityPrivacy,
    Network,
    Sync,
}

impl SettingsSection {
    fn title(&self, t: &Translator) -> String {
        match self {
            SettingsSection::General => t.text("setting-section-general"),
            SettingsSection::SecurityPrivacy => t.text("setting-section-security"),
            SettingsSection::Network => t.text("setting-section-network"),
            SettingsSection::Sync => t.text("setting-section-sync"),
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            SettingsSection::General => "\u{2699}",
            SettingsSection::SecurityPrivacy => "\u{1F6E1}",
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
    pending_delete_all_data: bool,
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
            pending_delete_all_data: false,
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

        let t = Self::translator();
        let mut render_body = |ui: &mut egui::Ui, this: &mut SettingPanel| {
            ui.heading(ctx.tab_title);
            ui.label(t.text("setting-subtitle"));
            ui.separator();

            if let Some(err) = &this.save_error {
                ui.colored_label(
                    ui.style().visuals.error_fg_color,
                    t.text_args(
                        "setting-save-error",
                        HashMap::from([("error", err.to_string())]),
                    ),
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
                                            SettingsSection::SecurityPrivacy,
                                            SettingsSection::Network,
                                            SettingsSection::Sync,
                                        ] {
                                            let is_active = this.active_section == section;
                                            let text =
                                                format!("{} {}", section.icon(), section.title(&t));
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
                                            SettingsSection::SecurityPrivacy => this
                                                .render_security_privacy_section(ui, notifications),
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
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

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

        let t = Self::translator();
        if let Err(err) = autostart::apply(desired) {
            self.settings.general.launch_at_startup = previous;
            self.save_error = Some(err.to_string());
            notifications.error(t.text_args(
                "setting-notify-launch-update-failed",
                HashMap::from([("error", err.to_string())]),
            ));
            return;
        }

        if self.try_save() {
            let message = if desired {
                t.text("setting-notify-launch-enabled")
            } else {
                t.text("setting-notify-launch-disabled")
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
                notifications.error(t.text_args(
                    "setting-notify-launch-save-failed",
                    HashMap::from([("error", save_error)]),
                ));
            }
            Err(rollback_err) => {
                let rollback_message = format!(
                    "{save_error}; also failed to restore the previous macOS login item state: {rollback_err}"
                );
                self.save_error = Some(rollback_message.clone());
                notifications.error(t.text_args(
                    "setting-notify-launch-save-and-rollback-failed",
                    HashMap::from([("message", rollback_message)]),
                ));
            }
        }
    }

    fn poll_sync_tasks(&mut self, notifications: &mut NotificationCenter) {
        let t = Self::translator();
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
                    notifications.success(t.text_args(
                        "setting-notify-sync-backup-done",
                        HashMap::from([("id", manifest_id)]),
                    ));
                    clear_task = true;
                }
                Ok(SyncTaskMessage::ListDone { snapshots }) => {
                    sync_runtime_set_remote_snapshots(snapshots);
                    notifications.success(t.text("setting-notify-sync-list-done"));
                    clear_task = true;
                }
                Ok(SyncTaskMessage::RestoreDone { manifest_id }) => {
                    notifications.warning(t.text_args(
                        "setting-notify-sync-restore-done",
                        HashMap::from([("id", manifest_id)]),
                    ));
                    clear_task = true;
                }
                Ok(SyncTaskMessage::CleanupDone) => {
                    notifications.success(t.text("setting-notify-sync-cleanup-done"));
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

    fn spawn_sync_task<F>(&mut self, kind: SyncRuntimeTaskKind, label: String, task: F)
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
        let t_label = Self::translator().text("setting-sync-task-label-backup");
        self.spawn_sync_task(SyncRuntimeTaskKind::ManualBackup, t_label, move || {
            let t = Translator::new(LocaleDomain::Gui, current_ui_language());
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| err.to_string())?;
            runtime.block_on(async move {
                sync_runtime_set_task_progress(
                    SyncRuntimeTaskKind::ManualBackup,
                    Some(SyncRuntimeProgress {
                        fraction: 0.02,
                        stage: t.text("setting-sync-stage-connecting"),
                        detail: Some(t.text("setting-sync-stage-validating")),
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
                    .create_upload_and_cleanup_snapshot_with_progress(&plan, keep_last, &mut report)
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
        });
    }

    fn refresh_remote_snapshots(&mut self) {
        let config = self.sync_config();
        let device_id = self.settings.sync.device_id.clone();
        let t_label = Self::translator().text("setting-sync-task-label-refresh");
        self.spawn_sync_task(
            SyncRuntimeTaskKind::RefreshRemoteSnapshots,
            t_label,
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
        let t_label = Self::translator().text("setting-sync-task-label-restore");
        self.spawn_sync_task(SyncRuntimeTaskKind::RestoreSnapshot, t_label, move || {
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
        });
    }

    fn run_retention_cleanup(&mut self) {
        let config = self.sync_config();
        let device_id = self.settings.sync.device_id.clone();
        let keep_last = self.settings.sync.retention.keep_last;
        let t_label = Self::translator().text("setting-sync-task-label-cleanup");
        self.spawn_sync_task(SyncRuntimeTaskKind::RetentionCleanup, t_label, move || {
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
        });
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
        let translator = Translator::new(LocaleDomain::Gui, self.settings.general.ui_language);
        ui.strong(translator.text("setting-general-title"));
        ui.add_space(12.0);

        // ── Block 1: Language ──
        ui.add_space(4.0);
        let mut requested_language = None;
        egui::Grid::new("general-language-grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                ui.label(format!("{}:", translator.text("language")));
                egui::ComboBox::from_id_salt("settings-ui-language")
                    .width(160.0)
                    .selected_text(self.settings.general.ui_language.label())
                    .show_ui(ui, |ui| {
                        for language in UiLanguage::available() {
                            if ui
                                .selectable_label(
                                    self.settings.general.ui_language == *language,
                                    language.label(),
                                )
                                .clicked()
                            {
                                requested_language = Some(*language);
                                ui.close();
                            }
                        }
                    });
                ui.end_row();
            });
        if let Some(language) = requested_language
            && language != self.settings.general.ui_language
        {
            match save_ui_language(language) {
                Ok(settings) => {
                    self.settings = settings;
                    self.save_error = None;
                    notifications.success(translator.text("setting-notify-language-updated"));
                }
                Err(err) => {
                    self.save_error = Some(err.to_string());
                    notifications.error(translator.text_args(
                        "setting-notify-language-update-failed",
                        HashMap::from([("error", err.to_string())]),
                    ));
                }
            }
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // ── Block 2: Launch at startup ──
        let previous_launch_setting = self.settings.general.launch_at_startup;
        let enable_unavailable_reason = autostart::enable_availability()
            .unsupported_reason()
            .map(str::to_owned);
        let mut startup_setting_changed = false;
        let hint_text = if let Some(reason) = &enable_unavailable_reason {
            translator.text_args(
                "setting-launch-at-startup-hint-unavailable",
                HashMap::from([("reason", reason.clone())]),
            )
        } else {
            translator.text("setting-launch-at-startup-hint")
        };
        egui::Grid::new("general-startup-grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                label_with_hint(
                    ui,
                    &translator.text("setting-launch-at-startup"),
                    &hint_text,
                );
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(enable_unavailable_reason.is_none(), |ui| {
                        startup_setting_changed = ui
                            .radio_value(
                                &mut self.settings.general.launch_at_startup,
                                true,
                                translator.text("setting-yes"),
                            )
                            .changed();
                    });
                    startup_setting_changed |= ui
                        .radio_value(
                            &mut self.settings.general.launch_at_startup,
                            false,
                            translator.text("setting-no"),
                        )
                        .changed();
                });
                ui.end_row();
            });
        if startup_setting_changed {
            self.persist_launch_at_startup_change(previous_launch_setting, notifications);
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // ── Block 3: Theme ──
        ui.label(translator.text_args(
            "setting-theme-mode-current",
            HashMap::from([("mode", self.theme_state.theme_mode.label().to_string())]),
        ));
        ui.add_space(8.0);

        let mut theme_changed = false;
        egui::Grid::new("general-theme-grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                ui.label(translator.text("setting-light-theme"));
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

                ui.label(translator.text("setting-dark-theme"));
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
        ui.small(translator.text("setting-theme-default-hint"));

        if theme_changed {
            self.save_theme_state(ui.ctx());
        }
    }

    fn render_security_privacy_section(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        let t = Self::translator();
        ui.strong(t.text("setting-security-title"));
        ui.add_space(12.0);

        // ── Block 1: Location Services ──
        ui.add_space(4.0);
        ui.strong(t.text("setting-location-services"));
        ui.add_space(8.0);
        let location_status = current_location_status();
        egui::Grid::new("security-location-grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                ui.label(t.text("setting-system-location-services"));
                ui.label(bool_status_label(location_status.services_enabled, &t));
                ui.end_row();

                ui.label(t.text("setting-app-authorization"));
                ui.label(location_status.authorization_label(&t));
                ui.end_row();

                if let Some(detail) = location_status.detail_message(&t) {
                    ui.label(t.text("setting-detail"));
                    ui.small(detail);
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        if ui
            .button(t.text("setting-open-location-settings"))
            .clicked()
        {
            match open_location_settings() {
                Ok(()) => notifications.info(t.text("setting-notify-location-settings-opened")),
                Err(err) => notifications.error(t.text_args(
                    "setting-notify-location-settings-failed",
                    HashMap::from([("error", err.to_string())]),
                )),
            }
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(12.0);

        // ── Block 2: Danger Zone ──
        ui.add_space(4.0);
        ui.strong(t.text("setting-danger-zone"));
        ui.add_space(8.0);
        egui::Grid::new("security-danger-grid")
            .num_columns(2)
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                label_with_hint(
                    ui,
                    &t.text("setting-delete-all-app-data"),
                    &t.text("setting-delete-all-app-data-hint"),
                );
                ui.horizontal(|ui| {
                    ui.style_mut().visuals.widgets.noninteractive.fg_stroke =
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 50, 50));
                    if ui.button(t.text("setting-delete-all-data-btn")).clicked() {
                        self.pending_delete_all_data = true;
                    }
                });
                ui.end_row();
            });

        // ── Confirmation modal ──
        if self.pending_delete_all_data {
            let mut keep_open = true;
            egui::Window::new(t.text("setting-confirm-delete-title"))
                .collapsible(false)
                .resizable(false)
                .open(&mut keep_open)
                .show(ui.ctx(), |ui| {
                    ui.colored_label(
                        ui.style().visuals.error_fg_color,
                        t.text("setting-confirm-delete-warning"),
                    );
                    ui.add_space(8.0);
                    ui.label(t.text("setting-confirm-delete-description"));
                    ui.label(format!(
                        "\u{2022} {}",
                        t.text("setting-confirm-delete-item-config")
                    ));
                    ui.label(format!(
                        "\u{2022} {}",
                        t.text("setting-confirm-delete-item-sessions")
                    ));
                    ui.label(format!(
                        "\u{2022} {}",
                        t.text("setting-confirm-delete-item-skills")
                    ));
                    ui.label(format!(
                        "\u{2022} {}",
                        t.text("setting-confirm-delete-item-memory")
                    ));
                    ui.label(format!(
                        "\u{2022} {}",
                        t.text("setting-confirm-delete-item-databases")
                    ));
                    ui.add_space(8.0);
                    ui.strong(t.text("setting-confirm-delete-irreversible"));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(t.text("setting-cancel")).clicked() {
                            self.pending_delete_all_data = false;
                        }
                        ui.style_mut().visuals.widgets.noninteractive.fg_stroke =
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 50, 50));
                        if ui.button(t.text("setting-delete-everything-btn")).clicked() {
                            self.pending_delete_all_data = false;
                            match klaw_util::default_data_dir() {
                                Some(dir) => {
                                    if let Err(err) = std::fs::remove_dir_all(&dir) {
                                        notifications.error(t.text_args(
                                            "setting-notify-delete-data-failed",
                                            HashMap::from([("error", err.to_string())]),
                                        ));
                                    } else {
                                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                }
                                None => {
                                    notifications
                                        .error(t.text("setting-notify-data-dir-unavailable"));
                                }
                            }
                        }
                    });
                });
            if !keep_open {
                self.pending_delete_all_data = false;
            }
        }
    }

    fn render_network_section(&mut self, ui: &mut egui::Ui) {
        let t = Self::translator();
        ui.strong(t.text("setting-network-title"));
        ui.add_space(8.0);

        ui.label(t.text("setting-proxy-configuration"));
        ui.add_space(4.0);

        if ui
            .radio_value(
                &mut self.settings.network.proxy_mode,
                ProxyMode::NoProxy,
                t.text("setting-proxy-no-proxy"),
            )
            .changed()
            || ui
                .radio_value(
                    &mut self.settings.network.proxy_mode,
                    ProxyMode::SystemProxy,
                    t.text("setting-proxy-system"),
                )
                .changed()
            || ui
                .radio_value(
                    &mut self.settings.network.proxy_mode,
                    ProxyMode::ManualProxy,
                    t.text("setting-proxy-manual"),
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
                ui.strong(t.text("setting-http-proxy"));
                if render_proxy_fields(ui, &mut self.settings.network.http_proxy, &t) {
                    self.try_save();
                }
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.strong(t.text("setting-https-proxy"));
                if render_proxy_fields(ui, &mut self.settings.network.https_proxy, &t) {
                    self.try_save();
                }
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.strong(t.text("setting-socks5-proxy"));
                if render_proxy_fields(ui, &mut self.settings.network.socks5_proxy, &t) {
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
        let t = Self::translator();
        ui.strong(t.text("setting-sync-title"));
        ui.add_space(8.0);
        let sync_validation_error = self.sync_validation_error();

        let mut changed = false;

        ui.horizontal(|ui| {
            let previous = self.settings.sync.enabled;
            ui.add(klaw_ui_kit::toggle::toggle(&mut self.settings.sync.enabled));
            ui.label(t.text("setting-sync-enable-label"));
            let changed_now = self.settings.sync.enabled != previous;
            changed |= changed_now;
            if changed_now {
                if self.settings.sync.enabled {
                    notifications.success(t.text("setting-notify-sync-enabled"));
                } else {
                    notifications.info(t.text("setting-notify-sync-disabled"));
                }
            }
        });

        ui.add_space(8.0);
        egui::CollapsingHeader::new(t.text("setting-sync-general"))
            .id_salt("sync-general")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("sync-general-grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(t.text("setting-sync-provider"));
                        changed |= ui
                            .radio_value(
                                &mut self.settings.sync.provider,
                                SyncProvider::S3,
                                t.text("setting-sync-provider-s3"),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(t.text("setting-sync-mode"));
                        changed |= ui
                            .radio_value(
                                &mut self.settings.sync.mode,
                                SyncMode::ManifestVersioned,
                                t.text("setting-sync-mode-versioned"),
                            )
                            .changed();
                        ui.end_row();

                        ui.label(t.text("setting-sync-device-id"));
                        changed |= ui
                            .text_edit_singleline(&mut self.settings.sync.device_id)
                            .changed();
                        ui.end_row();
                    });
            });

        egui::CollapsingHeader::new(t.text("setting-sync-schedule-header"))
            .id_salt("sync-schedule")
            .default_open(true)
            .show(ui, |ui| {
                changed |= ui
                    .checkbox(
                        &mut self.settings.sync.schedule.auto_backup,
                        t.text("setting-sync-auto-backup"),
                    )
                    .changed();
                egui::Grid::new("sync-schedule-grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(t.text("setting-sync-interval"));
                        let mut interval = self.settings.sync.schedule.interval_minutes.to_string();
                        if ui.text_edit_singleline(&mut interval).changed()
                            && let Ok(parsed) = interval.parse::<u32>()
                        {
                            self.settings.sync.schedule.interval_minutes = parsed.max(1);
                            changed = true;
                        }
                        ui.end_row();

                        ui.label(t.text("setting-sync-keep-latest"));
                        let mut keep_last = self.settings.sync.retention.keep_last.to_string();
                        if ui.text_edit_singleline(&mut keep_last).changed()
                            && let Ok(parsed) = keep_last.parse::<u32>()
                        {
                            self.settings.sync.retention.keep_last = parsed.max(1);
                            changed = true;
                        }
                        ui.end_row();
                    });
            });

        egui::CollapsingHeader::new(t.text("setting-sync-s3-header"))
            .id_salt("sync-s3")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("sync-s3-grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-endpoint"),
                            &mut self.settings.sync.s3.endpoint,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-region"),
                            &mut self.settings.sync.s3.region,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-bucket"),
                            &mut self.settings.sync.s3.bucket,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-prefix"),
                            &mut self.settings.sync.s3.prefix,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-access-key"),
                            &mut self.settings.sync.s3.access_key,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-secret-key"),
                            &mut self.settings.sync.s3.secret_key,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-session-token"),
                            &mut self.settings.sync.s3.session_token,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-access-key-env"),
                            &mut self.settings.sync.s3.access_key_env,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-secret-key-env"),
                            &mut self.settings.sync.s3.secret_key_env,
                        );
                        ui.end_row();
                        changed |= render_sync_text_field(
                            ui,
                            t.text("setting-s3-session-token-env"),
                            &mut self.settings.sync.s3.session_token_env,
                        );
                        ui.end_row();
                    });
                changed |= ui
                    .checkbox(
                        &mut self.settings.sync.s3.force_path_style,
                        t.text("setting-s3-force-path-style"),
                    )
                    .changed();
            });

        egui::CollapsingHeader::new(t.text("setting-sync-scope-header"))
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
                    if ui
                        .checkbox(&mut checked, item.label_with_translator(&t))
                        .clicked()
                    {
                        if checked && index.is_none() {
                            self.settings.sync.backup_items.push(*item);
                            changed = true;
                        } else if !checked && let Some(idx) = index {
                            self.settings.sync.backup_items.remove(idx);
                            changed = true;
                        }
                    }
                }
                ui.add_space(4.0);
                ui.label(t.text("setting-sync-scope-restore-hint"));
            });

        egui::CollapsingHeader::new(t.text("setting-sync-actions-header"))
            .id_salt("sync-actions")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(remote_update) = &runtime.remote_update {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        t.text_args(
                            "setting-sync-remote-newer",
                            HashMap::from([
                                ("id", remote_update.manifest_id.clone()),
                                ("device", remote_update.device_id.clone()),
                            ]),
                        ),
                    );
                    ui.label(t.text_args(
                        "setting-sync-remote-created",
                        HashMap::from([(
                            "time",
                            crate::time_format::format_timestamp_millis(remote_update.created_at),
                        )]),
                    ));
                    ui.add_space(6.0);
                }
                ui.label(t.text_args(
                    "setting-sync-last-sync",
                    HashMap::from([(
                        "time",
                        format_optional_timestamp_millis(self.settings.sync.last_sync_at),
                    )]),
                ));
                ui.label(
                    t.text_args(
                        "setting-sync-last-manifest-id",
                        HashMap::from([(
                            "id",
                            self.settings
                                .sync
                                .last_manifest_id
                                .clone()
                                .unwrap_or_default(),
                        )]),
                    ),
                );
                if let Some(task) = &runtime.active_task {
                    ui.label(t.text_args(
                        "setting-sync-in-progress",
                        HashMap::from([("label", task.label.clone())]),
                    ));
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
                        .add_enabled(can_run, egui::Button::new(t.text("setting-sync-run-now")))
                        .clicked()
                    {
                        self.run_backup();
                    }
                    if ui
                        .add_enabled(
                            !self.sync_busy() && sync_validation_error.is_none(),
                            egui::Button::new(t.text("setting-sync-refresh-remote")),
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
                            egui::Button::new(t.text("setting-sync-run-cleanup")),
                        )
                        .clicked()
                    {
                        self.run_retention_cleanup();
                    }
                });
                if self.settings.sync.enabled && sync_validation_error.is_none() {
                    ui.small(t.text("setting-sync-manual-progress-hint"));
                }
            });

        egui::CollapsingHeader::new(t.text("setting-sync-remote-header"))
            .id_salt("sync-remote")
            .default_open(true)
            .show(ui, |ui| {
                if runtime.remote_snapshots.is_empty() {
                    ui.label(t.text("setting-sync-no-remote"));
                } else {
                    let mut restore_target = None;
                    for snapshot in &runtime.remote_snapshots {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(t.text_args(
                                    "setting-sync-manifest-id",
                                    HashMap::from([("id", snapshot.manifest_id.clone())]),
                                ));
                                ui.label(t.text_args(
                                    "setting-sync-created",
                                    HashMap::from([(
                                        "time",
                                        crate::time_format::format_timestamp_millis(
                                            snapshot.created_at,
                                        ),
                                    )]),
                                ));
                                ui.label(t.text_args(
                                    "setting-sync-device",
                                    HashMap::from([("device", snapshot.device_id.clone())]),
                                ));
                            });
                            if ui
                                .add_enabled(
                                    !self.sync_busy() && sync_validation_error.is_none(),
                                    egui::Button::new(t.text("setting-sync-restore-btn")),
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
            egui::Window::new(t.text("setting-sync-confirm-restore-title"))
                .collapsible(false)
                .resizable(false)
                .open(&mut keep_open)
                .show(ui.ctx(), |ui| {
                    ui.label(t.text("setting-sync-confirm-restore-desc1"));
                    ui.label(t.text("setting-sync-confirm-restore-desc2"));
                    ui.label(t.text("setting-sync-confirm-restore-desc3"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(t.text("setting-cancel")).clicked() {
                            self.pending_restore_manifest_id = None;
                        }
                        if ui
                            .add_enabled(
                                !self.sync_busy(),
                                egui::Button::new(t.text("setting-sync-restore-now-btn")),
                            )
                            .clicked()
                        {
                            self.pending_restore_manifest_id = None;
                            self.restore_snapshot(manifest_id.clone());
                            notifications.info(t.text("setting-notify-restore-started"));
                        }
                    });
                });
            if !keep_open {
                self.pending_restore_manifest_id = None;
            }
        }
    }
}

fn bool_status_label(enabled: bool, t: &Translator) -> String {
    if enabled {
        t.text("setting-enabled")
    } else {
        t.text("setting-disabled")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocationAuthorizationState {
    #[cfg(target_os = "macos")]
    NotDetermined,
    #[cfg(target_os = "macos")]
    Restricted,
    #[cfg(target_os = "macos")]
    Denied,
    #[cfg(target_os = "macos")]
    AuthorizedAlways,
    #[cfg(target_os = "macos")]
    AuthorizedWhenInUse,
    #[cfg(not(target_os = "macos"))]
    UnsupportedPlatform,
    #[cfg(target_os = "macos")]
    Unknown(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocationStatus {
    services_enabled: bool,
    authorization: LocationAuthorizationState,
}

impl LocationStatus {
    fn authorization_label(self, t: &Translator) -> String {
        match self.authorization {
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::NotDetermined => t.text("setting-auth-not-determined"),
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::Restricted => t.text("setting-auth-restricted"),
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::Denied => t.text("setting-auth-denied"),
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::AuthorizedAlways => {
                t.text("setting-auth-authorized-always")
            }
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::AuthorizedWhenInUse => {
                t.text("setting-auth-authorized-when-in-use")
            }
            #[cfg(not(target_os = "macos"))]
            LocationAuthorizationState::UnsupportedPlatform => {
                t.text("setting-auth-unsupported-platform")
            }
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::Unknown(_) => t.text("setting-auth-unknown"),
        }
    }

    fn detail_message(self, t: &Translator) -> Option<String> {
        match self.authorization {
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::NotDetermined => {
                Some(t.text("setting-auth-detail-not-determined"))
            }
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::Restricted => {
                Some(t.text("setting-auth-detail-restricted"))
            }
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::Denied => Some(t.text("setting-auth-detail-denied")),
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::AuthorizedAlways
            | LocationAuthorizationState::AuthorizedWhenInUse => (!self.services_enabled)
                .then_some(t.text("setting-auth-detail-auth-but-services-off")),
            #[cfg(not(target_os = "macos"))]
            LocationAuthorizationState::UnsupportedPlatform => {
                Some(t.text("setting-auth-detail-unsupported-platform"))
            }
            #[cfg(target_os = "macos")]
            LocationAuthorizationState::Unknown(_) => Some(t.text("setting-auth-detail-unknown")),
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
    let t = Translator::new(LocaleDomain::Gui, current_ui_language());
    SyncRuntimeProgress {
        fraction: progress.fraction.clamp(0.0, 1.0),
        stage: match progress.stage {
            klaw_storage::BackupProgressStage::ReconcilingRemote => {
                t.text("setting-sync-stage-reconciling")
            }
            klaw_storage::BackupProgressStage::PreparingManifest => {
                t.text("setting-sync-stage-preparing")
            }
            klaw_storage::BackupProgressStage::UploadingBlobs => {
                t.text("setting-sync-stage-uploading-blobs")
            }
            klaw_storage::BackupProgressStage::UploadingManifest => {
                t.text("setting-sync-stage-uploading-manifest")
            }
            klaw_storage::BackupProgressStage::UpdatingLatestPointer => {
                t.text("setting-sync-stage-updating-pointer")
            }
            klaw_storage::BackupProgressStage::CleaningUpRemote => {
                t.text("setting-sync-stage-cleaning-up")
            }
            klaw_storage::BackupProgressStage::Completed => t.text("setting-sync-stage-completed"),
        },
        detail: Some(progress.detail),
    }
}

fn render_sync_text_field(ui: &mut egui::Ui, label: String, value: &mut String) -> bool {
    ui.label(label);
    ui.text_edit_singleline(value).changed()
}

fn render_proxy_fields(
    ui: &mut egui::Ui,
    config: &mut crate::settings::ProxyConfig,
    t: &Translator,
) -> bool {
    let mut changed = false;

    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label(t.text("setting-proxy-host"));
            if ui.text_edit_singleline(&mut config.host).changed() {
                changed = true;
            }
            ui.end_row();

            ui.label(t.text("setting-proxy-port"));
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
