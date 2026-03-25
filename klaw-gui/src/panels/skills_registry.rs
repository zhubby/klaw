use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge;
use crate::widgets::ArrayEditor;
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigError, ConfigSnapshot, ConfigStore, SkillsRegistryConfig};
use klaw_skill::{
    FileSystemSkillStore, InstalledSkill, RegistrySource, open_default_skills_manager,
};
use klaw_skill::{RegistrySyncReport, RegistrySyncStatus};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;
use tokio::runtime::Builder;

const SYNC_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone)]
struct SkillsRegistryForm {
    original_name: Option<String>,
    name: String,
    address: String,
    installed_skills: ArrayEditor,
}

impl SkillsRegistryForm {
    fn new() -> Self {
        Self {
            original_name: None,
            name: String::new(),
            address: String::new(),
            installed_skills: ArrayEditor::new("Installed skills"),
        }
    }

    fn edit(name: &str, registry: &SkillsRegistryConfig) -> Self {
        Self {
            original_name: Some(name.to_string()),
            name: name.to_string(),
            address: registry.address.clone(),
            installed_skills: ArrayEditor::from_vec("Installed skills", &registry.installed),
        }
    }

    fn title(&self) -> &'static str {
        if self.original_name.is_some() {
            "Edit Skills Registry"
        } else {
            "Add Skills Registry"
        }
    }

    fn normalized_name(&self) -> String {
        self.name.trim().to_string()
    }

    fn to_config(&self) -> SkillsRegistryConfig {
        SkillsRegistryConfig {
            address: self.address.trim().to_string(),
            installed: self.installed_skills.to_vec(),
        }
    }
}

#[derive(Default)]
pub struct SkillsRegistryPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<SkillsRegistryForm>,
    sync_timeout_text: String,
    syncing_registry: Option<String>,
    sync_result_rx: Option<Receiver<(String, Result<RegistrySyncReport, String>)>>,
    selected_registry: Option<String>,
    delete_confirm_id: Option<String>,
    registry_statuses: Vec<RegistrySyncStatus>,
}

impl SkillsRegistryPanel {
    fn request_runtime_skills_reload(notifications: &mut NotificationCenter) {
        if let Err(err) = runtime_bridge::request_reload_skills_prompt() {
            notifications.warning(format!("Runtime skills prompt reload not sent: {err}"));
        }
    }

    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                self.load_registry_statuses();
                notifications.success("Skills registry config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config_path = Some(snapshot.path);
        self.revision = Some(snapshot.revision);
        self.sync_timeout_text = snapshot.config.skills.sync_timeout.to_string();
        self.config = snapshot.config;
    }

    fn status_label(path: Option<&Path>) -> String {
        match path {
            Some(path) => format!("Path: {}", path.display()),
            None => "Path: (not loaded)".to_string(),
        }
    }

    fn save_config<F>(
        &mut self,
        notifications: &mut NotificationCenter,
        success_message: &str,
        mutate: F,
    ) -> bool
    where
        F: FnOnce(&mut AppConfig) -> Result<(), String>,
    {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return false;
        };
        match store.update_config(|config| mutate(config).map_err(ConfigError::InvalidConfig)) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                notifications.success(success_message);
                Self::request_runtime_skills_reload(notifications);
                true
            }
            Err(err) => {
                notifications.error(format!("Save failed: {err}"));
                false
            }
        }
    }

    fn reload(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success("Configuration reloaded from disk");
            }
            Err(err) => notifications.error(format!("Reload failed: {err}")),
        }
    }

    fn reload_snapshot_silently(&mut self) -> Result<(), String> {
        let Some(store) = self.store.as_ref() else {
            return Err("Configuration store is not available".to_string());
        };
        let snapshot = store
            .reload()
            .map_err(|err| format!("Reload failed: {err}"))?;
        self.apply_snapshot(snapshot);
        self.load_registry_statuses();
        Ok(())
    }

    fn load_registry_statuses(&mut self) {
        let registry_names: Vec<String> = self.config.skills.registries.keys().cloned().collect();
        if registry_names.is_empty() {
            self.registry_statuses.clear();
            return;
        }
        match run_skill_task(move |store| async move {
            store.get_registry_statuses(&registry_names).await
        }) {
            Ok(statuses) => self.registry_statuses = statuses,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to load registry statuses");
            }
        }
    }

    fn save_sync_timeout(&mut self, notifications: &mut NotificationCenter) {
        let timeout = match self.sync_timeout_text.trim().parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                notifications.error("skills.sync_timeout must be a positive integer");
                return;
            }
        };

        self.save_config(notifications, "skills.sync_timeout saved", move |config| {
            config.skills.sync_timeout = timeout;
            Ok(())
        });
    }

    fn sync_registry(&mut self, registry_name: &str, notifications: &mut NotificationCenter) {
        if self.syncing_registry.is_some() {
            notifications.warning("A skills registry sync is already running");
            return;
        }

        let Some(registry) = self.config.skills.registries.get(registry_name) else {
            notifications.error(format!("Skills registry `{registry_name}` not found"));
            return;
        };

        let timeout = match self.sync_timeout_text.trim().parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                notifications.error("skills.sync_timeout must be a positive integer");
                return;
            }
        };

        let source = RegistrySource {
            name: registry_name.to_string(),
            address: registry.address.clone(),
        };
        let installed = registry
            .installed
            .iter()
            .map(|skill_name| InstalledSkill {
                registry: registry_name.to_string(),
                name: skill_name.clone(),
            })
            .collect::<Vec<_>>();

        let registry_name = registry_name.to_string();
        let status_registry_name = registry_name.clone();
        let (tx, rx) = mpsc::channel();
        self.syncing_registry = Some(registry_name.clone());
        self.sync_result_rx = Some(rx);
        thread::spawn(move || {
            let result = run_skill_sync_task(source, installed, timeout);
            let _ = tx.send((registry_name, result));
        });
        notifications.info(format!(
            "Started syncing registry `{}`",
            status_registry_name
        ));
    }

    fn poll_sync_result(&mut self, notifications: &mut NotificationCenter) {
        let Some(rx) = self.sync_result_rx.as_ref() else {
            return;
        };

        match rx.try_recv() {
            Ok((registry_name, result)) => {
                self.sync_result_rx = None;
                self.syncing_registry = None;
                match result {
                    Ok(report) => {
                        if let Err(err) = self.reload_snapshot_silently() {
                            notifications.warning(err);
                        }
                        Self::request_runtime_skills_reload(notifications);
                        notifications.success(format!(
                            "Registry `{registry_name}` synced: added {}, removed {}",
                            report.installed_skills.len(),
                            report.removed_skills.len()
                        ));
                    }
                    Err(err) => {
                        notifications
                            .error(format!("Failed to sync registry `{registry_name}`: {err}"));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sync_result_rx = None;
                self.syncing_registry = None;
                notifications.error("Skill registry sync worker disconnected");
            }
        }
    }

    fn open_add_registry(&mut self) {
        self.form = Some(SkillsRegistryForm::new());
    }

    fn open_edit_registry(&mut self, name: &str) {
        if let Some(registry) = self.config.skills.registries.get(name) {
            self.form = Some(SkillsRegistryForm::edit(name, registry));
        }
    }

    fn delete_registry(&mut self, name: &str, notifications: &mut NotificationCenter) {
        if !self.config.skills.registries.contains_key(name) {
            notifications.error(format!("Skills registry `{name}` not found"));
            return;
        }

        let name = name.to_string();
        let name_for_config = name.clone();
        if self.save_config(
            notifications,
            &format!("Skills registry `{name}` deleted"),
            move |config| {
                config.skills.registries.remove(&name_for_config);
                Ok(())
            },
        ) {
            self.selected_registry = None;
            self.cleanup_registry_manifest(&name, notifications);
        }
    }

    fn cleanup_registry_manifest(
        &mut self,
        registry_name: &str,
        notifications: &mut NotificationCenter,
    ) {
        let registry_name = registry_name.to_string();
        match run_skill_task(
            move |store| async move { store.cleanup_registry(&registry_name).await },
        ) {
            Ok(count) => {
                if count > 0 {
                    notifications.info(format!("Cleaned {count} installed skills from manifest"));
                }
            }
            Err(err) => {
                notifications.warning(format!("Failed to cleanup registry manifest: {err}"))
            }
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.clone() else {
            return;
        };

        if self.save_config(notifications, "Skills registry saved", move |config| {
            let next = Self::apply_form(config.clone(), &form)?;
            *config = next;
            Ok(())
        }) {
            self.form = None;
        }
    }

    fn apply_form(mut config: AppConfig, form: &SkillsRegistryForm) -> Result<AppConfig, String> {
        let name = form.normalized_name();
        if name.is_empty() {
            return Err("Skills registry name cannot be empty".to_string());
        }

        let registry = form.to_config();
        if registry.address.trim().is_empty() {
            return Err("Skills registry address cannot be empty".to_string());
        }

        if let Some(original_name) = form.original_name.as_ref() {
            if original_name != &name {
                if config.skills.registries.contains_key(&name) {
                    return Err(format!(
                        "Skills registry '{}' already exists, choose another name",
                        name
                    ));
                }
                config.skills.registries.remove(original_name);
            }
        } else if config.skills.registries.contains_key(&name) {
            return Err(format!(
                "Skills registry '{}' already exists, choose another name",
                name
            ));
        }

        config.skills.registries.insert(name, registry);
        Ok(config)
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        let Some(form) = self.form.as_mut() else {
            return;
        };

        egui::Window::new(form.title())
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_width(520.0);
                egui::Grid::new("skill-registry-form-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut form.name);
                        ui.end_row();

                        ui.label("Address");
                        ui.text_edit_singleline(&mut form.address);
                        ui.end_row();
                    });

                ui.separator();
                form.installed_skills.show(ui);

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save_clicked = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if save_clicked {
            self.save_form(notifications);
        }
        if cancel_clicked {
            self.form = None;
        }
    }

    fn render_delete_confirm_dialog(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        let Some(registry_name) = self.delete_confirm_id.clone() else {
            return;
        };

        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Delete Skills Registry")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Are you sure you want to delete registry '{}'?",
                    registry_name
                ));
                ui.label("This will remove the registry from configuration and clean up installed skills from the manifest.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(
                            RichText::new("Delete").color(ui.visuals().warn_fg_color),
                        )
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            self.delete_registry(&registry_name, notifications);
            self.delete_confirm_id = None;
        }
        if cancelled {
            self.delete_confirm_id = None;
        }
    }
}

impl PanelRenderer for SkillsRegistryPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);
        self.poll_sync_result(notifications);
        if self.sync_result_rx.is_some() {
            ui.ctx().request_repaint_after(SYNC_POLL_INTERVAL);
        }

        ui.heading(ctx.tab_title);
        ui.label(Self::status_label(self.config_path.as_deref()));
        ui.horizontal(|ui| {
            ui.label(format!("Revision: {}", self.revision.unwrap_or_default()));
            ui.label(format!(
                "Registries: {}",
                self.config.skills.registries.len()
            ));
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("sync_timeout (seconds)");
            ui.add(egui::TextEdit::singleline(&mut self.sync_timeout_text).desired_width(120.0));
            if ui.button("Save Timeout").clicked() {
                self.save_sync_timeout(notifications);
            }
            if ui.button("Reload").clicked() {
                self.reload(notifications);
            }
        });

        ui.add_space(8.0);

        if ui.button("Add Skills Registry").clicked() {
            self.open_add_registry();
        }

        ui.add_space(8.0);

        if self.config.skills.registries.is_empty() {
            ui.label("No skill registries configured.");
        } else {
            let mut edit_registry_name: Option<String> = None;
            let mut sync_registry_name: Option<String> = None;
            let mut delete_registry_name: Option<String> = None;

            let registry_names = self
                .config
                .skills
                .registries
                .keys()
                .cloned()
                .collect::<Vec<_>>();

            let available_height = ui.available_height();
            TableBuilder::new(ui)
                .striped(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(100.0))
                .column(Column::auto().at_least(240.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::auto().at_least(100.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::remainder().at_least(120.0))
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height)
                .sense(egui::Sense::click())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Name");
                    });
                    header.col(|ui| {
                        ui.strong("Address");
                    });
                    header.col(|ui| {
                        ui.strong("Synced");
                    });
                    header.col(|ui| {
                        ui.strong("Commit");
                    });
                    header.col(|ui| {
                        ui.strong("Installed");
                    });
                    header.col(|ui| {
                        ui.strong("Skills");
                    });
                })
                .body(|body| {
                    body.rows(20.0, registry_names.len(), |mut row| {
                        let idx = row.index();
                        let name = &registry_names[idx];
                        let Some(registry) = self.config.skills.registries.get(name) else {
                            return;
                        };

                        let is_selected = self.selected_registry.as_deref() == Some(name);
                        row.set_selected(is_selected);

                        let is_syncing = self.syncing_registry.as_deref() == Some(name.as_str());

                        let status = self
                            .registry_statuses
                            .iter()
                            .find(|s| s.registry_name == *name);

                        row.col(|ui| {
                            ui.label(name);
                        });
                        row.col(|ui| {
                            ui.label(&registry.address);
                        });
                        row.col(|ui| {
                            if is_syncing {
                                ui.add(egui::Spinner::new().size(14.0));
                            } else if let Some(s) = &status {
                                if s.is_stale {
                                    ui.label(
                                        RichText::new(format!("{} Outdated", regular::WARNING))
                                            .color(ui.visuals().warn_fg_color),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new(format!("{} Synced", regular::CHECK))
                                            .color(egui::Color32::from_rgb(0x22, 0xC5, 0x5E)),
                                    );
                                }
                            } else {
                                ui.label(RichText::new("-").weak());
                            }
                        });
                        row.col(|ui| {
                            if let Some(s) = &status {
                                if let Some(commit) = &s.commit {
                                    let short = if commit.len() > 8 {
                                        &commit[..8]
                                    } else {
                                        commit.as_str()
                                    };
                                    ui.label(short);
                                } else {
                                    ui.label(RichText::new("-").weak());
                                }
                            } else {
                                ui.label(RichText::new("-").weak());
                            }
                        });
                        row.col(|ui| {
                            ui.label(registry.installed.len().to_string());
                        });
                        row.col(|ui| {
                            ui.label(registry.installed.join(", "));
                        });

                        let response = row.response();

                        if response.clicked() {
                            self.selected_registry = if is_selected {
                                None
                            } else {
                                Some(name.clone())
                            };
                        }

                        let name_clone = name.clone();
                        response.context_menu(|ui| {
                            if ui
                                .add_enabled(
                                    !is_syncing,
                                    egui::Button::new(format!("{} Sync", regular::ARROW_CLOCKWISE)),
                                )
                                .clicked()
                            {
                                sync_registry_name = Some(name_clone.clone());
                                ui.close();
                            }
                            if ui
                                .button(format!("{} Edit", regular::PENCIL_SIMPLE))
                                .clicked()
                            {
                                edit_registry_name = Some(name_clone.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(format!("{} Copy Name", regular::COPY)).clicked() {
                                ui.ctx().output_mut(|o| {
                                    o.commands.push(egui::OutputCommand::CopyText(name.clone()));
                                });
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(
                                    RichText::new(format!("{} Delete", regular::TRASH))
                                        .color(ui.visuals().warn_fg_color),
                                )
                                .clicked()
                            {
                                delete_registry_name = Some(name_clone);
                                ui.close();
                            }
                        });
                    });
                });

            if let Some(name) = edit_registry_name {
                self.open_edit_registry(&name);
            }
            if let Some(name) = sync_registry_name {
                self.sync_registry(&name, notifications);
            }
            if let Some(name) = delete_registry_name {
                self.delete_confirm_id = Some(name);
            }
        }

        self.render_delete_confirm_dialog(ui.ctx(), notifications);
        self.render_form_window(ui, notifications);
    }
}

fn run_skill_sync_task(
    source: RegistrySource,
    installed: Vec<InstalledSkill>,
    timeout: u64,
) -> Result<klaw_skill::RegistrySyncReport, String> {
    run_skill_task(move |store| async move {
        store
            .sync_registry_installed_skills(&[source], &installed, timeout)
            .await
    })
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

    #[test]
    fn apply_form_adds_registry() {
        let config = AppConfig::default();
        let mut form = SkillsRegistryForm::new();
        form.name = "private".to_string();
        form.address = "https://example.com/skills".to_string();
        form.installed_skills =
            ArrayEditor::from_vec("Installed skills", &["one".to_string(), "two".to_string()]);

        let updated = SkillsRegistryPanel::apply_form(config, &form).expect("should apply");

        assert!(updated.skills.registries.contains_key("private"));
        assert_eq!(updated.skills.registries["private"].installed.len(), 2);
    }

    #[test]
    fn apply_form_rejects_duplicate_name() {
        let config = AppConfig::default();
        let mut form = SkillsRegistryForm::new();
        form.name = "anthropic".to_string();
        form.address = "https://example.com/other".to_string();

        let err =
            SkillsRegistryPanel::apply_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_form_edits_registry() {
        let mut config = AppConfig::default();
        config.skills.registries.insert(
            "private".to_string(),
            SkillsRegistryConfig {
                address: "https://example.com/v1".to_string(),
                installed: vec!["old".to_string()],
            },
        );

        let source = config
            .skills
            .registries
            .get("private")
            .expect("registry should exist")
            .clone();
        let mut form = SkillsRegistryForm::edit("private", &source);
        form.address = "https://example.com/v2".to_string();

        let updated = SkillsRegistryPanel::apply_form(config, &form).expect("should apply");

        assert_eq!(
            updated.skills.registries["private"].address,
            "https://example.com/v2"
        );
    }
}
