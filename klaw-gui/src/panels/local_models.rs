use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore};
use klaw_model::{
    DownloadProgress, ModelInstallRequest, ModelInstallResult, ModelService, ModelSummary,
    ModelUsageBinding,
};
use klaw_util::{default_data_dir, models_dir};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use tokio::runtime::Builder;
use tokio_util::sync::CancellationToken;

enum ModelTaskMessage {
    Refreshed(Result<Vec<ModelSummary>, String>),
    Progress(DownloadProgress),
    Installed(Result<ModelInstallResult, String>),
    Removed(Result<String, String>),
}

#[derive(Debug, Clone)]
struct InstallForm {
    repo_id: String,
    revision: String,
}

impl Default for InstallForm {
    fn default() -> Self {
        Self {
            repo_id: String::new(),
            revision: "main".to_string(),
        }
    }
}

impl InstallForm {
    fn to_request(&self) -> Result<ModelInstallRequest, String> {
        let repo_id = self.repo_id.trim();
        if repo_id.is_empty() {
            return Err("repo_id cannot be empty".to_string());
        }
        let revision = self.revision.trim();
        if revision.is_empty() {
            return Err("revision cannot be empty".to_string());
        }
        Ok(ModelInstallRequest {
            repo_id: repo_id.to_string(),
            revision: revision.to_string(),
            quantization: None,
        })
    }
}

#[derive(Default)]
pub struct LocalModelsPanel {
    store: Option<ConfigStore>,
    config: AppConfig,
    installed: Vec<ModelSummary>,
    task_rx: Option<Receiver<ModelTaskMessage>>,
    install_form: InstallForm,
    install_window_open: bool,
    install_cancel: Option<CancellationToken>,
    selected_model: Option<String>,
    delete_confirm: Option<String>,
    progress: Option<DownloadProgress>,
    progress_by_file: BTreeMap<String, DownloadProgress>,
}

impl PanelRenderer for LocalModelsPanel {
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &RenderCtx<'_>,
        notifications: &mut NotificationCenter,
    ) {
        self.poll_tasks(notifications);
        self.ensure_store_loaded(notifications);
        ui.heading(ctx.tab_title);
        ui.label("Manage local Hugging Face model assets under .klaw/models.");
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.begin_refresh();
            }
            if ui.button("Install Model").clicked() {
                self.install_window_open = true;
            }
            if ui.button("Open Models Directory").clicked() {
                if let Err(err) = open_path_in_os(&self.models_dir_path()) {
                    notifications.error(format!("Failed to open models directory: {err}"));
                }
            }
        });

        ui.separator();
        self.render_installed_models(ui, notifications);
        self.render_install_window(ui, notifications);
        self.render_install_progress_window(ui, notifications);
        self.render_delete_confirm(ui, notifications);
    }
}

impl LocalModelsPanel {
    fn ensure_store_loaded(&mut self, notifications: &mut NotificationCenter) {
        if self.store.is_some() {
            return;
        }
        match ConfigStore::open(None) {
            Ok(store) => {
                let snapshot = store.snapshot();
                self.store = Some(store);
                self.apply_snapshot(snapshot);
                self.begin_refresh();
                notifications.success("Local model config loaded from disk");
            }
            Err(err) => notifications.error(format!("Failed to load config: {err}")),
        }
    }

    fn apply_snapshot(&mut self, snapshot: ConfigSnapshot) {
        self.config = snapshot.config;
    }

    fn models_dir_path(&self) -> PathBuf {
        if let Some(root_dir) = self.config.models.root_dir.as_ref() {
            PathBuf::from(root_dir)
        } else if let Some(root_dir) = self.config.storage.root_dir.as_ref() {
            models_dir(root_dir)
        } else {
            models_dir(default_data_dir().unwrap_or_else(|| PathBuf::from(".klaw")))
        }
    }

    fn begin_refresh(&mut self) {
        let config = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.task_rx = Some(rx);
        thread::spawn(move || {
            let result = ModelService::open_default(&config)
                .and_then(|service| service.list_installed())
                .map_err(|err| err.to_string());
            let _ = tx.send(ModelTaskMessage::Refreshed(result));
        });
    }

    fn begin_install(&mut self, request: ModelInstallRequest) {
        if self.install_cancel.is_some() {
            return;
        }
        let config = self.config.clone();
        let (tx, rx) = mpsc::channel();
        let cancellation = CancellationToken::new();
        self.task_rx = Some(rx);
        self.progress = None;
        self.progress_by_file.clear();
        self.install_cancel = Some(cancellation.clone());
        thread::spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build");
            let tx_progress = tx.clone();
            let result = runtime.block_on(async move {
                let service = ModelService::open_default(&config).map_err(|err| err.to_string())?;
                service
                    .install_model(request, cancellation, move |progress| {
                        let _ = tx_progress.send(ModelTaskMessage::Progress(progress));
                    })
                    .await
                    .map_err(|err| err.to_string())
            });
            let _ = tx.send(ModelTaskMessage::Installed(result));
        });
    }

    fn begin_upgrade(&mut self, summary: &ModelSummary, notifications: &mut NotificationCenter) {
        if self.install_cancel.is_some() {
            notifications.error("Another model download is already running");
            return;
        }
        self.begin_install(ModelInstallRequest {
            repo_id: summary.repo_id.clone(),
            revision: summary.revision.clone(),
            quantization: None,
        });
        notifications.info(format!(
            "Upgrading model '{}' from latest '{}'",
            summary.model_id, summary.revision
        ));
    }

    fn begin_remove(&mut self, model_id: String) {
        let config = self.config.clone();
        let bindings = active_bindings_for_model(&config, &model_id);
        let (tx, rx) = mpsc::channel();
        self.task_rx = Some(rx);
        thread::spawn(move || {
            let result = ModelService::open_default(&config)
                .and_then(|service| service.remove_model(&model_id, &bindings))
                .map(|_| model_id.clone())
                .map_err(|err| err.to_string());
            let _ = tx.send(ModelTaskMessage::Removed(result));
        });
    }

    fn poll_tasks(&mut self, notifications: &mut NotificationCenter) {
        let mut refresh_after = false;
        let mut clear_receiver = false;
        let Some(rx) = self.task_rx.take() else {
            return;
        };
        while let Ok(message) = rx.try_recv() {
            match message {
                ModelTaskMessage::Refreshed(result) => {
                    clear_receiver = true;
                    match result {
                        Ok(installed) => self.installed = installed,
                        Err(err) => notifications.error(format!("Refresh failed: {err}")),
                    }
                }
                ModelTaskMessage::Progress(progress) => {
                    self.progress_by_file
                        .insert(progress.file_name.clone(), progress.clone());
                    self.progress = Some(progress);
                }
                ModelTaskMessage::Installed(result) => {
                    clear_receiver = true;
                    self.progress = None;
                    self.progress_by_file.clear();
                    self.install_cancel = None;
                    match result {
                        Ok(installed) => {
                            if installed.up_to_date {
                                notifications.info(format!(
                                    "Model '{}' is already up to date",
                                    installed.manifest.model_id
                                ));
                            } else {
                                notifications.success(format!(
                                    "Installed model '{}'",
                                    installed.manifest.model_id
                                ));
                            }
                            refresh_after = true;
                        }
                        Err(err) if err == "operation cancelled" => {
                            notifications.info("Model install cancelled");
                        }
                        Err(err) => notifications.error(format!("Install failed: {err}")),
                    }
                }
                ModelTaskMessage::Removed(result) => {
                    clear_receiver = true;
                    match result {
                        Ok(model_id) => {
                            notifications.success(format!("Removed model '{model_id}'"));
                            refresh_after = true;
                        }
                        Err(err) => notifications.error(format!("Remove failed: {err}")),
                    }
                }
            }
        }
        if !clear_receiver {
            self.task_rx = Some(rx);
        }
        if refresh_after {
            self.begin_refresh();
        }
    }

    fn render_install_window(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        if !self.install_window_open {
            return;
        }
        let mut open = self.install_window_open;
        let mut install_clicked = false;
        let mut cancel_clicked = false;
        egui::Window::new("Install Model")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label("Download a complete Hugging Face repository snapshot.");
                ui.label("Repository");
                ui.text_edit_singleline(&mut self.install_form.repo_id);
                ui.label("Branch / revision");
                ui.text_edit_singleline(&mut self.install_form.revision);
                ui.horizontal(|ui| {
                    cancel_clicked = ui.button("Cancel").clicked();
                    install_clicked = ui.button("Download").clicked();
                });
            });
        if cancel_clicked {
            open = false;
        }
        if install_clicked {
            match self.install_form.to_request() {
                Ok(request) => {
                    open = false;
                    self.begin_install(request);
                    notifications.info("Starting model download");
                }
                Err(err) => notifications.error(err),
            }
        }
        self.install_window_open = open;
    }

    fn render_install_progress_window(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        if self.install_cancel.is_none() && self.progress_by_file.is_empty() {
            return;
        }
        let current_label = self.progress.as_ref().map(|progress| {
            format!(
                "File {} / {}: {}",
                progress.file_index, progress.total_files, progress.file_name
            )
        });
        egui::Window::new("Downloading Model")
            .collapsible(false)
            .resizable(true)
            .show(ui.ctx(), |ui| {
                if let Some(label) = current_label.as_ref() {
                    ui.label(label);
                } else {
                    ui.label("Preparing repository file list...");
                }
                if let Some(overall) = self.overall_progress() {
                    ui.add(egui::ProgressBar::new(overall).text("Overall progress"));
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for progress in self.progress_by_file.values() {
                            ui.label(&progress.file_name);
                            let text = if let Some(total_bytes) = progress.total_bytes {
                                format!("{} / {} bytes", progress.downloaded_bytes, total_bytes)
                            } else {
                                format!("{} bytes", progress.downloaded_bytes)
                            };
                            let value = progress
                                .total_bytes
                                .filter(|total| *total > 0)
                                .map(|total| progress.downloaded_bytes as f32 / total as f32)
                                .unwrap_or(0.0);
                            ui.add(egui::ProgressBar::new(value.clamp(0.0, 1.0)).text(text));
                        }
                    });
                if ui.button("Cancel Download").clicked() {
                    if let Some(token) = self.install_cancel.as_ref() {
                        token.cancel();
                        notifications.info("Cancelling model download");
                    }
                }
            });
    }

    fn overall_progress(&self) -> Option<f32> {
        let current = self.progress.as_ref()?;
        if current.total_files == 0 {
            return None;
        }
        let completed = self
            .progress_by_file
            .values()
            .filter(|progress| {
                progress
                    .total_bytes
                    .is_some_and(|total| total > 0 && progress.downloaded_bytes >= total)
            })
            .count() as f32;
        let current_fraction = current
            .total_bytes
            .filter(|total| *total > 0)
            .map(|total| current.downloaded_bytes as f32 / total as f32)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        Some(((completed + current_fraction) / current.total_files as f32).clamp(0.0, 1.0))
    }

    fn render_installed_models(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        ui.label("Installed Models");
        if self.installed.is_empty() {
            ui.label("No local models installed yet.");
            return;
        }
        let mut delete_model_id = None;
        let mut upgrade_summary = None;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .column(Column::remainder())
            .column(Column::auto())
            .column(Column::remainder())
            .header(22.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Size");
                });
                header.col(|ui| {
                    ui.strong("Created");
                });
            })
            .body(|mut body| {
                for summary in self.installed.clone() {
                    let is_selected = self.selected_model.as_deref() == Some(&summary.model_id);
                    body.row(28.0, |mut row| {
                        row.set_selected(is_selected);
                        let mut row_clicked = false;
                        row.col(|ui| {
                            let response = ui.selectable_label(
                                is_selected,
                                RichText::new(format!(
                                    "{} ({})",
                                    summary.repo_id, summary.revision
                                ))
                                .monospace(),
                            );
                            row_clicked |= response.clicked();
                            model_row_context_menu(
                                response,
                                &summary,
                                &mut upgrade_summary,
                                &mut delete_model_id,
                            );
                        });
                        row.col(|ui| {
                            let response =
                                ui.selectable_label(is_selected, format_bytes(summary.size_bytes));
                            row_clicked |= response.clicked();
                            model_row_context_menu(
                                response,
                                &summary,
                                &mut upgrade_summary,
                                &mut delete_model_id,
                            );
                        });
                        row.col(|ui| {
                            let response = ui.selectable_label(is_selected, &summary.installed_at);
                            row_clicked |= response.clicked();
                            model_row_context_menu(
                                response,
                                &summary,
                                &mut upgrade_summary,
                                &mut delete_model_id,
                            );
                        });

                        if row_clicked {
                            self.selected_model = if is_selected {
                                None
                            } else {
                                Some(summary.model_id.clone())
                            };
                        }
                    });
                }
            });
        if let Some(model_id) = delete_model_id {
            self.delete_confirm = Some(model_id);
        }
        if let Some(summary) = upgrade_summary {
            self.begin_upgrade(&summary, notifications);
        }
    }

    fn render_delete_confirm(&mut self, ui: &mut egui::Ui, notifications: &mut NotificationCenter) {
        let Some(model_id) = self.delete_confirm.clone() else {
            return;
        };
        egui::Window::new("Delete Local Model")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(RichText::new(format!("Delete model '{model_id}'?")).strong());
                ui.add_space(8.0);
                ui.label("This removes the local snapshot files and manifest. Models currently bound in config cannot be deleted.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.delete_confirm = None;
                    }
                    if ui
                        .add(egui::Button::new(
                            RichText::new(format!("{} Delete", regular::TRASH))
                                .color(ui.visuals().warn_fg_color),
                        ))
                        .clicked()
                    {
                        self.delete_confirm = None;
                        self.begin_remove(model_id.clone());
                        notifications.info(format!("Removing model '{model_id}'"));
                    }
                });
            });
    }
}

fn active_bindings_for_model(config: &AppConfig, model_id: &str) -> Vec<ModelUsageBinding> {
    let mut bindings = Vec::new();
    if config.models.default_embedding_model_id.as_deref() == Some(model_id) {
        bindings.push(ModelUsageBinding::Embedding);
    }
    if config.models.default_reranker_model_id.as_deref() == Some(model_id) {
        bindings.push(ModelUsageBinding::Reranker);
    }
    if config.models.default_chat_model_id.as_deref() == Some(model_id) {
        bindings.push(ModelUsageBinding::Chat);
    }
    if config.knowledge.models.embedding_model_id.as_deref() == Some(model_id) {
        bindings.push(ModelUsageBinding::KnowledgeEmbedding);
    }
    if config.knowledge.models.orchestrator_model_id.as_deref() == Some(model_id) {
        bindings.push(ModelUsageBinding::KnowledgeOrchestrator);
    }
    if config.knowledge.models.reranker_model_id.as_deref() == Some(model_id) {
        bindings.push(ModelUsageBinding::KnowledgeReranker);
    }
    bindings
}

fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if value < 1024 {
        return format!("{value} B");
    }
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

fn model_row_context_menu(
    response: egui::Response,
    summary: &ModelSummary,
    upgrade_summary: &mut Option<ModelSummary>,
    delete_model_id: &mut Option<String>,
) {
    response.context_menu(|ui| {
        if ui
            .button(format!("{} Upgrade", regular::ARROW_CLOCKWISE))
            .clicked()
        {
            *upgrade_summary = Some(summary.clone());
            ui.close();
        }
        ui.separator();
        if ui
            .add(egui::Button::new(
                RichText::new(format!("{} Delete", regular::TRASH))
                    .color(ui.visuals().warn_fg_color),
            ))
            .clicked()
        {
            *delete_model_id = Some(summary.model_id.clone());
            ui.close();
        }
    });
}

fn open_path_in_os(path: &std::path::Path) -> Result<(), std::io::Error> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?.wait()?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer").arg(path).spawn()?.wait()?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(path).spawn()?.wait()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_form_builds_repo_revision_request() {
        let form = InstallForm {
            repo_id: "Qwen/Qwen3-Embedding-0.6B-GGUF".to_string(),
            revision: "main".to_string(),
        };

        let request = form.to_request().expect("request should parse");
        assert_eq!(request.repo_id, "Qwen/Qwen3-Embedding-0.6B-GGUF");
        assert_eq!(request.revision, "main");
        assert!(request.quantization.is_none());
    }

    #[test]
    fn format_bytes_uses_readable_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn active_bindings_include_global_and_knowledge_assignments() {
        let mut config = AppConfig::default();
        config.models.default_embedding_model_id = Some("m1".to_string());
        config.models.default_chat_model_id = Some("m1".to_string());
        config.knowledge.models.reranker_model_id = Some("m1".to_string());

        let bindings = active_bindings_for_model(&config, "m1");
        assert!(bindings.contains(&ModelUsageBinding::Embedding));
        assert!(bindings.contains(&ModelUsageBinding::Chat));
        assert!(bindings.contains(&ModelUsageBinding::KnowledgeReranker));
    }
}
