use std::sync::{Arc, Mutex};

use klaw_config::AppConfig;
use time::OffsetDateTime;

use crate::{
    DownloadProgress, HuggingFaceDownloader, HuggingFaceModelRef, InstalledModelManifest,
    ModelError, ModelInstallRequest, ModelInstallResult, ModelStorage, ModelSummary,
    ModelUsageBinding, normalize_model_id,
};

#[derive(Clone)]
pub struct ModelService {
    storage: ModelStorage,
    downloader: HuggingFaceDownloader,
}

impl ModelService {
    pub fn open_default(config: &AppConfig) -> Result<Self, ModelError> {
        let storage = ModelStorage::open_default(config)?;
        let auth_token = config
            .models
            .huggingface
            .auth_token_env
            .as_deref()
            .and_then(|key| std::env::var(key).ok());
        let downloader =
            HuggingFaceDownloader::new(config.models.huggingface.endpoint.clone(), auth_token)?;
        Ok(Self { storage, downloader })
    }

    pub fn storage(&self) -> &ModelStorage {
        &self.storage
    }

    pub fn list_installed(&self) -> Result<Vec<ModelSummary>, ModelError> {
        self.storage.list_installed()
    }

    pub fn remove_model(
        &self,
        model_id: &str,
        active_bindings: &[ModelUsageBinding],
    ) -> Result<(), ModelError> {
        self.storage.remove_model(model_id, active_bindings)
    }

    pub async fn install_model<F>(
        &self,
        request: ModelInstallRequest,
        progress: F,
    ) -> Result<ModelInstallResult, ModelError>
    where
        F: FnMut(DownloadProgress) + Send + 'static,
    {
        let model_ref = HuggingFaceModelRef::new(request.repo_id.clone(), request.revision.clone())?;
        let model_id = normalize_model_id(&request.repo_id, &request.revision);
        let progress = Arc::new(Mutex::new(progress));
        let mut files = Vec::new();

        for file_name in &request.files {
            let progress_ref = Arc::clone(&progress);
            let downloaded = self
                .downloader
                .download_file(
                    &model_id,
                    &model_ref,
                    file_name,
                    self.storage.paths(),
                    move |update| {
                        if let Ok(mut callback) = progress_ref.lock() {
                            (callback)(update);
                        }
                    },
                )
                .await?;
            files.push(downloaded);
        }

        let size_bytes = files.iter().map(|file| file.size_bytes).sum();
        let manifest = InstalledModelManifest {
            model_id: model_id.clone(),
            source: "huggingface".to_string(),
            repo_id: request.repo_id,
            revision: request.revision,
            files,
            capabilities: request.capabilities,
            quantization: request.quantization,
            size_bytes,
            installed_at: OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
            last_used_at: None,
        };
        self.storage.save_manifest(&manifest)?;
        Ok(ModelInstallResult {
            manifest,
            downloaded_files: request.files.len(),
        })
    }
}
