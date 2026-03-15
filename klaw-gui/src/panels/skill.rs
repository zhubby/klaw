use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore, SkillRegistryConfig};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct SkillRegistryForm {
    original_name: Option<String>,
    name: String,
    address: String,
    installed_text: String,
}

impl SkillRegistryForm {
    fn new() -> Self {
        Self {
            original_name: None,
            name: String::new(),
            address: String::new(),
            installed_text: String::new(),
        }
    }

    fn edit(name: &str, registry: &SkillRegistryConfig) -> Self {
        Self {
            original_name: Some(name.to_string()),
            name: name.to_string(),
            address: registry.address.clone(),
            installed_text: registry.installed.join("\n"),
        }
    }

    fn title(&self) -> &'static str {
        if self.original_name.is_some() {
            "Edit Skill Registry"
        } else {
            "Add Skill Registry"
        }
    }

    fn normalized_name(&self) -> String {
        self.name.trim().to_string()
    }

    fn to_config(&self) -> SkillRegistryConfig {
        SkillRegistryConfig {
            address: self.address.trim().to_string(),
            installed: self
                .installed_text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }
}

#[derive(Default)]
pub struct SkillPanel {
    store: Option<ConfigStore>,
    config_path: Option<PathBuf>,
    revision: Option<u64>,
    config: AppConfig,
    form: Option<SkillRegistryForm>,
    sync_timeout_text: String,
}

impl SkillPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                notifications.success("Skill registry config loaded from disk");
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

    fn save_config(
        &mut self,
        next: AppConfig,
        notifications: &mut NotificationCenter,
        success_message: &str,
    ) {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match toml::to_string_pretty(&next) {
            Ok(raw) => match store.save_raw_toml(&raw) {
                Ok(snapshot) => {
                    self.apply_snapshot(snapshot);
                    notifications.success(success_message);
                }
                Err(err) => notifications.error(format!("Save failed: {err}")),
            },
            Err(err) => notifications.error(format!("Failed to render config TOML: {err}")),
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

    fn save_sync_timeout(&mut self, notifications: &mut NotificationCenter) {
        let timeout = match self.sync_timeout_text.trim().parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                notifications.error("skills.sync_timeout must be a positive integer");
                return;
            }
        };

        let mut next = self.config.clone();
        next.skills.sync_timeout = timeout;
        self.save_config(next, notifications, "skills.sync_timeout saved");
    }

    fn open_add_registry(&mut self) {
        self.form = Some(SkillRegistryForm::new());
    }

    fn open_edit_registry(&mut self, name: &str) {
        if let Some(registry) = self.config.skills.registries.get(name) {
            self.form = Some(SkillRegistryForm::edit(name, registry));
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.as_ref() else {
            return;
        };

        match Self::apply_form(self.config.clone(), form) {
            Ok(next) => {
                self.save_config(next, notifications, "Skill registry saved");
                self.form = None;
            }
            Err(err) => notifications.error(err),
        }
    }

    fn apply_form(mut config: AppConfig, form: &SkillRegistryForm) -> Result<AppConfig, String> {
        let name = form.normalized_name();
        if name.is_empty() {
            return Err("Skill registry name cannot be empty".to_string());
        }

        let registry = form.to_config();
        if registry.address.trim().is_empty() {
            return Err("Skill registry address cannot be empty".to_string());
        }

        if let Some(original_name) = form.original_name.as_ref() {
            if original_name != &name {
                if config.skills.registries.contains_key(&name) {
                    return Err(format!(
                        "Skill registry '{}' already exists, choose another name",
                        name
                    ));
                }
                config.skills.registries.remove(original_name);
            }
        } else if config.skills.registries.contains_key(&name) {
            return Err(format!(
                "Skill registry '{}' already exists, choose another name",
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
                ui.label("Installed skills (one per line)");
                ui.add(
                    egui::TextEdit::multiline(&mut form.installed_text)
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );

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
}

impl PanelRenderer for SkillPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.ensure_store_loaded(notifications);

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

        if ui.button("Add Skill Registry").clicked() {
            self.open_add_registry();
        }

        ui.add_space(8.0);

        if self.config.skills.registries.is_empty() {
            ui.label("No skill registries configured.");
        } else {
            egui::Grid::new("skill-registry-list-grid")
                .striped(true)
                .num_columns(5)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.strong("Name");
                    ui.strong("Address");
                    ui.strong("Installed Count");
                    ui.strong("Installed");
                    ui.strong("Actions");
                    ui.end_row();

                    let names = self
                        .config
                        .skills
                        .registries
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>();

                    for name in names {
                        let Some(registry) = self.config.skills.registries.get(&name) else {
                            continue;
                        };

                        ui.label(&name);
                        ui.label(&registry.address);
                        ui.label(registry.installed.len().to_string());
                        ui.label(registry.installed.join(", "));
                        if ui.button("Edit").clicked() {
                            self.open_edit_registry(&name);
                        }
                        ui.end_row();
                    }
                });
        }

        self.render_form_window(ui, notifications);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_form_adds_registry() {
        let config = AppConfig::default();
        let mut form = SkillRegistryForm::new();
        form.name = "private".to_string();
        form.address = "https://example.com/skills".to_string();
        form.installed_text = "one\ntwo".to_string();

        let updated = SkillPanel::apply_form(config, &form).expect("should apply");

        assert!(updated.skills.registries.contains_key("private"));
        assert_eq!(updated.skills.registries["private"].installed.len(), 2);
    }

    #[test]
    fn apply_form_rejects_duplicate_name() {
        let config = AppConfig::default();
        let mut form = SkillRegistryForm::new();
        form.name = "anthropic".to_string();
        form.address = "https://example.com/other".to_string();

        let err = SkillPanel::apply_form(config, &form).expect_err("duplicate should fail");

        assert!(err.contains("already exists"));
    }

    #[test]
    fn apply_form_edits_registry() {
        let mut config = AppConfig::default();
        config.skills.registries.insert(
            "private".to_string(),
            SkillRegistryConfig {
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
        let mut form = SkillRegistryForm::edit("private", &source);
        form.address = "https://example.com/v2".to_string();

        let updated = SkillPanel::apply_form(config, &form).expect("should apply");

        assert_eq!(
            updated.skills.registries["private"].address,
            "https://example.com/v2"
        );
    }
}
