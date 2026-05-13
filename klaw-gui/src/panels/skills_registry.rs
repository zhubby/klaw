use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use crate::runtime_bridge;
use crate::settings::current_ui_language;
use crate::widgets::ArrayEditor;
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigError, ConfigSnapshot, ConfigStore, SkillsRegistryConfig};
use klaw_skill::{
    FileSystemSkillStore, InstalledSkill, RegistrySource, open_default_skills_manager,
};
use klaw_skill::{RegistrySyncReport, RegistrySyncStatus};
use klaw_ui_kit::{LocaleDomain, Translator};
use std::collections::HashMap;
use std::future::Future;
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

    fn title(&self) -> String {
        let t = Translator::new(LocaleDomain::Gui, current_ui_language());
        if self.original_name.is_some() {
            t.text("skills-reg-form-title-edit")
        } else {
            t.text("skills-reg-form-title-add")
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
    config: AppConfig,
    form: Option<SkillsRegistryForm>,
    config_window_open: bool,
    sync_timeout_text: String,
    syncing_registry: Option<String>,
    sync_result_rx: Option<Receiver<(String, Result<RegistrySyncReport, String>)>>,
    selected_registry: Option<String>,
    delete_confirm_id: Option<String>,
    registry_statuses: Vec<RegistrySyncStatus>,
}

impl SkillsRegistryPanel {
    fn translator() -> Translator {
        Translator::new(LocaleDomain::Gui, current_ui_language())
    }

    fn request_runtime_skills_reload(notifications: &mut NotificationCenter) {
        if let Err(err) = runtime_bridge::request_reload_skills_prompt() {
            notifications.warning(Self::translator().text_args(
                "skills-reg-notify-reload-not-sent",
                HashMap::from([("error", err.to_string())]),
            ));
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
                notifications.success(Self::translator().text("skills-reg-notify-config-loaded"));
            }
            Err(err) => notifications.error(Self::translator().text_args(
                "skills-reg-notify-load-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.sync_timeout_text = snapshot.config.skills.sync_timeout.to_string();
        self.config = snapshot.config;
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
            notifications.error(Self::translator().text("skills-reg-notify-store-unavailable"));
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
                notifications.error(Self::translator().text_args(
                    "skills-reg-notify-save-failed",
                    HashMap::from([("error", err.to_string())]),
                ));
                false
            }
        }
    }

    fn reload(&mut self, notifications: &mut NotificationCenter) {
        let Some(store) = self.store.as_ref() else {
            notifications.error(Self::translator().text("skills-reg-notify-store-unavailable"));
            return;
        };
        match store.reload() {
            Ok(snapshot) => {
                self.apply_snapshot(snapshot);
                notifications.success(Self::translator().text("skills-reg-notify-config-reloaded"));
            }
            Err(err) => notifications.error(Self::translator().text_args(
                "skills-reg-notify-reload-failed",
                HashMap::from([("error", err.to_string())]),
            )),
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

    fn save_sync_timeout(&mut self, notifications: &mut NotificationCenter) -> bool {
        let timeout = match self.sync_timeout_text.trim().parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                notifications
                    .error(Self::translator().text("skills-reg-error-sync-timeout-invalid"));
                return false;
            }
        };

        self.save_config(
            notifications,
            &Self::translator().text("skills-reg-notify-sync-timeout-saved"),
            move |config| {
                config.skills.sync_timeout = timeout;
                Ok(())
            },
        )
    }

    fn render_config_window(
        &mut self,
        ctx: &egui::Context,
        notifications: &mut NotificationCenter,
    ) {
        if !self.config_window_open {
            return;
        }

        let t = Self::translator();
        let mut save_clicked = false;
        let mut cancel_clicked = false;

        egui::Window::new(t.text("skills-reg-config-title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                egui::Grid::new("skills-registry-config-grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(t.text("skills-reg-config-sync-timeout"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.sync_timeout_text)
                                .desired_width(120.0),
                        );
                        ui.end_row();
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t.text("skills-reg-config-save")).clicked() {
                        save_clicked = true;
                    }
                    if ui.button(t.text("skills-reg-config-cancel")).clicked() {
                        cancel_clicked = true;
                    }
                });
            });

        if save_clicked && self.save_sync_timeout(notifications) {
            self.config_window_open = false;
        }
        if cancel_clicked {
            self.sync_timeout_text = self.config.skills.sync_timeout.to_string();
            self.config_window_open = false;
        }
    }

    fn sync_registry(&mut self, registry_name: &str, notifications: &mut NotificationCenter) {
        if self.syncing_registry.is_some() {
            notifications
                .warning(Self::translator().text("skills-reg-notify-sync-already-running"));
            return;
        }

        let Some(registry) = self.config.skills.registries.get(registry_name) else {
            notifications.error(Self::translator().text_args(
                "skills-reg-notify-registry-not-found",
                HashMap::from([("registry_name", registry_name.to_string())]),
            ));
            return;
        };

        let timeout = match self.sync_timeout_text.trim().parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                notifications
                    .error(Self::translator().text("skills-reg-error-sync-timeout-invalid"));
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
        notifications.info(Self::translator().text_args(
            "skills-reg-notify-sync-started",
            HashMap::from([("registry_name", status_registry_name)]),
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
                        notifications.success(Self::translator().text_args(
                            "skills-reg-notify-sync-success",
                            HashMap::from([
                                ("registry_name", registry_name.clone()),
                                ("added", report.installed_skills.len().to_string()),
                                ("removed", report.removed_skills.len().to_string()),
                            ]),
                        ));
                    }
                    Err(err) => {
                        notifications.error(Self::translator().text_args(
                            "skills-reg-notify-sync-failed",
                            HashMap::from([
                                ("registry_name", registry_name.clone()),
                                ("error", err.to_string()),
                            ]),
                        ));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sync_result_rx = None;
                self.syncing_registry = None;
                notifications.error(Self::translator().text("skills-reg-notify-sync-disconnected"));
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
            notifications.error(Self::translator().text_args(
                "skills-reg-notify-registry-not-found",
                HashMap::from([("registry_name", name.to_string())]),
            ));
            return;
        }

        let name = name.to_string();
        let name_for_config = name.clone();
        if self.save_config(
            notifications,
            &Self::translator().text_args(
                "skills-reg-notify-registry-deleted",
                HashMap::from([("registry_name", name.clone())]),
            ),
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
                    notifications.info(Self::translator().text_args(
                        "skills-reg-notify-cleaned-skills",
                        HashMap::from([("count", count.to_string())]),
                    ));
                }
            }
            Err(err) => notifications.warning(Self::translator().text_args(
                "skills-reg-notify-cleanup-failed",
                HashMap::from([("error", err.to_string())]),
            )),
        }
    }

    fn save_form(&mut self, notifications: &mut NotificationCenter) {
        let Some(form) = self.form.clone() else {
            return;
        };

        if self.save_config(
            notifications,
            &Self::translator().text("skills-reg-notify-registry-saved"),
            move |config| {
                let next = Self::apply_form(config.clone(), &form)?;
                *config = next;
                Ok(())
            },
        ) {
            self.form = None;
        }
    }

    fn apply_form(mut config: AppConfig, form: &SkillsRegistryForm) -> Result<AppConfig, String> {
        let t = Self::translator();
        let name = form.normalized_name();
        if name.is_empty() {
            return Err(t.text("skills-reg-error-name-empty"));
        }

        let registry = form.to_config();
        if registry.address.trim().is_empty() {
            return Err(t.text("skills-reg-error-address-empty"));
        }

        if let Some(original_name) = form.original_name.as_ref() {
            if original_name != &name {
                if config.skills.registries.contains_key(&name) {
                    return Err(t.text_args(
                        "skills-reg-error-name-duplicate",
                        HashMap::from([("name", name.clone())]),
                    ));
                }
                config.skills.registries.remove(original_name);
            }
        } else if config.skills.registries.contains_key(&name) {
            return Err(t.text_args(
                "skills-reg-error-name-duplicate",
                HashMap::from([("name", name.clone())]),
            ));
        }

        config.skills.registries.insert(name, registry);
        Ok(config)
    }

    fn render_form_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let t = Self::translator();
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
                        ui.label(t.text("skills-reg-form-label-name"));
                        ui.text_edit_singleline(&mut form.name);
                        ui.end_row();

                        ui.label(t.text("skills-reg-form-label-address"));
                        ui.text_edit_singleline(&mut form.address);
                        ui.end_row();
                    });

                ui.separator();
                form.installed_skills.show(ui);

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t.text("skills-reg-form-btn-save")).clicked() {
                        save_clicked = true;
                    }
                    if ui.button(t.text("skills-reg-form-btn-cancel")).clicked() {
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

        let t = Self::translator();
        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new(t.text("skills-reg-delete-title"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(t.text_args(
                    "skills-reg-delete-message",
                    HashMap::from([("registry_name", registry_name.clone())]),
                ));
                ui.label(t.text("skills-reg-delete-description"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(
                            RichText::new(t.text_args(
                                "skills-reg-delete-btn",
                                HashMap::from([("icon", regular::TRASH.to_string())]),
                            ))
                            .color(ui.visuals().warn_fg_color),
                        )
                        .clicked()
                    {
                        confirmed = true;
                    }
                    if ui.button(t.text("skills-reg-delete-cancel")).clicked() {
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

        let t = Self::translator();
        ui.heading(ctx.tab_title);
        ui.horizontal(|ui| {
            ui.label(t.text_args(
                "skills-reg-label-registries-count",
                HashMap::from([("count", self.config.skills.registries.len().to_string())]),
            ));
        });
        ui.separator();

        ui.horizontal(|ui| {
            if ui
                .button(t.text_args(
                    "skills-reg-btn-config",
                    HashMap::from([("icon", regular::GEAR.to_string())]),
                ))
                .clicked()
            {
                self.sync_timeout_text = self.config.skills.sync_timeout.to_string();
                self.config_window_open = true;
            }
            if ui
                .button(t.text_args(
                    "skills-reg-btn-reload",
                    HashMap::from([("icon", regular::ARROWS_CLOCKWISE.to_string())]),
                ))
                .clicked()
            {
                self.reload(notifications);
            }
            if ui
                .button(t.text_args(
                    "skills-reg-btn-add",
                    HashMap::from([("icon", regular::PLUS.to_string())]),
                ))
                .clicked()
            {
                self.open_add_registry();
            }
        });

        ui.add_space(8.0);

        if self.config.skills.registries.is_empty() {
            ui.label(t.text("skills-reg-no-registries"));
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
                .min_scrolled_height(0.0)
                .max_scroll_height(available_height)
                .sense(egui::Sense::click())
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong(t.text("skills-reg-col-name"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-reg-col-address"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-reg-col-synced"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-reg-col-commit"));
                    });
                    header.col(|ui| {
                        ui.strong(t.text("skills-reg-col-installed"));
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
                                        RichText::new(t.text_args(
                                            "skills-reg-status-outdated",
                                            HashMap::from([("icon", regular::WARNING.to_string())]),
                                        ))
                                        .color(ui.visuals().warn_fg_color),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new(t.text_args(
                                            "skills-reg-status-synced",
                                            HashMap::from([("icon", regular::CHECK.to_string())]),
                                        ))
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
                                    egui::Button::new(t.text_args(
                                        "skills-reg-ctx-sync",
                                        HashMap::from([(
                                            "icon",
                                            regular::ARROW_CLOCKWISE.to_string(),
                                        )]),
                                    )),
                                )
                                .clicked()
                            {
                                sync_registry_name = Some(name_clone.clone());
                                ui.close();
                            }
                            if ui
                                .button(t.text_args(
                                    "skills-reg-ctx-edit",
                                    HashMap::from([("icon", regular::PENCIL_SIMPLE.to_string())]),
                                ))
                                .clicked()
                            {
                                edit_registry_name = Some(name_clone.clone());
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(t.text_args(
                                    "skills-reg-ctx-copy-name",
                                    HashMap::from([("icon", regular::COPY.to_string())]),
                                ))
                                .clicked()
                            {
                                ui.ctx().output_mut(|o| {
                                    o.commands.push(egui::OutputCommand::CopyText(name.clone()));
                                });
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .button(
                                    RichText::new(t.text_args(
                                        "skills-reg-ctx-delete",
                                        HashMap::from([("icon", regular::TRASH.to_string())]),
                                    ))
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
        self.render_config_window(ui.ctx(), notifications);
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
