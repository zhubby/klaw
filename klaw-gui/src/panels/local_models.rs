use crate::notifications::NotificationCenter;
use crate::panels::{PanelRenderer, RenderCtx};
use egui_extras::{Column, TableBuilder};
use klaw_config::{AppConfig, ConfigSnapshot, ConfigStore};
use klaw_model::{
    DownloadProgress, ModelCapability, ModelInstallRequest, ModelInstallResult, ModelService,
    ModelSummary, ModelUsageBinding,
};
use klaw_util::{default_data_dir, models_dir};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use tokio::runtime::Builder;

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
    files_text: String,
    quantization: String,
    embedding: bool,
    rerank: bool,
    chat: bool,
}

impl Default for InstallForm {
    fn default() -> Self {
        Self {
            repo_id: String::new(),
            revision: "main".to_string(),
            files_text: String::new(),
            quantization: String::new(),
            embedding: true,
            rerank: false,
            chat: false,
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
        let files = self
            .files_text
            .lines()
            .flat_map(|line| line.split(','))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if files.is_empty() {
            return Err("at least one file is required".to_string());
        }
        let mut capabilities = Vec::new();
        if self.embedding {
            capabilities.push(ModelCapability::Embedding);
        }
        if self.rerank {
            capabilities.push(ModelCapability::Rerank);
        }
        if self.chat {
            capabilities.push(ModelCapability::Chat);
        }
        if capabilities.is_empty() {
            return Err("select at least one capability".to_string());
        }
        Ok(ModelInstallRequest {
            repo_id: repo_id.to_string(),
            revision: revision.to_string(),
            files,
            capabilities,
            quantization: (!self.quantization.trim().is_empty())
                .then(|| self.quantization.trim().to_string()),
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
    delete_confirm: Option<String>,
    progress: Option<DownloadProgress>,
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
            if ui.button("Open Models Directory").clicked() {
                if let Err(err) = open_path_in_os(&self.models_dir_path()) {
                    notifications.error(format!("Failed to open models directory: {err}"));
                }
            }
        });

        if let Some(progress) = &self.progress {
            let label = if let Some(total_bytes) = progress.total_bytes {
                format!(
                    "Downloading {}: {} / {} bytes",
                    progress.file_name, progress.downloaded_bytes, total_bytes
                )
            } else {
                format!(
                    "Downloading {}: {} bytes",
                    progress.file_name, progress.downloaded_bytes
                )
            };
            ui.label(label);
        }

        ui.separator();
        ui.collapsing("Install Model", |ui| {
            ui.label("Enter a Hugging Face repo, revision, and one or more files.");
            ui.text_edit_singleline(&mut self.install_form.repo_id);
            ui.text_edit_singleline(&mut self.install_form.revision);
            ui.label("Files (one per line or comma-separated)");
            ui.text_edit_multiline(&mut self.install_form.files_text);
            ui.text_edit_singleline(&mut self.install_form.quantization);
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.install_form.embedding, "Embedding");
                ui.checkbox(&mut self.install_form.rerank, "Rerank");
                ui.checkbox(&mut self.install_form.chat, "Chat");
            });
            if ui.button("Install").clicked() {
                match self.install_form.to_request() {
                    Ok(request) => self.begin_install(request),
                    Err(err) => notifications.error(err),
                }
            }
        });

        ui.separator();
        self.render_installed_models(ui, notifications);
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
        let config = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.task_rx = Some(rx);
        self.progress = None;
        thread::spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime should build");
            let tx_progress = tx.clone();
            let result = runtime.block_on(async move {
                let service = ModelService::open_default(&config).map_err(|err| err.to_string())?;
                service
                    .install_model(request, move |progress| {
                        let _ = tx_progress.send(ModelTaskMessage::Progress(progress));
                    })
                    .await
                    .map_err(|err| err.to_string())
            });
            let _ = tx.send(ModelTaskMessage::Installed(result));
        });
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
                    self.progress = Some(progress);
                }
                ModelTaskMessage::Installed(result) => {
                    clear_receiver = true;
                    self.progress = None;
                    match result {
                        Ok(installed) => {
                            notifications.success(format!(
                                "Installed model '{}'",
                                installed.manifest.model_id
                            ));
                            refresh_after = true;
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
        TableBuilder::new(ui)
            .striped(true)
            .column(Column::remainder())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .body(|mut body| {
                for summary in self.installed.clone() {
                    body.row(28.0, |mut row| {
                        row.col(|ui| {
                            ui.monospace(format!(
                                "{} ({})",
                                summary.model_id, summary.repo_id
                            ));
                        });
                        row.col(|ui| {
                            if ui.button("Use as Embedding").clicked() {
                                self.set_default_binding(
                                    notifications,
                                    |config| {
                                        config.models.default_embedding_model_id =
                                            Some(summary.model_id.clone());
                                    },
                                    "Default embedding model updated",
                                );
                            }
                        });
                        row.col(|ui| {
                            if ui.button("Use as Reranker").clicked() {
                                self.set_default_binding(
                                    notifications,
                                    |config| {
                                        config.models.default_reranker_model_id =
                                            Some(summary.model_id.clone());
                                    },
                                    "Default reranker model updated",
                                );
                            }
                        });
                        row.col(|ui| {
                            if ui.button("Use as Chat").clicked() {
                                self.set_default_binding(
                                    notifications,
                                    |config| {
                                        config.models.default_chat_model_id =
                                            Some(summary.model_id.clone());
                                    },
                                    "Default chat model updated",
                                );
                            }
                        });
                        row.col(|ui| {
                            if ui.button("Remove").clicked() {
                                self.delete_confirm = Some(summary.model_id.clone());
                            }
                        });
                    });
                }
            });
    }

    fn render_delete_confirm(
        &mut self,
        ui: &mut egui::Ui,
        notifications: &mut NotificationCenter,
    ) {
        let Some(model_id) = self.delete_confirm.clone() else {
            return;
        };
        egui::Window::new("Remove Local Model")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(format!("Remove model '{model_id}'?"));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.delete_confirm = None;
                    }
                    if ui.button("Remove").clicked() {
                        self.delete_confirm = None;
                        self.begin_remove(model_id.clone());
                        notifications.info(format!("Removing model '{model_id}'"));
                    }
                });
            });
    }

    fn set_default_binding<F>(
        &mut self,
        notifications: &mut NotificationCenter,
        mutate: F,
        success_message: &str,
    ) where
        F: FnOnce(&mut AppConfig),
    {
        let Some(store) = self.store.as_ref() else {
            notifications.error("Configuration store is not available");
            return;
        };
        match store.update_config(|config| {
            mutate(config);
            Ok(())
        }) {
            Ok((snapshot, ())) => {
                self.apply_snapshot(snapshot);
                notifications.success(success_message);
            }
            Err(err) => notifications.error(format!("Failed to update config: {err}")),
        }
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
    fn install_form_parses_multiline_files_and_capabilities() {
        let form = InstallForm {
            repo_id: "Qwen/Qwen3-Embedding-0.6B-GGUF".to_string(),
            revision: "main".to_string(),
            files_text: "model.gguf\nREADME.md".to_string(),
            quantization: "Q4_K_M".to_string(),
            embedding: true,
            rerank: false,
            chat: true,
        };

        let request = form.to_request().expect("request should parse");
        assert_eq!(request.files, vec!["model.gguf", "README.md"]);
        assert_eq!(
            request.capabilities,
            vec![ModelCapability::Embedding, ModelCapability::Chat]
        );
        assert_eq!(request.quantization.as_deref(), Some("Q4_K_M"));
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
