use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore};
use klaw_skill::{
    open_default_skill_store, FileSystemSkillStore, RegistrySkillSummary, SkillRecord,
    SkillSourceKind, SkillStore, SkillSummary, SkillUninstallResult,
};
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

#[derive(Default)]
pub struct SkillManagePanel {
    config_store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    skill_root: Option<PathBuf>,
    loaded: bool,
    items: Vec<SkillSummary>,
    detail_name: Option<String>,
    detail_record: Option<SkillRecord>,
    detail_window_open: bool,
    install_window: Option<InstallSkillWindow>,
}

impl SkillManagePanel {
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
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.config = snapshot.config;
    }

    fn ensure_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.loaded {
            return;
        }
        self.load_items(notifications, false);
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
                    notifications.error(format!("Failed to reload config: {err}"));
                    return;
                }
            }
        } else {
            store.snapshot()
        };
        self.apply_snapshot(snapshot);

        match load_skill_list() {
            Ok((skill_root, items)) => {
                self.skill_root = Some(skill_root);
                self.items = items;
                self.loaded = true;
                if let Some(current_name) = self.detail_name.as_deref() {
                    if !self.items.iter().any(|item| item.name == current_name) {
                        self.detail_name = None;
                        self.detail_record = None;
                        self.detail_window_open = false;
                    }
                }
            }
            Err(err) => notifications.error(format!("Failed to load installed skills: {err}")),
        }
    }

    fn load_detail(&mut self, skill_name: &str, notifications: &mut NotificationCenter) {
        match load_skill_detail(skill_name.to_string()) {
            Ok(record) => {
                self.detail_name = Some(skill_name.to_string());
                self.detail_record = Some(record);
                self.detail_window_open = true;
            }
            Err(err) => {
                self.detail_record = None;
                self.detail_window_open = false;
                notifications.error(format!("Failed to load skill `{skill_name}`: {err}"));
            }
        }
    }

    fn open_install_window(&mut self, notifications: &mut NotificationCenter) {
        if self.config.skills.registries.is_empty() {
            notifications.warning("No skill registry configured");
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

    fn reload_install_window_catalog(&self, window: &mut InstallSkillWindow) {
        if window.selected_registry.is_empty() {
            window.skills.clear();
            window.error = Some("Select a registry first".to_string());
            return;
        }

        match load_registry_catalog(window.selected_registry.clone()) {
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

        let (next_config, changed) =
            Self::add_skill_to_config(self.config.clone(), registry_name, skill_id);
        if !changed {
            notifications.info(format!("`{skill_id}` is already installed"));
            return;
        }

        let raw = match toml::to_string_pretty(&next_config) {
            Ok(raw) => raw,
            Err(err) => {
                notifications.error(format!("Failed to render config TOML: {err}"));
                return;
            }
        };

        match store.save_raw_toml(&raw) {
            Ok(snapshot) => self.apply_snapshot(snapshot),
            Err(err) => {
                notifications.error(format!(
                    "Failed to save installed skill `{skill_id}` to config: {err}"
                ));
                return;
            }
        }

        match install_registry_skill_in_store(registry_name.to_string(), skill_id.to_string()) {
            Ok((_record, _already_installed)) => {
                self.load_items(notifications, false);
                if let Some(mut window) = self.install_window.take() {
                    self.reload_install_window_catalog(&mut window);
                    self.install_window = Some(window);
                }
                notifications.success(format!(
                    "Installed `{skill_id}` from registry `{registry_name}`"
                ));
            }
            Err(err) => {
                notifications.warning(format!(
                    "Saved `{skill_id}` in config, but install did not complete immediately: {err}"
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

        let (next_config, changed) =
            Self::remove_skill_from_config(self.config.clone(), registry_name, skill_id);
        if !changed {
            notifications.info(format!("`{skill_id}` is not installed"));
            return;
        }

        let raw = match toml::to_string_pretty(&next_config) {
            Ok(raw) => raw,
            Err(err) => {
                notifications.error(format!("Failed to render config TOML: {err}"));
                return;
            }
        };

        match store.save_raw_toml(&raw) {
            Ok(snapshot) => self.apply_snapshot(snapshot),
            Err(err) => {
                notifications.error(format!(
                    "Failed to update config while uninstalling `{skill_id}`: {err}"
                ));
                return;
            }
        }

        match uninstall_registry_skill_from_store(registry_name.to_string(), skill_id.to_string()) {
            Ok(()) => {
                if self.detail_name.as_deref() == Some(skill_id) {
                    self.detail_name = None;
                    self.detail_record = None;
                    self.detail_window_open = false;
                }
                self.load_items(notifications, false);
                if let Some(mut window) = self.install_window.take() {
                    self.reload_install_window_catalog(&mut window);
                    self.install_window = Some(window);
                }
                notifications.success(format!(
                    "Uninstalled `{skill_id}` from registry `{registry_name}`"
                ));
            }
            Err(err) => {
                notifications.warning(format!(
                    "Removed `{skill_id}` from config, but registry uninstall did not complete immediately: {err}"
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
            let (next_config, changed) =
                Self::remove_skill_from_config(self.config.clone(), registry_name, skill_name);
            if changed {
                let raw = match toml::to_string_pretty(&next_config) {
                    Ok(raw) => raw,
                    Err(err) => {
                        notifications.error(format!("Failed to render config TOML: {err}"));
                        return;
                    }
                };
                match store.save_raw_toml(&raw) {
                    Ok(snapshot) => {
                        self.apply_snapshot(snapshot);
                        config_updated = true;
                    }
                    Err(err) => {
                        notifications.error(format!(
                            "Failed to remove `{skill_name}` from registry config: {err}"
                        ));
                        return;
                    }
                }
            }
        }

        match uninstall_skill_from_store(skill_name.to_string()) {
            Ok(result) => {
                if self.detail_name.as_deref() == Some(skill_name) {
                    self.detail_name = None;
                    self.detail_record = None;
                    self.detail_window_open = false;
                }
                self.load_items(notifications, false);
                notifications.success(format_uninstall_result(skill_name, registry, &result));
            }
            Err(err) => {
                self.load_items(notifications, false);
                if config_updated {
                    notifications.warning(format!(
                        "Removed `{skill_name}` from config, but local cleanup failed: {err}"
                    ));
                } else {
                    notifications.error(format!("Failed to uninstall `{skill_name}`: {err}"));
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

    fn source_label(summary: &SkillSummary) -> &'static str {
        match summary.source_kind {
            SkillSourceKind::Local => "local",
            SkillSourceKind::Registry => "registry",
        }
    }

    fn stale_label(summary: &SkillSummary) -> &'static str {
        match summary.stale {
            Some(true) => "stale",
            Some(false) => "fresh",
            None => "-",
        }
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Config: {}", path.display()),
            None => "Config: (not loaded)".to_string(),
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

        let mut open = self.detail_window_open;
        egui::Window::new(format!("Skill Detail: {}", record.name))
            .open(&mut open)
            .resizable(true)
            .default_width(820.0)
            .default_height(window_max_height.min(540.0))
            .max_height(window_max_height)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Name: {}", record.name));
                    ui.label(format!(
                        "Source: {}",
                        match record.source_kind {
                            SkillSourceKind::Local => "local",
                            SkillSourceKind::Registry => "registry",
                        }
                    ));
                    ui.label(format!(
                        "Registry: {}",
                        record.registry.as_deref().unwrap_or("-")
                    ));
                    ui.label(format!(
                        "State: {}",
                        match record.stale {
                            Some(true) => "stale",
                            Some(false) => "fresh",
                            None => "-",
                        }
                    ));
                });
                ui.label(format!("Path: {}", record.local_path.display()));
                ui.label(format!("Updated (ms): {}", record.updated_at_ms));
                ui.separator();

                let mut content = record.content.clone();
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
                                .interactive(false),
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

        egui::Window::new("Install Skill")
            .open(&mut open)
            .resizable(true)
            .default_width(900.0)
            .default_height(520.0)
            .max_width(960.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Registry");
                    egui::ComboBox::from_id_salt("skill-install-registry")
                        .selected_text(if selected_registry.is_empty() {
                            "(select registry)"
                        } else {
                            selected_registry.as_str()
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
                                ui.strong("Action");
                                ui.strong("Skill");
                                ui.strong("ID");
                                ui.strong("Path");
                                ui.end_row();

                                for skill in &window.skills {
                                    let installed = selected_installed
                                        .iter()
                                        .any(|name| name == &skill.id || name == &skill.name);
                                    if ui
                                        .button(if installed { "Uninstall" } else { "Install" })
                                        .clicked()
                                    {
                                        toggle_action = Some((
                                            selected_registry.clone(),
                                            skill.id.clone(),
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
        if registry_changed {
            if let Some(mut current) = self.install_window.take() {
                current.selected_registry = selected_registry.clone();
                self.reload_install_window_catalog(&mut current);
                if let Some(error) = current.error.as_ref() {
                    notifications.warning(error.clone());
                }
                self.install_window = Some(current);
            }
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
}

impl PanelRenderer for SkillManagePanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_loaded(notifications);

        let mut select_skill = None;
        let mut remove_skill = None;

        ui.heading(ctx.tab_title);
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.horizontal(|ui| {
            ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
            ui.label(format!("Installed: {}", self.items.len()));
            ui.label(format!(
                "Registries: {}",
                self.config.skills.registries.len()
            ));
        });
        if let Some(skill_root) = self.skill_root.as_ref() {
            ui.label(format!("Skill Root: {}", skill_root.display()));
        }

        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.load_items(notifications, true);
            }
            if ui.button("Install").clicked() {
                self.open_install_window(notifications);
            }
        });
        ui.separator();

        if self.items.is_empty() {
            ui.label("No installed skills found.");
        } else {
            egui::ScrollArea::vertical()
                .id_salt("skill-manage-list")
                .max_height(260.0)
                .show(ui, |ui| {
                    egui::Grid::new("skill-manage-grid")
                        .striped(true)
                        .num_columns(7)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.strong("Name");
                            ui.strong("Source");
                            ui.strong("Registry");
                            ui.strong("State");
                            ui.strong("Updated (ms)");
                            ui.strong("Path");
                            ui.strong("Actions");
                            ui.end_row();

                            for item in &self.items {
                                ui.label(&item.name);
                                ui.label(Self::source_label(&item));
                                ui.label(item.registry.as_deref().unwrap_or("-"));
                                ui.label(Self::stale_label(&item));
                                ui.monospace(item.updated_at_ms.to_string());
                                ui.label(item.local_path.display().to_string());

                                ui.horizontal(|ui| {
                                    if ui.button("View").clicked() {
                                        select_skill = Some(item.name.clone());
                                    }
                                    if ui.button("Remove").clicked() {
                                        remove_skill =
                                            Some((item.name.clone(), item.registry.clone()));
                                    }
                                });
                                ui.end_row();
                            }
                        });
                });
        }

        if let Some(skill_name) = select_skill {
            self.load_detail(&skill_name, notifications);
        }
        if let Some((skill_name, registry)) = remove_skill {
            self.uninstall_skill(&skill_name, registry.as_deref(), notifications);
        }
        self.render_detail_window(ui.ctx());
        self.render_install_window(ui.ctx(), notifications);
    }
}

fn load_skill_list() -> Result<(PathBuf, Vec<SkillSummary>), String> {
    run_skill_task(|store| async move {
        let root = store.root_dir().to_path_buf();
        let items = store.list().await?;
        Ok((root, items))
    })
}

fn load_skill_detail(skill_name: String) -> Result<SkillRecord, String> {
    run_skill_task(move |store| async move { store.get(&skill_name).await })
}

fn load_registry_catalog(registry_name: String) -> Result<Vec<RegistrySkillSummary>, String> {
    run_skill_task(move |store| async move { store.list_registry_skills(&registry_name).await })
}

fn install_registry_skill_in_store(
    registry_name: String,
    skill_name: String,
) -> Result<(SkillRecord, bool), String> {
    run_skill_task(move |store| async move {
        store
            .install_registry_skill(&registry_name, &skill_name)
            .await
    })
}

fn uninstall_skill_from_store(skill_name: String) -> Result<SkillUninstallResult, String> {
    run_skill_task(move |store| async move { store.uninstall_skill(&skill_name).await })
}

fn uninstall_registry_skill_from_store(
    registry_name: String,
    skill_name: String,
) -> Result<(), String> {
    run_skill_task(move |store| async move {
        store
            .uninstall_registry_skill(&registry_name, &skill_name)
            .await
    })
}

fn format_uninstall_result(
    skill_name: &str,
    registry: Option<&str>,
    result: &SkillUninstallResult,
) -> String {
    let scope = match registry {
        Some(registry_name) => format!("registry skill `{skill_name}` from `{registry_name}`"),
        None => format!("local skill `{skill_name}`"),
    };
    format!(
        "Removed {} (managed index: {}, local files: {})",
        scope, result.removed_managed, result.removed_local
    )
}

fn run_skill_task<T, F, Fut>(op: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(FileSystemSkillStore) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, klaw_skill::SkillError>> + Send + 'static,
{
    let join = thread::spawn(move || {
        let store = open_default_skill_store()
            .map_err(|err| format!("failed to open skill store: {err}"))?;
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
    use klaw_config::SkillRegistryConfig;

    #[test]
    fn remove_skill_from_config_updates_matching_registry_only() {
        let mut config = AppConfig::default();
        config.skills.registries.insert(
            "private".to_string(),
            SkillRegistryConfig {
                address: "https://example.com/private.git".to_string(),
                installed: vec!["demo".to_string(), "plan".to_string()],
            },
        );
        config.skills.registries.insert(
            "public".to_string(),
            SkillRegistryConfig {
                address: "https://example.com/public.git".to_string(),
                installed: vec!["demo".to_string()],
            },
        );

        let (next, changed) = SkillManagePanel::remove_skill_from_config(config, "private", "demo");

        assert!(changed);
        assert_eq!(next.skills.registries["private"].installed, vec!["plan"]);
        assert_eq!(next.skills.registries["public"].installed, vec!["demo"]);
    }

    #[test]
    fn add_skill_to_config_appends_and_sorts() {
        let mut config = AppConfig::default();
        config.skills.registries.insert(
            "private".to_string(),
            klaw_config::SkillRegistryConfig {
                address: "https://example.com/private.git".to_string(),
                installed: vec!["zeta".to_string()],
            },
        );

        let (next, changed) = SkillManagePanel::add_skill_to_config(config, "private", "alpha");

        assert!(changed);
        assert_eq!(
            next.skills.registries["private"].installed,
            vec!["alpha", "zeta"]
        );
    }
}
