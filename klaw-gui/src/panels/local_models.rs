use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui::RichText;
use egui_extras::{Column, TableBuilder};
use egui_file_dialog::FileDialog;
use egui_phosphor::regular;
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore};
use klaw_model::{
    DownloadProgress, ModelInstallRequest, ModelInstallResult, ModelService, ModelSummary,
    ModelUsageBinding,
};
use klaw_util::{default_data_dir, models_dir};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
    DefaultGgufUpdated(Result<(String, Option<String>), String>),
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
    default_gguf_dialog: FileDialog,
    pending_default_gguf_model: Option<String>,
}

impl Default for LocalModelsPanel {
    fn default() -> Self {
        Self {
            store: None,
            config: AppConfig::default(),
            installed: Vec::new(),
            task_rx: None,
            install_form: InstallForm::default(),
            install_window_open: false,
            install_cancel: None,
            selected_model: None,
            delete_confirm: None,
            progress: None,
            progress_by_file: BTreeMap::new(),
            default_gguf_dialog: gguf_file_dialog(PathBuf::from(".")),
            pending_default_gguf_model: None,
        }
    }
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

        let selected_model = self.selected_model.clone();
        let selected_default_gguf = self
            .selected_model_summary()
            .and_then(|summary| summary.default_gguf_model_file.clone());
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
            if ui
                .add_enabled(
                    selected_model.is_some(),
                    egui::Button::new("Set Default GGUF File"),
                )
                .clicked()
            {
                if let Some(model_id) = selected_model.clone() {
                    self.open_default_gguf_dialog(model_id);
                }
            }
            if ui
                .add_enabled(
                    selected_model.is_some() && selected_default_gguf.is_some(),
                    egui::Button::new("Clear Default GGUF"),
                )
                .clicked()
            {
                if let Some(model_id) = selected_model.clone() {
                    self.begin_set_default_gguf_model_file(model_id, None);
                }
            }
        });
        if let Some(model_id) = selected_model.as_deref() {
            ui.label(format!(
                "Selected model default GGUF: {}",
                selected_default_gguf.as_deref().unwrap_or("not configured")
            ));
            ui.small(format!("Selected model: {model_id}"));
        } else {
            ui.label("Select a model row to set its default GGUF file.");
        }

        ui.separator();
        self.render_installed_models(ui, notifications);
        self.render_install_window(ui, notifications);
        self.render_install_progress_window(ui, notifications);
        self.render_delete_confirm(ui, notifications);
        self.default_gguf_dialog.update(ui.ctx());
        self.handle_default_gguf_selection(notifications);
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

    fn selected_model_summary(&self) -> Option<&ModelSummary> {
        let selected_model = self.selected_model.as_deref()?;
        self.installed
            .iter()
            .find(|summary| summary.model_id == selected_model)
    }

    fn open_default_gguf_dialog(&mut self, model_id: String) {
        self.default_gguf_dialog =
            gguf_file_dialog(self.models_dir_path().join("snapshots").join(&model_id));
        self.pending_default_gguf_model = Some(model_id);
        self.default_gguf_dialog.pick_file();
    }

    fn handle_default_gguf_selection(&mut self, notifications: &mut NotificationCenter) {
        let Some(path) = self.default_gguf_dialog.take_picked() else {
            return;
        };
        let Some(model_id) = self.pending_default_gguf_model.take() else {
            notifications.error("No selected model for default GGUF file");
            return;
        };
        if !is_gguf_file(&path) {
            notifications.error("Default model file must have a .gguf extension");
            return;
        }
        let value = match gguf_manifest_relative_path(&path, &self.models_dir_path()) {
            Ok(value) => value,
            Err(err) => {
                notifications.error(err);
                return;
            }
        };
        self.begin_set_default_gguf_model_file(model_id, Some(value));
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

    fn begin_set_default_gguf_model_file(
        &mut self,
        model_id: String,
        relative_path: Option<String>,
    ) {
        let config = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.task_rx = Some(rx);
        thread::spawn(move || {
            let result = ModelService::open_default(&config)
                .and_then(|service| {
                    service.set_default_gguf_model_file(&model_id, relative_path.clone())
                })
                .map(|manifest| (manifest.model_id, manifest.default_gguf_model_file))
                .map_err(|err| err.to_string());
            let _ = tx.send(ModelTaskMessage::DefaultGgufUpdated(result));
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
                ModelTaskMessage::DefaultGgufUpdated(result) => {
                    clear_receiver = true;
                    match result {
                        Ok((model_id, default_gguf_model_file)) => {
                            if let Some(summary) = self
                                .installed
                                .iter_mut()
                                .find(|summary| summary.model_id == model_id)
                            {
                                summary.default_gguf_model_file = default_gguf_model_file;
                            }
                            notifications
                                .success(format!("Saved default GGUF file for model '{model_id}'"));
                        }
                        Err(err) => {
                            notifications.error(format!("Failed to save default GGUF file: {err}"));
                        }
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
                let progress_height = ui.spacing().interact_size.y * 0.5;
                if let Some(label) = current_label.as_ref() {
                    ui.label(label);
                } else {
                    ui.label("Preparing repository file list...");
                }
                if let Some(overall) = self.overall_progress() {
                    ui.add(
                        egui::ProgressBar::new(overall)
                            .text("Overall progress")
                            .desired_height(progress_height),
                    );
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
                            ui.add(
                                egui::ProgressBar::new(value.clamp(0.0, 1.0))
                                    .text(text)
                                    .desired_height(progress_height),
                            );
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
        let installed = self.installed.clone();
        let row_height = ui.spacing().interact_size.y;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
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
            .body(|body| {
                body.rows(row_height, installed.len(), |mut row| {
                    let summary = &installed[row.index()];
                    let is_selected = self.selected_model.as_deref() == Some(&summary.model_id);
                    row.set_selected(is_selected);
                    row.col(|ui| {
                        ui.label(
                            RichText::new(format!("{} ({})", summary.repo_id, summary.revision))
                                .monospace(),
                        );
                    });
                    row.col(|ui| {
                        ui.label(format_bytes(summary.size_bytes));
                    });
                    row.col(|ui| {
                        ui.label(&summary.installed_at);
                    });

                    let response = row.response();
                    if response.clicked() {
                        self.selected_model = if is_selected {
                            None
                        } else {
                            Some(summary.model_id.clone())
                        };
                    }
                    model_row_context_menu(
                        response,
                        summary,
                        &mut upgrade_summary,
                        &mut delete_model_id,
                    );
                });
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

fn gguf_file_dialog(initial_directory: PathBuf) -> FileDialog {
    FileDialog::new()
        .initial_directory(initial_directory)
        .add_file_filter_extensions("GGUF model files", vec!["gguf"])
        .default_file_filter("GGUF model files")
}

fn is_gguf_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
}

fn gguf_manifest_relative_path(path: &Path, models_dir: &Path) -> Result<String, String> {
    let selected = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let models_dir = models_dir
        .canonicalize()
        .unwrap_or_else(|_| models_dir.to_path_buf());
    selected
        .strip_prefix(&models_dir)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .map_err(|_| "Default GGUF file must be inside the models directory".to_string())
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
    fn gguf_manifest_relative_path_stores_model_dir_files_relative() {
        let root = PathBuf::from("/tmp/klaw-models");
        let file = root.join("snapshots/qwen/preferred.gguf");

        assert_eq!(
            gguf_manifest_relative_path(&file, &root).expect("path should be relative"),
            "snapshots/qwen/preferred.gguf"
        );
    }

    #[test]
    fn gguf_manifest_relative_path_normalizes_backslashes() {
        let root = PathBuf::from("/tmp/klaw-models");
        let file = root.join("snapshots\\qwen\\preferred.gguf");

        assert_eq!(
            gguf_manifest_relative_path(&file, &root).expect("path should be relative"),
            "snapshots/qwen/preferred.gguf"
        );
    }

    #[test]
    fn gguf_manifest_relative_path_rejects_external_files() {
        let root = PathBuf::from("/tmp/klaw-models");
        let file = PathBuf::from("/tmp/external/preferred.gguf");

        let err = gguf_manifest_relative_path(&file, &root).expect_err("external path should fail");

        assert!(err.contains("models directory"));
    }

    #[test]
    fn is_gguf_file_accepts_extension_case_insensitively() {
        assert!(is_gguf_file(Path::new("model.GGUF")));
        assert!(!is_gguf_file(Path::new("model.bin")));
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
