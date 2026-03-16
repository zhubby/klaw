use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore};
use klaw_skill::{
    open_default_skill_store, FileSystemSkillStore, InstalledSkill, RegistrySource,
    RegistrySyncReport, SkillRecord, SkillSourceKind, SkillStore, SkillSummary,
    SkillUninstallResult,
};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::thread;
use tokio::runtime::Builder;

#[derive(Default)]
pub struct SkillManagePanel {
    config_store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    skill_root: Option<PathBuf>,
    loaded: bool,
    items: Vec<SkillSummary>,
    selected_name: Option<String>,
    selected_record: Option<SkillRecord>,
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

        let requested = self.selected_name.clone();
        match load_skill_list() {
            Ok((skill_root, items)) => {
                self.skill_root = Some(skill_root);
                self.items = items;
                self.loaded = true;
                self.selected_name = Self::choose_selected_name(&self.items, requested.as_deref());
                if let Some(name) = self.selected_name.clone() {
                    self.load_detail(&name, notifications);
                } else {
                    self.selected_record = None;
                }
            }
            Err(err) => notifications.error(format!("Failed to load installed skills: {err}")),
        }
    }

    fn load_detail(&mut self, skill_name: &str, notifications: &mut NotificationCenter) {
        match load_skill_detail(skill_name.to_string()) {
            Ok(record) => {
                self.selected_name = Some(skill_name.to_string());
                self.selected_record = Some(record);
            }
            Err(err) => {
                self.selected_record = None;
                notifications.error(format!("Failed to load skill `{skill_name}`: {err}"));
            }
        }
    }

    fn sync_managed_skills(&mut self, notifications: &mut NotificationCenter) {
        self.ensure_store_loaded(notifications);
        let Some(store) = self.config_store.as_ref() else {
            return;
        };

        let snapshot = match store.reload() {
            Ok(snapshot) => snapshot,
            Err(err) => {
                notifications.error(format!("Failed to reload config: {err}"));
                return;
            }
        };
        self.apply_snapshot(snapshot.clone());

        match sync_skills_from_config(snapshot.config) {
            Ok(report) => {
                self.load_items(notifications, false);
                notifications.success(format_sync_report(&report));
            }
            Err(err) => notifications.error(format!("Managed skill sync failed: {err}")),
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

    fn choose_selected_name(items: &[SkillSummary], requested: Option<&str>) -> Option<String> {
        if let Some(requested_name) = requested {
            if items.iter().any(|item| item.name == requested_name) {
                return Some(requested_name.to_string());
            }
        }
        items.first().map(|item| item.name.clone())
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
            if ui.button("Sync Managed Skills").clicked() {
                self.sync_managed_skills(notifications);
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
                                if self.selected_name.as_deref() == Some(item.name.as_str()) {
                                    ui.strong(&item.name);
                                } else {
                                    ui.label(&item.name);
                                }
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

        ui.separator();
        ui.label("Skill Details");
        if let Some(record) = self.selected_record.as_ref() {
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
            ui.add_space(6.0);

            let mut content = record.content.clone();
            ui.add(
                egui::TextEdit::multiline(&mut content)
                    .desired_rows(18)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .interactive(false),
            );
        } else {
            ui.label("Select a skill to inspect its content.");
        }
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

fn uninstall_skill_from_store(skill_name: String) -> Result<SkillUninstallResult, String> {
    run_skill_task(move |store| async move { store.uninstall_skill(&skill_name).await })
}

fn sync_skills_from_config(config: AppConfig) -> Result<RegistrySyncReport, String> {
    let sources = config
        .skills
        .registries
        .iter()
        .map(|(name, registry)| RegistrySource {
            name: name.clone(),
            address: registry.address.clone(),
        })
        .collect::<Vec<_>>();
    let installed = config
        .skills
        .registries
        .iter()
        .flat_map(|(registry_name, registry)| {
            registry.installed.iter().map(|skill_name| InstalledSkill {
                registry: registry_name.clone(),
                name: skill_name.clone(),
            })
        })
        .collect::<Vec<_>>();
    let timeout = config.skills.sync_timeout;

    run_skill_task(move |store| async move {
        store
            .sync_registry_installed_skills(&sources, &installed, timeout)
            .await
    })
}

fn format_sync_report(report: &RegistrySyncReport) -> String {
    format!(
        "Synced {} registries, added {}, removed {}",
        report.synced_registries.len(),
        report.installed_skills.len(),
        report.removed_skills.len()
    )
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
    fn choose_selected_name_falls_back_to_first_item() {
        let items = vec![
            SkillSummary {
                name: "alpha".to_string(),
                local_path: PathBuf::from("/tmp/alpha/SKILL.md"),
                updated_at_ms: 1,
                source_kind: SkillSourceKind::Local,
                registry: None,
                stale: None,
            },
            SkillSummary {
                name: "beta".to_string(),
                local_path: PathBuf::from("/tmp/beta/SKILL.md"),
                updated_at_ms: 2,
                source_kind: SkillSourceKind::Registry,
                registry: Some("anthropic".to_string()),
                stale: Some(false),
            },
        ];

        let selected = SkillManagePanel::choose_selected_name(&items, Some("missing"));

        assert_eq!(selected.as_deref(), Some("alpha"));
    }
}
