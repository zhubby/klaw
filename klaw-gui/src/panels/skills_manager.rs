use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge;
use crate::settings::current_ui_language;
use crate::time_format::format_timestamp_millis;
use crate::widgets::markdown;
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_file_dialog::FileDialog;
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore};
use klaw_skill::{
    FileSystemSkillStore, RegistrySkillSummary, SkillRecord, SkillSourceKind, SkillSummary,
    SkillUninstallResult, SkillsManager, open_default_skills_manager,
};
use klaw_ui_kit::{LocaleDomain, Translator};
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::thread;
use tokio::runtime::Builder;

#[derive(Debug, Clone, Default)]
struct InstallSkillWindow {
    selected_registry: String,
    skills: Vec<RegistrySkillSummary>,
    error: Option<String>,
}

pub struct SkillsManagerPanel {
    config_store: Option<ConfigStore>,
    config: AppConfig,
    skill_root: Option<PathBuf>,
    loaded: bool,
    items: Vec<SkillSummary>,
    selected_skill: Option<String>,
    detail_name: Option<String>,
    detail_record: Option<SkillRecord>,
    detail_window_open: bool,
    install_window: Option<InstallSkillWindow>,
    local_install_dialog: FileDialog,
    delete_confirm_skill: Option<(String, Option<String>)>,
}

impl Default for SkillsManagerPanel {
    fn default() -> Self {
        Self {
            config_store: None,
            config: AppConfig::default(),
            skill_root: None,
            loaded: false,
            items: Vec::new(),
            selected_skill: None,
            detail_name: None,
            detail_record: None,
            detail_window_open: false,
            install_window: None,
            local_install_dialog: FileDialog::new(),
            delete_confirm_skill: None,
        }
    }
}

impl SkillsManagerPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn request_runtime_skills_reload(notifications: &mut NotificationCenter) {
        if let Err(err) = runtime_bridge::request_reload_skills_prompt() {
            notifications.warning(Self::translator().text_args(
                "skills-mgr-notify-reload-not-sent",
                HashMap::from([("error", err.to_string())]),
            ));
        }
    }

    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.config_store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.config_store = Some(store);
                self.apply_snapshot(snapshot);
            }
            Err(err) => notifications.error(Self::translator().text_args(
                "skills-mgr-notify-load-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
    }

    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.load_items(notifications, false);
    }

    fn reload_config_snapshot(&mut self, notifications: &mut NotificationCenter) -> bool {
        self.ensure_store_loaded(notifications);
        let Some(store) = self.config_store.as_ref() else {
            return false;
        };

        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                true
            }
            Err(err) => {
                notifications.error(Self::translator().text_args(
                    "skills-mgr-notify-reload-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
                false
            }
        }
    }

    fn load_items(&mut self, notifications: &mut NotificationCenter, reload_config: bool) {
        self.ensure_store_loaded(notifications);
        let Some(store) = self.config_store.as_ref() else {
            return;
        };

        let snapshot = if reload_config {
            match store.reload() {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    notifications.error(Self::translator().text_args(
                        "skills-mgr-notify-reload-failed",
                        HashMap::from([("error", err.to_string())]),
                    ));
                    return;
                }
            }
        } else {
            store.snapshot()
        };
        self.apply_snapshot(snapshot);

        match load_installed_skill_list() {
            Ok((skill_root, items)) => {
                self.skill_root = Some(skill_root);
                self.items = items;
                self.loaded = true;
                if let Some(current_name) = self.detail_name.as_deref()
                    && !self.items.iter().any(|item| item.name == current_name)
                {
                    self.detail_name = None;
                    self.detail_record = None;
                    self.detail_window_open = false;
                }
            }
            Err(err) => notifications.error(Self::translator().text_args(
                "skills-mgr-notify-load-skills-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn load_detail(&mut self, skill_name: &str, notifications: &mut NotificationCenter) {
        match load_installed_skill_detail(skill_name.to_string()) {
            Ok(record) => {
                self.detail_name = Some(skill_name.to_string());
                self.detail_record = Some(record);
                self.detail_window_open = true;
            }
            Err(err) => {
                self.detail_record = None;
                self.detail_window_open = false;
                notifications.error(Self::translator().text_args(
                    "skills-mgr-notify-load-detail-failed",
                    HashMap::from([
                        ("skill_name", skill_name.to_string()),
                        ("error", err.to_string()),
                    ]),
                ));
            }
        }
    }

    fn open_install_window(&mut self, notifications: &mut NotificationCenter) {
        let _ = self.reload_config_snapshot(notifications);

        if self.config.skills.registries.is_empty() {
            notifications.warning(Self::translator().text("skills-mgr-notify-no-registry"));
            return;
        }

        let selected_registry = self
            .install_window
            .as_ref()
            .map(|window| window.selected_registry.clone())
            .filter(|name| self.config.skills.registries.contains_key(name))
            .or_else(|| self.config.skills.registries.keys().next().cloned())
            .unwrap_or_default();

        let mut window = InstallSkillWindow {
            selected_registry,
            ..InstallSkillWindow::default()
        };
        self.reload_install_window_catalog(&mut window);
        if let Some(error) = window.error.as_ref() {
            notifications.warning(error.clone());
        }
        self.install_window = Some(window);
    }

    fn open_local_install_dialog(&mut self) {
        self.local_install_dialog.pick_file();
    }

    fn handle_local_install_selection(&mut self, notifications: &mut NotificationCenter) {
        let Some(selected_path) = self.local_install_dialog.take_picked() else {
            return;
        };
        match install_local_skill_from_markdown_path(&selected_path, &self.items) {
            Ok(result) => {
                self.load_items(notifications, false);
                Self::request_runtime_skills_reload(notifications);
                notifications.success(Self::translator().text_args(
                    "skills-mgr-notify-local-install-success",
                    HashMap::from([
                        ("skill_name", result.skill_name.clone()),
                        ("source_dir", result.source_dir.display().to_string()),
                        ("target_dir", result.target_dir.display().to_string()),
                    ]),
                ));
            }
            Err(err) => notifications.error(Self::translator().text_args(
                "skills-mgr-notify-local-install-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn reload_install_window_catalog(&self, window: &mut InstallSkillWindow) {
        if window.selected_registry.is_empty() {
            window.skills.clear();
            window.error =
                Some(Self::translator().text("skills-mgr-install-select-registry-error"));
            return;
        }

        match load_source_catalog(window.selected_registry.clone()) {
            Ok(skills) => {
                window.skills = skills;
                window.error = None;
            }
            Err(err) => {
                window.skills.clear();
                window.error = Some(err);
            }
        }
    }

    fn install_registry_skill(
        &mut self,
        registry_name: &str,
        skill_id: &str,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        let Some(store) = self.config_store.as_ref() else {
            return;
        };

        let registry_name = registry_name.to_string();
        let skill_id = skill_id.to_string();
        let changed = match store.update_config(|config| {
            let (next_config, changed) =
                Self::add_skill_to_config(config.clone(), &registry_name, &skill_id);
            if changed {
                *config = next_config;
            }
            Ok(changed)
        }) {
            Ok((snapshot, changed)) => {
                self.apply_snapshot(snapshot);
                changed
            }
            Err(err) => {
                notifications.error(Self::translator().text_args(
                    "skills-mgr-notify-install-config-save-failed",
                    HashMap::from([("skill_id", skill_id.clone()), ("error", err.to_string())]),
                ));
                return;
            }
        };
        if !changed {
            notifications.info(Self::translator().text_args(
                "skills-mgr-notify-already-installed",
                HashMap::from([("skill_id", skill_id.clone())]),
            ));
            return;
        }

        match install_from_registry_in_store(registry_name.clone(), skill_id.clone()) {
            Ok((_record, _already_installed)) => {
                self.load_items(notifications, false);
                if let Some(mut window) = self.install_window.take() {
                    self.reload_install_window_catalog(&mut window);
                    self.install_window = Some(window);
                }
                Self::request_runtime_skills_reload(notifications);
                notifications.success(Self::translator().text_args(
                    "skills-mgr-notify-install-success",
                    HashMap::from([
                        ("skill_id", skill_id.clone()),
                        ("registry_name", registry_name.clone()),
                    ]),
                ));
            }
            Err(err) => {
                Self::request_runtime_skills_reload(notifications);
                notifications.warning(Self::translator().text_args(
                    "skills-mgr-notify-install-partial",
                    HashMap::from([("skill_id", skill_id.clone()), ("error", err.to_string())]),
                ));
            }
        }
    }

    fn uninstall_registry_skill_from_window(
        &mut self,
        registry_name: &str,
        skill_id: &str,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        let Some(store) = self.config_store.as_ref() else {
            return;
        };

        let registry_name = registry_name.to_string();
        let skill_id = skill_id.to_string();
        let changed = match store.update_config(|config| {
            let (next_config, changed) =
                Self::remove_skill_from_config(config.clone(), &registry_name, &skill_id);
            if changed {
                *config = next_config;
            }
            Ok(changed)
        }) {
            Ok((snapshot, changed)) => {
                self.apply_snapshot(snapshot);
                changed
            }
            Err(err) => {
                notifications.error(Self::translator().text_args(
                    "skills-mgr-notify-uninstall-config-update-failed",
                    HashMap::from([("skill_id", skill_id.clone()), ("error", err.to_string())]),
                ));
                return;
            }
        };
        if !changed {
            notifications.info(Self::translator().text_args(
                "skills-mgr-notify-not-installed",
                HashMap::from([("skill_id", skill_id.clone())]),
            ));
            return;
        }

        match uninstall_from_registry_in_store(registry_name.clone(), skill_id.clone()) {
            Ok(()) => {
                if self.detail_name.as_deref() == Some(skill_id.as_str()) {
                    self.detail_name = None;
                    self.detail_record = None;
                    self.detail_window_open = false;
                }
                self.load_items(notifications, false);
                if let Some(mut window) = self.install_window.take() {
                    self.reload_install_window_catalog(&mut window);
                    self.install_window = Some(window);
                }
                Self::request_runtime_skills_reload(notifications);
                notifications.success(Self::translator().text_args(
                    "skills-mgr-notify-uninstall-success",
                    HashMap::from([
                        ("skill_id", skill_id.clone()),
                        ("registry_name", registry_name.clone()),
                    ]),
                ));
            }
            Err(err) => {
                Self::request_runtime_skills_reload(notifications);
                notifications.warning(Self::translator().text_args(
                    "skills-mgr-notify-uninstall-partial",
                    HashMap::from([("skill_id", skill_id.clone()), ("error", err.to_string())]),
                ));
            }
        }
    }

    fn uninstall_skill(
        &mut self,
        skill_name: &str,
        registry: Option<&str>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        let Some(store) = self.config_store.as_ref() else {
            return;
        };

        let mut config_updated = false;
        if let Some(registry_name) = registry {
            let registry_name = registry_name.to_string();
            match store.update_config(|config| {
                let (next_config, changed) =
                    Self::remove_skill_from_config(config.clone(), &registry_name, skill_name);
                if changed {
                    *config = next_config;
                }
                Ok(changed)
            }) {
                Ok((snapshot, changed)) => {
                    self.apply_snapshot(snapshot);
                    config_updated = changed;
                }
                Err(err) => {
                    notifications.error(Self::translator().text_args(
                        "skills-mgr-notify-remove-config-failed",
                        HashMap::from([
                            ("skill_name", skill_name.to_string()),
                            ("error", err.to_string()),
                        ]),
                    ));
                    return;
                }
            }
        }

        match uninstall_installed_skill_from_store(skill_name.to_string()) {
            Ok(result) => {
                if self.detail_name.as_deref() == Some(skill_name) {
                    self.detail_name = None;
                    self.detail_record = None;
                    self.detail_window_open = false;
                }
                self.load_items(notifications, false);
                Self::request_runtime_skills_reload(notifications);
                notifications.success(format_uninstall_result(skill_name, registry, &result));
            }
            Err(err) => {
                self.load_items(notifications, false);
                if config_updated {
                    notifications.warning(Self::translator().text_args(
                        "skills-mgr-notify-uninstall-local-cleanup-failed",
                        HashMap::from([
                            ("skill_name", skill_name.to_string()),
                            ("error", err.to_string()),
                        ]),
                    ));
                } else {
                    notifications.error(Self::translator().text_args(
                        "skills-mgr-notify-uninstall-failed",
                        HashMap::from([
                            ("skill_name", skill_name.to_string()),
                            ("error", err.to_string()),
                        ]),
                    ));
                }
            }
        }
    }

    fn remove_skill_from_config(
        mut config: AppConfig,
        registry_name: &str,
        skill_name: &str,
    ) -> (AppConfig, bool) {
        let Some(registry) = config.skills.registries.get_mut(registry_name) else {
            return (config, false);
        };

        let before = registry.installed.len();
        registry
            .installed
            .retain(|installed| installed != skill_name);
        let changed = before != registry.installed.len();
        (config, changed)
    }

    fn add_skill_to_config(
        mut config: AppConfig,
        registry_name: &str,
        skill_name: &str,
    ) -> (AppConfig, bool) {
        let Some(registry) = config.skills.registries.get_mut(registry_name) else {
            return (config, false);
        };

        if registry
            .installed
            .iter()
            .any(|installed| installed == skill_name)
        {
            return (config, false);
        }
        registry.installed.push(skill_name.to_string());
        registry.installed.sort();
        (config, true)
    }

    fn source_label(summary: &SkillSummary) -> String {
        match summary.source_kind {
            SkillSourceKind::Local => Self::translator().text("skills-mgr-source-local"),
            SkillSourceKind::Registry => Self::translator().text("skills-mgr-source-registry"),
        }
    }

    fn stale_display(summary: &SkillSummary) -> (&'static str, Color32, String) {
        match summary.stale {
            Some(true) => (
                regular::WARNING,
                Color32::from_rgb(200, 150, 50),
                Self::translator().text("skills-mgr-state-stale"),
            ),
            Some(false) => (
                regular::CHECK_CIRCLE,
                Color32::from_rgb(50, 180, 80),
                Self::translator().text("skills-mgr-state-fresh"),
            ),
            None => (
                regular::MINUS,
                Color32::from_rgb(140, 140, 140),
                Self::translator().text("skills-mgr-state-none"),
            ),
        }
    }

    fn render_detail_window(&mut self, ctx: &egui::Context) {
        if !self.detail_window_open {
            return;
        }

        let Some(record) = self.detail_record.as_ref() else {
            self.detail_window_open = false;
            return;
        };

        let viewport_height = ctx.input(|input| {
            input
                .viewport()
                .inner_rect
                .map(|rect| rect.height())
                .unwrap_or(760.0)
        });
        let window_max_height = (viewport_height - 96.0).clamp(360.0, 680.0);
        let markdown_height = (window_max_height - 180.0).clamp(180.0, 360.0);

        let t = Self::translator();
        let mut open = self.detail_window_open;
        egui::Window::new(t.text_args(
            "skills-mgr-detail-title",
            HashMap::from([("name", record.name.clone())]),
        ))
        .open(&mut open)
        .resizable(true)
        .default_width(820.0)
        .default_height(window_max_height.min(540.0))
        .max_height(window_max_height)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(t.text_args(
                    "skills-mgr-detail-name",
                    HashMap::from([("name", record.name.clone())]),
                ));
                ui.label(t.text_args(
                    "skills-mgr-detail-source",
                    HashMap::from([(
                        "source",
                        match record.source_kind {
                            SkillSourceKind::Local => t.text("skills-mgr-source-local"),
                            SkillSourceKind::Registry => t.text("skills-mgr-source-registry"),
                        },
                    )]),
                ));
                ui.label(t.text_args(
                    "skills-mgr-detail-registry",
                    HashMap::from([(
                        "registry",
                        record.registry.as_deref().unwrap_or("-").to_string(),
                    )]),
                ));
                let (icon, color, text) = skill_stale_display(record.stale);
                ui.label(
                    RichText::new(t.text_args(
                        "skills-mgr-detail-state",
                        HashMap::from([("icon", icon.to_string()), ("state", text.to_string())]),
                    ))
                    .color(color)
                    .strong(),
                );
            });
            ui.label(t.text_args(
                "skills-mgr-detail-path",
                HashMap::from([("path", record.local_path.display().to_string())]),
            ));
            ui.label(t.text_args(
                "skills-mgr-detail-updated",
                HashMap::from([("time", format_timestamp_millis(record.updated_at_ms))]),
            ));
            ui.separator();

            let mut content = record.content.clone();
            let mut layouter = markdown::text_layouter;
            egui::ScrollArea::vertical()
                .id_salt(("skill-detail-markdown", &record.name))
                .max_height(markdown_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_sized(
                        [ui.available_width(), markdown_height],
                        egui::TextEdit::multiline(&mut content)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace)
                            .interactive(false)
                            .code_editor()
                            .layouter(&mut layouter),
                    );
                });
        });
        self.detail_window_open = open;
        if !self.detail_window_open {
            self.detail_name = None;
            self.detail_record = None;
        }
    }

    fn render_install_window(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(window) = self.install_window.as_mut() else {
            return;
        };

        let t = Self::translator();

        let mut open = true;
        let mut selected_registry = window.selected_registry.clone();
        let registry_names = self
            .config
            .skills
            .registries
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut toggle_action: Option<(String, String, bool)> = None;
        let selected_installed = self
            .config
            .skills
            .registries
            .get(&selected_registry)
            .map(|registry| registry.installed.clone())
            .unwrap_or_default();

        egui::Window::new(t.text("skills-mgr-install-title"))
            .open(&mut open)
            .resizable(true)
            .default_width(900.0)
            .default_height(520.0)
            .max_width(960.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t.text("skills-mgr-install-registry"));
                    egui::ComboBox::from_id_salt("skill-install-registry")
                        .selected_text(if selected_registry.is_empty() {
                            t.text("skills-mgr-install-select-registry")
                        } else {
                            selected_registry.clone()
                        })
                        .show_ui(ui, |ui| {
                            for name in &registry_names {
                                ui.selectable_value(
                                    &mut selected_registry,
                                    name.clone(),
                                    name.as_str(),
                                );
                            }
                        });
                });

                if let Some(error) = window.error.as_ref() {
                    ui.add_space(6.0);
                    ui.label(error);
                }

                ui.separator();
                egui::ScrollArea::both()
                    .id_salt("skill-install-table-scroll")
                    .max_height(320.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("skill-install-grid")
                            .striped(true)
                            .min_col_width(120.0)
                            .show(ui, |ui| {
                                ui.strong(t.text("skills-mgr-install-col-action"));
                                ui.strong(t.text("skills-mgr-install-col-skill"));
                                ui.strong(t.text("skills-mgr-install-col-id"));
                                ui.strong(t.text("skills-mgr-install-col-path"));
                                ui.end_row();

                                for skill in &window.skills {
                                    let skill_selector =
                                        registry_skill_selector(skill, &selected_registry);
                                    let installed = selected_installed
                                        .iter()
                                        .any(|name| name == &skill_selector || name == &skill.name);
                                    if ui
                                        .button(if installed {
                                            t.text("skills-mgr-install-btn-uninstall")
                                        } else {
                                            t.text("skills-mgr-install-btn-install")
                                        })
                                        .clicked()
                                    {
                                        toggle_action = Some((
                                            selected_registry.clone(),
                                            skill_selector,
                                            installed,
                                        ));
                                    }
                                    ui.label(&skill.name);
                                    ui.monospace(&skill.id);
                                    ui.label(skill.local_path.display().to_string());
                                    ui.end_row();
                                }
                            });
                    });
            });

        if !open {
            self.install_window = None;
            return;
        }

        let registry_changed = self
            .install_window
            .as_ref()
            .map(|current| current.selected_registry != selected_registry)
            .unwrap_or(false);
        if registry_changed && let Some(mut current) = self.install_window.take() {
            current.selected_registry = selected_registry.clone();
            self.reload_install_window_catalog(&mut current);
            if let Some(error) = current.error.as_ref() {
                notifications.warning(error.clone());
            }
            self.install_window = Some(current);
        }

        if let Some((registry_name, skill_name, installed)) = toggle_action {
            if installed {
                self.uninstall_registry_skill_from_window(
                    &registry_name,
                    &skill_name,
                    notifications,
                );
            } else {
                self.install_registry_skill(&registry_name, &skill_name, notifications);
            }
        }
    }

    fn render_delete_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some((skill_name, registry)) = self.delete_confirm_skill.clone() else {
            return;
        };

        let t = Self::translator();

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(t.text("skills-mgr-delete-title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .resizable(false)
            .collapsible(false)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.label(t.text_args(
                    "skills-mgr-delete-message",
                    HashMap::from([("name", skill_name.clone())]),
                ));
                if let Some(reg) = registry.as_ref() {
                    ui.label(t.text_args(
                        "skills-mgr-delete-registry",
                        HashMap::from([("registry", reg.clone())]),
                    ));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(
                            RichText::new(t.text_args(
                                "skills-mgr-delete-btn-remove",
                                HashMap::from([("icon", regular::TRASH.to_string())]),
                            ))
                            .color(ui.visuals().warn_fg_color),
                        )
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button(t.text("skills-mgr-delete-cancel")).clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            self.delete_confirm_skill = None;
            self.uninstall_skill(&skill_name, registry.as_deref(), notifications);
        }
        if cancelled {
            self.delete_confirm_skill = None;
        }
    }
}

impl PanelRenderer for SkillsManagerPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);
        self.local_install_dialog.update(ui.ctx());
        self.handle_local_install_selection(notifications);

        let t = Self::translator();

        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            ui.label(t.text_args(
                "skills-mgr-label-installed-count",
                HashMap::from([("count", self.items.len().to_string())]),
            ));
            ui.label(t.text_args(
                "skills-mgr-label-registries-count",
                HashMap::from([("count", self.config.skills.registries.len().to_string())]),
            ));
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(t.text_args(
                    "skills-mgr-btn-refresh",
                    HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.load_items(notifications, true);
            }
            if ui
                .button(t.text_args(
                    "skills-mgr-btn-install",
                    HashMap::from([("icon", regular::DOWNLOAD_SIMPLE.to_string())]),
                ))
                .clicked()
            {
                self.open_install_window(notifications);
            }
            if ui
                .button(t.text_args(
                    "skills-mgr-btn-install-local",
                    HashMap::from([("icon", regular::FOLDER_OPEN.to_string())]),
                ))
                .clicked()
            {
                self.open_local_install_dialog();
            }
        });
        ui.separator();

        if self.items.is_empty() {
            ui.label(t.text("skills-mgr-no-skills"));
        } else {
            let mut view_skill: Option<String> = None;

            let available_height = ui.available_height();
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(100.0))
                .column(Column::auto().at_least(60.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::auto().at_least(60.0))
                .column(Column::auto().at_least(120.0))
                .column(Column::remainder().at_least(150.0))
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height)
                .sense(egui::Sense::click())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong(t.text("skills-mgr-col-name"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-mgr-col-source"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-mgr-col-registry"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-mgr-col-state"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-mgr-col-updated"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-mgr-col-path"));
                    });
                })
                .body(|body| {
                    body.rows(20.0, self.items.len(), |mut row| {
                        let idx = row.index();
                        let item = &self.items[idx];
                        let is_selected = self.selected_skill.as_deref() == Some(&item.name);

                        row.set_selected(is_selected);

                        row.col(|ui| {
                            ui.label(&item.name);
                        });
                        row.col(|ui| {
                            ui.label(Self::source_label(item));
                        });
                        row.col(|ui| {
                            ui.label(item.registry.as_deref().unwrap_or("-"));
                        });
                        row.col(|ui| {
                            let (icon, color, text) = Self::stale_display(item);
                            ui.label(
                                RichText::new(format!("{icon} {text}"))
                                    .color(color)
                                    .strong(),
                            );
                        });
                        row.col(|ui| {
                            ui.monospace(format_timestamp_millis(item.updated_at_ms));
                        });
                        row.col(|ui| {
                            ui.label(item.local_path.display().to_string());
                        });

                        let response = row.response();

                        if response.clicked() {
                            self.selected_skill = if is_selected {
                                None
                            } else {
                                Some(item.name.clone())
                            };
                        }

                        let skill_name = item.name.clone();
                        let skill_registry = item.registry.clone();
                        response.context_menu(|ui| {
                            if ui
                                .button(t.text_args(
                                    "skills-mgr-menu-view",
                                    HashMap::from([("icon", regular::EYE.to_string())]),
                                ))
                                .clicked()
                            {
                                view_skill = Some(skill_name.clone());
                                ui.close();
                            }
                            if ui
                                .button(
                                    RichText::new(t.text_args(
                                        "skills-mgr-menu-remove",
                                        HashMap::from([("icon", regular::TRASH.to_string())]),
                                    ))
                                    .color(ui.visuals().warn_fg_color),
                                )
                                .clicked()
                            {
                                self.delete_confirm_skill = Some((skill_name, skill_registry));
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(t.text_args(
                                    "skills-mgr-menu-copy-name",
                                    HashMap::from([("icon", regular::COPY.to_string())]),
                                ))
                                .clicked()
                            {
                                ui.ctx().output_mut(|o| {
                                    o.commands
                                        .push(egui::OutputCommand::CopyText(item.name.clone()));
                                });
                                ui.close();
                            }
                        });
                    });
                });

            if let Some(skill_name) = view_skill {
                self.load_detail(&skill_name, notifications);
            }
        }
        self.render_delete_confirm_dialog(ui.ctx(), notifications);
        self.render_detail_window(ui.ctx());
        self.render_install_window(ui.ctx(), notifications);
    }
}

fn load_installed_skill_list() -> Result<(PathBuf, Vec<SkillSummary>), String> {
    run_skill_task(|store| async move {
        let root = store.root_dir().to_path_buf();
        let items = store.list_installed().await?;
        Ok((root, items))
    })
}

fn load_installed_skill_detail(skill_name: String) -> Result<SkillRecord, String> {
    run_skill_task(move |store| async move { store.get_installed(&skill_name).await })
}

fn load_source_catalog(registry_name: String) -> Result<Vec<RegistrySkillSummary>, String> {
    run_skill_task(move |store| async move { store.list_source_skills(&registry_name).await })
}

fn registry_skill_selector(skill: &RegistrySkillSummary, registry_name: &str) -> String {
    let selector = skill.id.trim();
    if !selector.is_empty() {
        return selector.to_string();
    }

    let parsed_name = skill.name.trim();
    if !parsed_name.is_empty() {
        return parsed_name.to_string();
    }

    registry_name.trim().to_string()
}

fn install_from_registry_in_store(
    registry_name: String,
    skill_name: String,
) -> Result<(SkillRecord, bool), String> {
    run_skill_task(move |store| async move {
        store
            .install_from_registry(&registry_name, &skill_name)
            .await
    })
}

fn uninstall_installed_skill_from_store(
    skill_name: String,
) -> Result<SkillUninstallResult, String> {
    run_skill_task(move |store| async move { store.uninstall(&skill_name).await })
}

fn uninstall_from_registry_in_store(
    registry_name: String,
    skill_name: String,
) -> Result<(), String> {
    run_skill_task(move |store| async move {
        store
            .uninstall_from_registry(&registry_name, &skill_name)
            .await
    })
}

fn format_uninstall_result(
    skill_name: &str,
    registry: Option<&str>,
    result: &SkillUninstallResult,
) -> String {
    let t = Translator::new(LocaleDomain::Gui, current_ui_language());
    let scope = match registry {
        Some(registry_name) => t.text_args(
            "skills-mgr-notify-uninstall-scope-registry",
            HashMap::from([
                ("skill_name", skill_name.to_string()),
                ("registry_name", registry_name.to_string()),
            ]),
        ),
        None => t.text_args(
            "skills-mgr-notify-uninstall-scope-local",
            HashMap::from([("skill_name", skill_name.to_string())]),
        ),
    };
    t.text_args(
        "skills-mgr-notify-uninstall-result",
        HashMap::from([
            ("scope", scope),
            ("managed", result.removed_managed.to_string()),
            ("local", result.removed_local.to_string()),
        ]),
    )
}

#[derive(Debug)]
struct LocalInstallResult {
    skill_name: String,
    source_dir: PathBuf,
    target_dir: PathBuf,
}

fn install_local_skill_from_markdown_path(
    selected_markdown_path: &Path,
    installed_items: &[SkillSummary],
) -> Result<LocalInstallResult, String> {
    let file_name = selected_markdown_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Selected path is not a valid file path".to_string())?;
    if file_name != "SKILL.md" {
        return Err("Please select a file named SKILL.md".to_string());
    }

    let source_dir = selected_markdown_path
        .parent()
        .ok_or_else(|| "Unable to resolve selected skill directory".to_string())?;
    let content = fs::read_to_string(selected_markdown_path)
        .map_err(|err| format!("Unable to read {}: {err}", selected_markdown_path.display()))?;
    let skill_name = parse_skill_name_from_markdown(&content).ok_or_else(|| {
        "Invalid SKILL.md format: missing skill name (`# <name>` or `name: <name>`)".to_string()
    })?;
    validate_local_skill_name(&skill_name)?;

    if installed_items
        .iter()
        .any(|item| item.name == skill_name && item.source_kind == SkillSourceKind::Registry)
    {
        return Err(format!(
            "Skill `{skill_name}` is already managed by a registry; uninstall it first"
        ));
    }

    let store = open_default_skills_manager()
        .map_err(|err| format!("failed to open default skills manager: {err}"))?;
    let target_dir = store.skills_dir().join(&skill_name);
    let source_dir_canonical = source_dir
        .canonicalize()
        .map_err(|err| format!("Unable to resolve source directory: {err}"))?;
    let target_dir_canonical = target_dir.canonicalize().ok();
    if target_dir_canonical
        .as_ref()
        .is_some_and(|target| target == &source_dir_canonical)
    {
        return Err("Selected SKILL.md is already inside the target skills directory".to_string());
    }

    if target_dir.exists() {
        return Err(format!(
            "Target directory already exists: {}",
            target_dir.display()
        ));
    }

    copy_directory_recursive(&source_dir_canonical, &target_dir).inspect_err(|_| {
        let _ = fs::remove_dir_all(&target_dir);
    })?;

    Ok(LocalInstallResult {
        skill_name,
        source_dir: source_dir_canonical,
        target_dir,
    })
}

fn validate_local_skill_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Skill name cannot be empty".to_string());
    }
    let valid = trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if !valid {
        return Err(format!(
            "Invalid skill name `{trimmed}`; only [a-zA-Z0-9_-] are allowed"
        ));
    }
    Ok(())
}

fn parse_skill_name_from_markdown(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("# ") {
            let value = name.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        if let Some(name) = trimmed.strip_prefix("name:") {
            let value = name.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn copy_directory_recursive(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to)
        .map_err(|err| format!("Unable to create target directory {}: {err}", to.display()))?;
    let entries = fs::read_dir(from)
        .map_err(|err| format!("Unable to read source directory {}: {err}", from.display()))?;

    for entry_result in entries {
        let entry = entry_result.map_err(|err| {
            format!(
                "Unable to enumerate source directory {}: {err}",
                from.display()
            )
        })?;
        let source_path = entry.path();
        let target_path = to.join(entry.file_name());
        let file_type = entry.file_type().map_err(|err| {
            format!(
                "Unable to inspect source entry {}: {err}",
                source_path.display()
            )
        })?;
        if file_type.is_dir() {
            copy_directory_recursive(&source_path, &target_path)?;
            continue;
        }
        if file_type.is_file() {
            fs::copy(&source_path, &target_path).map_err(|err| {
                format!(
                    "Unable to copy {} to {}: {err}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
            continue;
        }
        return Err(format!(
            "Unsupported entry type in local skill directory: {}",
            source_path.display()
        ));
    }
    Ok(())
}

fn skill_stale_display(stale: Option<bool>) -> (&'static str, Color32, String) {
    let t = Translator::new(LocaleDomain::Gui, current_ui_language());
    match stale {
        Some(true) => (
            regular::WARNING,
            Color32::from_rgb(200, 150, 50),
            t.text("skills-mgr-state-stale"),
        ),
        Some(false) => (
            regular::CHECK_CIRCLE,
            Color32::from_rgb(50, 180, 80),
            t.text("skills-mgr-state-fresh"),
        ),
        None => (
            regular::MINUS,
            Color32::from_rgb(140, 140, 140),
            t.text("skills-mgr-state-none"),
        ),
    }
}

fn run_skill_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(FileSystemSkillStore) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, klaw_skill::SkillError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let store = open_default_skills_manager()
            .map_err(|err| format!("failed to open skills manager: {err}"))?;
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to build runtime: {err}"))?;
        runtime
            .block_on(op(store))
            .map_err(|err| format!("skill operation failed: {err}"))
    });

    match join.join() {
        Ok(result) => result,
        Err(_) => Err("skill operation thread panicked".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::SkillsRegistryConfig;
    use std::{
        env, fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn remove_skill_from_config_updates_matching_registry_only() {
        let mut config = AppConfig::default();
        config.skills.registries.insert(
            "private".to_string(),
            SkillsRegistryConfig {
                address: "https://example.com/private.git".to_string(),
                installed: vec!["demo".to_string(), "plan".to_string()],
            },
        );
        config.skills.registries.insert(
            "public".to_string(),
            SkillsRegistryConfig {
                address: "https://example.com/public.git".to_string(),
                installed: vec!["demo".to_string()],
            },
        );

        let (next, changed) =
            SkillsManagerPanel::remove_skill_from_config(config, "private", "demo");

        assert!(changed);
        assert_eq!(next.skills.registries["private"].installed, vec!["plan"]);
        assert_eq!(next.skills.registries["public"].installed, vec!["demo"]);
    }

    #[test]
    fn add_skill_to_config_appends_and_sorts() {
        let mut config = AppConfig::default();
        config.skills.registries.insert(
            "private".to_string(),
            klaw_config::SkillsRegistryConfig {
                address: "https://example.com/private.git".to_string(),
                installed: vec!["zeta".to_string()],
            },
        );

        let (next, changed) = SkillsManagerPanel::add_skill_to_config(config, "private", "alpha");

        assert!(changed);
        assert_eq!(
            next.skills.registries["private"].installed,
            vec!["alpha", "zeta"]
        );
    }

    #[test]
    fn reload_config_snapshot_picks_up_new_registries_from_disk() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("klaw-gui-skills-manager-test-{suffix}"));
        let path = root.join("config.toml");
        fs::create_dir_all(&root).expect("should create temp root");
        fs::write(
            &path,
            toml::to_string_pretty(&AppConfig::default()).expect("default config should serialize"),
        )
        .expect("should write initial config");

        let store = ConfigStore::open(Some(&path)).expect("store should open");
        let mut panel = SkillsManagerPanel {
            config_store: Some(store.clone()),
            ..Default::default()
        };
        panel.apply_snapshot(store.snapshot());
        let mut notifications = NotificationCenter::default();

        let stale_store = ConfigStore::open(Some(&path)).expect("stale store should open");
        stale_store
            .update_config(|config| {
                config.skills.registries.insert(
                    "fresh".to_string(),
                    SkillsRegistryConfig {
                        address: "https://example.com/fresh.git".to_string(),
                        installed: Vec::new(),
                    },
                );
                Ok(())
            })
            .expect("config update should succeed");

        assert!(!panel.config.skills.registries.contains_key("fresh"));
        assert!(panel.reload_config_snapshot(&mut notifications));
        assert!(panel.config.skills.registries.contains_key("fresh"));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_skill_name_from_markdown_accepts_heading_and_name_field() {
        assert_eq!(
            parse_skill_name_from_markdown("# local_test\n\nbody"),
            Some("local_test".to_string())
        );
        assert_eq!(
            parse_skill_name_from_markdown("name: helper_skill\n\n# title"),
            Some("helper_skill".to_string())
        );
    }

    #[test]
    fn registry_skill_selector_prefers_non_empty_id_then_name_then_registry() {
        let skill = RegistrySkillSummary {
            id: "tools/amap".to_string(),
            name: "amap-lbs-skill".to_string(),
            local_path: PathBuf::from("/tmp/SKILL.md"),
        };
        assert_eq!(registry_skill_selector(&skill, "amap"), "tools/amap");

        let skill = RegistrySkillSummary {
            id: String::new(),
            name: "amap-lbs-skill".to_string(),
            local_path: PathBuf::from("/tmp/SKILL.md"),
        };
        assert_eq!(registry_skill_selector(&skill, "amap"), "amap-lbs-skill");

        let skill = RegistrySkillSummary {
            id: " ".to_string(),
            name: " ".to_string(),
            local_path: PathBuf::from("/tmp/SKILL.md"),
        };
        assert_eq!(registry_skill_selector(&skill, "amap"), "amap");
    }

    #[test]
    fn copy_directory_recursive_copies_nested_files() {
        let token = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let source = std::env::temp_dir().join(format!("klaw-gui-local-skill-source-{token}"));
        let target = std::env::temp_dir().join(format!("klaw-gui-local-skill-target-{token}"));
        let nested_source = source.join("assets");
        fs::create_dir_all(&nested_source).expect("create source tree");
        fs::write(source.join("SKILL.md"), "# local_test").expect("write skill");
        fs::write(nested_source.join("a.txt"), "abc").expect("write nested");

        copy_directory_recursive(&source, &target).expect("copy should succeed");

        let copied = fs::read_to_string(target.join("assets").join("a.txt"))
            .expect("copied nested file should exist");
        assert_eq!(copied, "abc");

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(target);
    }
}
