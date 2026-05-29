use crate::autostart::{self, ReconcileOutcome};
use crate::icon;
use crate::notifications::NotificationCenter;
use crate::panels::PanelRegistry;
use crate::release_check::{ReleaseCheckOutcome, ReleaseUpdateInfo, check_for_release_update};
use crate::runtime_bridge::{
    ProviderRuntimeSnapshot, RuntimeRequestHandle, begin_provider_status_request,
};
use crate::settings::{AppSettings, SyncMode, current_ui_language, load_settings, save_settings};
use crate::state::workbench::TabId;
use crate::state::{UiAction, UiState};
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
use klaw_ui_kit::{
    LocaleDomain, ThemeSwitch, Translator, theme_mode_from_preference, theme_preference,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

const WINDOW_RESIZE_HIT_ZONE: f32 = 8.0;

fn menu_item_label(icon: &'static str, translator: &Translator, key: &str) -> String {
    format!("{} {}", icon, translator.text(key))
}

pub struct ShellUi {
    panels: PanelRegistry,
    notifications: NotificationCenter,
    about_icon: Option<egui::TextureHandle>,
    about_icon_load_failed: bool,
    provider_ids: Vec<String>,
    config_default_provider: String,
    provider_default_models: BTreeMap<String, String>,
    runtime_provider_override: Option<String>,
    pending_provider_override_target: Option<Option<String>>,
    last_provider_sync_at: Instant,
    provider_status_request: Option<RuntimeRequestHandle<ProviderRuntimeSnapshot>>,
    release_check_request: Option<Receiver<Result<ReleaseCheckOutcome, String>>>,
    release_update: Option<ReleaseUpdateInfo>,
    sync_supervisor: SyncSupervisor,
}

const PROVIDER_SYNC_INTERVAL: Duration = Duration::from_secs(2);
const SYNC_POLL_INTERVAL: Duration = Duration::from_secs(5);
const ABOUT_GITHUB_URL: &str = "https://github.com/zhubby/klaw";

fn about_git_commit_sha() -> &'static str {
    option_env!("VERGEN_GIT_SHA").unwrap_or("unknown")
}

impl Default for ShellUi {
    fn default() -> Self {
        let mut notifications = NotificationCenter::default();
        let settings = load_settings();
        match autostart::reconcile(settings.general.launch_at_startup) {
            Ok(ReconcileOutcome::Unchanged) => {}
            #[cfg(target_os = "macos")]
            Ok(ReconcileOutcome::Enabled) => {
                notifications.info("Launch at startup was re-synced with macOS login items.");
            }
            #[cfg(target_os = "macos")]
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
            about_icon: None,
            about_icon_load_failed: false,
            provider_ids: Vec::new(),
            config_default_provider: String::new(),
            provider_default_models: BTreeMap::new(),
            runtime_provider_override: None,
            pending_provider_override_target: None,
            last_provider_sync_at: Instant::now() - PROVIDER_SYNC_INTERVAL,
            provider_status_request: None,
            release_check_request: Some(spawn_release_check()),
            release_update: None,
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

    pub fn set_pending_provider_override(&mut self, provider_id: Option<String>) {
        self.pending_provider_override_target = Some(provider_id.clone());
        self.runtime_provider_override = provider_id;
    }

    pub fn clear_pending_provider_override(&mut self) {
        self.pending_provider_override_target = None;
    }

    pub fn handle_tab_closed(&mut self, tab_id: TabId) {
        self.panels.handle_tab_closed(tab_id.menu);
    }

    fn should_emit_provider_override_action(&self, state: &UiState) -> bool {
        self.pending_provider_override_target.is_none()
            && self.runtime_provider_override != state.runtime_provider_override
    }

    fn sync_provider_choices(&mut self) {
        puffin::profile_scope!("sync_provider_choices");
        if let Some(request) = self.provider_status_request.as_mut()
            && let Some(result) = request.try_take_result()
        {
            self.provider_status_request = None;
            if let Ok(snapshot) = result {
                self.apply_provider_snapshot(snapshot);
            }
        }
        if self.last_provider_sync_at.elapsed() < PROVIDER_SYNC_INTERVAL {
            return;
        }
        if self.provider_status_request.is_some() {
            return;
        }
        self.last_provider_sync_at = Instant::now();
        self.provider_status_request = Some(begin_provider_status_request());
    }

    fn apply_provider_snapshot(&mut self, snapshot: ProviderRuntimeSnapshot) {
        self.config_default_provider = snapshot.default_provider_id;
        if self.pending_provider_override_target.is_none() {
            self.runtime_provider_override = snapshot.runtime_provider_override;
        }
        self.provider_ids = snapshot
            .provider_default_models
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        self.provider_ids.sort();
        self.provider_default_models = snapshot.provider_default_models;
    }

    fn poll_release_check(&mut self) {
        puffin::profile_scope!("poll_release_check");
        let Some(rx) = self.release_check_request.as_ref() else {
            return;
        };

        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.release_check_request = None;
                tracing::warn!("release update check worker disconnected before sending a result");
                return;
            }
        };
        self.release_check_request = None;

        match result {
            Ok(ReleaseCheckOutcome::UpToDate) => {
                self.release_update = None;
            }
            Ok(ReleaseCheckOutcome::UpdateAvailable(update)) => {
                self.release_update = Some(update);
            }
            Err(err) => {
                self.release_update = None;
                tracing::warn!("release update check failed: {err}");
            }
        }
    }

    fn about_icon_texture(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        if self.about_icon.is_none() && !self.about_icon_load_failed {
            match icon::about_icon_texture(ctx) {
                Ok(texture) => self.about_icon = Some(texture),
                Err(err) => {
                    self.about_icon_load_failed = true;
                    self.notifications
                        .warning(format!("Failed to load About dialog icon: {err}"));
                }
            }
        }
        self.about_icon.as_ref()
    }

    pub fn render(&mut self, ctx: &egui::Context, state: &mut UiState) -> Vec<UiAction> {
        puffin::profile_function!();
        let mut actions = Vec::new();
        self.panels.tick(ctx);
        self.sync_provider_choices();
        self.poll_release_check();
        if self.should_emit_provider_override_action(state) {
            actions.push(UiAction::SetRuntimeProviderOverride(
                self.runtime_provider_override.clone(),
            ));
        }
        self.sync_supervisor.tick(&mut self.notifications);
        ctx.request_repaint_after(SYNC_POLL_INTERVAL);
        if self.release_check_request.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        if let Some(action) = window_resize_action(ctx) {
            actions.push(action);
        }

        let translator = Translator::new(LocaleDomain::Gui, current_ui_language());
        egui::TopBottomPanel::top("klaw-menu-bar").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(
                    menu_item_label(regular::FILE, &translator, "menu-file"),
                    |ui| {
                        if ui
                            .button(menu_item_label(
                                regular::FLOPPY_DISK,
                                &translator,
                                "menu-force-persist-layout",
                            ))
                            .clicked()
                        {
                            actions.push(UiAction::ForcePersistLayout);
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .button(menu_item_label(
                                regular::EYE_SLASH,
                                &translator,
                                "menu-hide-window",
                            ))
                            .clicked()
                        {
                            actions.push(UiAction::HideWindow);
                            ui.close();
                        }
                    },
                );

                ui.menu_button(
                    menu_item_label(regular::EYE, &translator, "menu-view"),
                    |ui| {
                        let (icon, key) = if state.fullscreen {
                            (regular::CORNERS_IN, "menu-exit-full-windows")
                        } else {
                            (regular::CORNERS_OUT, "menu-toggle-full-windows")
                        };
                        if ui.button(menu_item_label(icon, &translator, key)).clicked() {
                            actions.push(UiAction::ToggleFullscreen);
                            ui.close();
                        }
                    },
                );

                ui.menu_button(
                    menu_item_label(regular::BROWSERS, &translator, "menu-windows"),
                    |ui| {
                        if ui
                            .button(menu_item_label(
                                regular::MINUS,
                                &translator,
                                "menu-minimize",
                            ))
                            .clicked()
                        {
                            actions.push(UiAction::MinimizeWindow);
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .button(menu_item_label(
                                regular::ARROWS_OUT,
                                &translator,
                                "menu-zoom",
                            ))
                            .clicked()
                        {
                            actions.push(UiAction::ZoomWindow);
                            ui.close();
                        }
                    },
                );

                ui.menu_button(
                    menu_item_label(regular::QUESTION, &translator, "menu-help"),
                    |ui| {
                        if ui
                            .button(menu_item_label(regular::INFO, &translator, "menu-about"))
                            .clicked()
                        {
                            actions.push(UiAction::ShowAbout);
                            ui.close();
                        }
                    },
                );

                let row_height = ui.spacing().interact_size.y;
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), row_height),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui
                            .button(regular::X)
                            .on_hover_text(translator.text("status-hide-window"))
                            .clicked()
                        {
                            actions.push(UiAction::HideWindow);
                        }

                        let zoom_icon = if state.fullscreen {
                            regular::ARROWS_IN
                        } else {
                            regular::ARROWS_OUT
                        };
                        if ui
                            .button(zoom_icon)
                            .on_hover_text(translator.text("status-zoom-window"))
                            .clicked()
                        {
                            actions.push(UiAction::ZoomWindow);
                        }

                        if ui
                            .button(regular::MINUS)
                            .on_hover_text(translator.text("status-minimize-window"))
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
                            if pointer_pressed_on_region && !pointer_in_window_resize_zone(ctx) {
                                actions.push(UiAction::StartWindowDrag);
                            }
                        }
                    },
                );
            });
        });

        egui::TopBottomPanel::bottom("klaw-status-bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(translator.text("status-theme-mode"));
                let mut preference = theme_preference(state.theme_mode);
                let response = ui.add(ThemeSwitch::new(&mut preference));
                if response.changed() {
                    actions.push(UiAction::SetThemeMode(theme_mode_from_preference(
                        preference,
                    )));
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(update) = self.release_update.as_ref() {
                        let mut update_args = HashMap::new();
                        update_args.insert("icon", regular::DOWNLOAD_SIMPLE.to_string());
                        update_args.insert("version", env!("CARGO_PKG_VERSION").to_string());
                        let version_label = egui::RichText::new(
                            translator.text_args("status-update-available", update_args),
                        )
                        .color(ui.visuals().warn_fg_color);
                        let mut hover_args = HashMap::new();
                        hover_args.insert("current", update.current_version.clone());
                        hover_args.insert("latest", update.latest_version.clone());
                        hover_args.insert("name", update.release_name.clone());
                        ui.hyperlink_to(version_label, &update.release_url)
                            .on_hover_text(translator.text_args("status-update-hover", hover_args));
                    } else {
                        let version_label =
                            format!("{} v{}", regular::INFO, env!("CARGO_PKG_VERSION"));
                        ui.label(version_label);
                    }

                    ui.separator();
                    if self.provider_ids.is_empty() {
                        ui.label(format!(
                            "{} {}",
                            translator.text("status-model-provider"),
                            translator.text("status-model-provider-na")
                        ));
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

                        ui.label(translator.text("status-model-provider"));

                        ui.separator();

                        let default_model = self
                            .provider_default_models
                            .get(selected_provider_id)
                            .map(String::as_str)
                            .unwrap_or("N/A");
                        let default_model_display = if default_model == "N/A" {
                            translator.text("status-model-provider-na")
                        } else {
                            default_model.to_string()
                        };
                        let mut model_args = HashMap::new();
                        model_args.insert("model", default_model_display);
                        ui.label(translator.text_args("status-default-model", model_args));
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
            egui::Window::new(translator.text("about-title"))
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.set_min_width(360.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("Klaw").strong().size(22.0));
                        ui.add_space(18.0);

                        if let Some(texture) = self.about_icon_texture(ctx) {
                            let source_size = texture.size_vec2();
                            let max_side = 160.0;
                            let scale = (max_side / source_size.x.max(source_size.y)).min(1.0);
                            let display_size = source_size * scale;
                            ui.add(
                                egui::Image::from_texture(texture).fit_to_exact_size(display_size),
                            );
                            ui.add_space(12.0);
                        }

                        let mut version_args = HashMap::new();
                        version_args.insert("version", env!("CARGO_PKG_VERSION").to_string());
                        ui.label(translator.text_args("about-version", version_args));
                        let mut commit_args = HashMap::new();
                        commit_args.insert("sha", about_git_commit_sha().to_string());
                        ui.monospace(translator.text_args("about-git-commit", commit_args));
                        ui.add_space(4.0);
                        ui.hyperlink_to(ABOUT_GITHUB_URL, ABOUT_GITHUB_URL);
                        ui.add_space(12.0);

                        if ui.button(translator.text("about-close")).clicked() {
                            actions.push(UiAction::HideAbout);
                        }
                    });
                });
        }

        self.notifications.show(ctx);

        actions
    }
}

fn window_resize_action(ctx: &egui::Context) -> Option<UiAction> {
    let (viewport, content_rect, pointer_pos, primary_pressed) = ctx.input(|input| {
        (
            input.viewport().clone(),
            input.content_rect(),
            input.pointer.latest_pos(),
            input.pointer.button_pressed(egui::PointerButton::Primary),
        )
    });
    if viewport.fullscreen == Some(true) || viewport.maximized == Some(true) {
        return None;
    }
    let direction = window_resize_direction(pointer_pos?, content_rect, WINDOW_RESIZE_HIT_ZONE)?;
    ctx.set_cursor_icon(resize_cursor_icon(direction));
    primary_pressed.then_some(UiAction::StartWindowResize(direction))
}

fn pointer_in_window_resize_zone(ctx: &egui::Context) -> bool {
    ctx.input(|input| {
        input.pointer.latest_pos().is_some_and(|pos| {
            window_resize_direction(pos, input.content_rect(), WINDOW_RESIZE_HIT_ZONE).is_some()
        })
    })
}

fn window_resize_direction(
    pos: egui::Pos2,
    rect: egui::Rect,
    hit_zone: f32,
) -> Option<egui::viewport::ResizeDirection> {
    use egui::viewport::ResizeDirection;

    if !rect.contains(pos) {
        return None;
    }

    let near_left = pos.x <= rect.left() + hit_zone;
    let near_right = pos.x >= rect.right() - hit_zone;
    let near_top = pos.y <= rect.top() + hit_zone;
    let near_bottom = pos.y >= rect.bottom() - hit_zone;

    match (near_left, near_right, near_top, near_bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, _, _, _) => Some(ResizeDirection::West),
        (_, true, _, _) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn resize_cursor_icon(direction: egui::viewport::ResizeDirection) -> egui::CursorIcon {
    use egui::CursorIcon;
    use egui::viewport::ResizeDirection;

    match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

fn spawn_release_check() -> Receiver<Result<ReleaseCheckOutcome, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(check_for_release_update());
    });
    rx
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
        puffin::profile_function!();
        {
            puffin::profile_scope!("sync_supervisor_poll_result");
            self.poll_task_result(notifications);
        }

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
        puffin::profile_function!();
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
            let result = {
                puffin::profile_scope!("sync_supervisor_worker");
                match kind {
                    SyncRuntimeTaskKind::StartupCheck => run_startup_check_task(&settings),
                    SyncRuntimeTaskKind::AutoBackup => run_auto_backup_task(&settings),
                    SyncRuntimeTaskKind::ManualBackup
                    | SyncRuntimeTaskKind::RetentionCleanup
                    | SyncRuntimeTaskKind::RefreshRemoteSnapshots
                    | SyncRuntimeTaskKind::RestoreSnapshot => {
                        Err("unsupported sync supervisor task".to_string())
                    }
                }
            };
            let message =
                result.unwrap_or_else(|message| SyncSupervisorMessage::Failed { kind, message });
            let _ = tx.send(message);
        });
    }

    fn poll_task_result(&mut self, notifications: &mut NotificationCenter) {
        puffin::profile_function!();
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
    puffin::profile_function!();
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
    puffin::profile_function!();
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
    use egui::viewport::ResizeDirection;

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
    fn window_resize_direction_detects_corners_edges_and_interior() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));

        assert_eq!(
            window_resize_direction(egui::pos2(2.0, 2.0), rect, 8.0),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(398.0, 2.0), rect, 8.0),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(2.0, 298.0), rect, 8.0),
            Some(ResizeDirection::SouthWest)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(398.0, 298.0), rect, 8.0),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(200.0, 2.0), rect, 8.0),
            Some(ResizeDirection::North)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(200.0, 298.0), rect, 8.0),
            Some(ResizeDirection::South)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(2.0, 150.0), rect, 8.0),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(398.0, 150.0), rect, 8.0),
            Some(ResizeDirection::East)
        );
        assert_eq!(
            window_resize_direction(egui::pos2(200.0, 150.0), rect, 8.0),
            None
        );
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
        let supervisor = SyncSupervisor {
            startup_check_completed: true,
            ..Default::default()
        };

        assert_eq!(supervisor.next_task(&ready_settings(false, None), 0), None);
    }

    #[test]
    fn next_task_runs_auto_backup_after_interval_elapses() {
        let supervisor = SyncSupervisor {
            startup_check_completed: true,
            ..Default::default()
        };
        let mut settings = ready_settings(true, Some(1_000));
        settings.sync.schedule.interval_minutes = 1;

        assert_eq!(
            supervisor.next_task(&settings, 62_000),
            Some(SyncRuntimeTaskKind::AutoBackup)
        );
    }

    #[test]
    fn pending_provider_override_suppresses_duplicate_action() {
        let mut shell = ShellUi::default();
        let state = UiState::default();

        shell.set_pending_provider_override(Some("anthropic".to_string()));

        assert!(!shell.should_emit_provider_override_action(&state));
    }

    #[test]
    fn provider_override_action_emits_when_no_request_is_pending() {
        let mut shell = ShellUi::default();
        let state = UiState::default();

        shell.set_runtime_provider_override(Some("anthropic".to_string()));

        assert!(shell.should_emit_provider_override_action(&state));
    }
}
