use std::sync::{Arc, Mutex};

use klaw_config::AppConfig;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

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
        let downloader = HuggingFaceDownloader::new(
            config.models.huggingface.endpoint.clone(),
            config.models.huggingface.token.clone(),
        )?;
        Ok(Self {
            storage,
            downloader,
        })
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

    pub fn set_default_gguf_model_file(
        &self,
        model_id: &str,
        relative_path: Option<String>,
    ) -> Result<InstalledModelManifest, ModelError> {
        self.storage
            .set_default_gguf_model_file(model_id, relative_path)
    }

    pub async fn install_model<F>(
        &self,
        request: ModelInstallRequest,
        cancellation: CancellationToken,
        progress: F,
    ) -> Result<ModelInstallResult, ModelError>
    where
        F: FnMut(DownloadProgress) + Send + 'static,
    {
        let model_ref =
            HuggingFaceModelRef::new(request.repo_id.clone(), request.revision.clone())?;
        let model_id = normalize_model_id(&request.repo_id, &request.revision);
        let progress = Arc::new(Mutex::new(progress));
        let mut files = Vec::new();
        let resolved_revision = self
            .downloader
            .resolve_revision_sha(&model_ref, &cancellation)
            .await?;
        if let Some(manifest) =
            current_manifest_if_matching(&self.storage, &model_id, resolved_revision.as_deref())
        {
            return Ok(ModelInstallResult {
                manifest,
                downloaded_files: 0,
                up_to_date: true,
            });
        }
        let file_names = self
            .downloader
            .list_repo_files(&model_ref, &cancellation)
            .await?;
        let total_files = file_names.len();

        for (index, file_name) in file_names.iter().enumerate() {
            let file_index = index + 1;
            if let Ok(mut callback) = progress.lock() {
                (callback)(DownloadProgress {
                    model_id: model_id.clone(),
                    file_name: file_name.clone(),
                    downloaded_bytes: 0,
                    total_bytes: None,
                    file_index,
                    total_files,
                });
            }
            let progress_ref = Arc::clone(&progress);
            let downloaded = self
                .downloader
                .download_file(
                    &model_id,
                    &model_ref,
                    file_name,
                    file_index,
                    total_files,
                    self.storage.paths(),
                    &cancellation,
                    move |update| {
                        if let Ok(mut callback) = progress_ref.lock() {
                            (callback)(update);
                        }
                    },
                )
                .await?;
            files.push(downloaded);
        }

        let downloaded_files = files.len();
        let size_bytes = files.iter().map(|file| file.size_bytes).sum();
        let manifest = InstalledModelManifest {
            model_id: model_id.clone(),
            source: "huggingface".to_string(),
            repo_id: request.repo_id,
            revision: request.revision,
            resolved_revision,
            default_gguf_model_file: None,
            files,
            capabilities: Vec::new(),
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
            downloaded_files,
            up_to_date: false,
        })
    }
}

fn current_manifest_if_matching(
    storage: &ModelStorage,
    model_id: &str,
    resolved_revision: Option<&str>,
) -> Option<InstalledModelManifest> {
    let resolved_revision = resolved_revision?;
    let manifest = storage.load_manifest(model_id).ok()?;
    (manifest.resolved_revision.as_deref() == Some(resolved_revision)).then_some(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InstalledModelFile, InstalledModelManifest, ModelCapability, ModelFileFormat,
        ModelStoragePaths,
    };

    #[tokio::test]
    async fn cancelled_install_stops_before_manifest_is_saved() {
        let root = std::env::temp_dir().join(format!("klaw-model-cancel-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root));
        storage.paths().ensure_dirs().expect("dirs");
        let service = ModelService {
            storage,
            downloader: HuggingFaceDownloader::new("https://huggingface.co", None)
                .expect("downloader"),
        };
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let request = ModelInstallRequest {
            repo_id: "Qwen/Qwen3-Reranker-0.6B".to_string(),
            revision: "main".to_string(),
            quantization: None,
        };

        let err = service
            .install_model(request, cancellation, |_| {})
            .await
            .expect_err("cancelled install should fail");

        assert!(matches!(err, ModelError::Cancelled));
        assert!(service.list_installed().expect("list installed").is_empty());
    }

    #[test]
    fn current_manifest_matches_resolved_revision() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-current-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root));
        storage.paths().ensure_dirs().expect("dirs");
        storage
            .save_manifest(&InstalledModelManifest {
                model_id: "qwen-main".to_string(),
                source: "huggingface".to_string(),
                repo_id: "Qwen/Qwen3".to_string(),
                revision: "main".to_string(),
                resolved_revision: Some("abcdef".to_string()),
                default_gguf_model_file: None,
                files: vec![InstalledModelFile {
                    relative_path: "snapshots/qwen-main/model.gguf".to_string(),
                    size_bytes: 10,
                    sha256: None,
                    format: ModelFileFormat::Gguf,
                }],
                capabilities: vec![ModelCapability::Chat],
                quantization: None,
                size_bytes: 10,
                installed_at: "2026-04-25T00:00:00Z".to_string(),
                last_used_at: None,
            })
            .expect("manifest");

        let current = current_manifest_if_matching(&storage, "qwen-main", Some("abcdef"))
            .expect("matching manifest should be returned");

        assert_eq!(current.model_id, "qwen-main");
        assert!(current_manifest_if_matching(&storage, "qwen-main", Some("new-sha")).is_none());
        assert!(current_manifest_if_matching(&storage, "qwen-main", None).is_none());
    }
}
