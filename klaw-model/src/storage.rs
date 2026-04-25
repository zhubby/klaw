use std::path::{Path, PathBuf};

use klaw_config::AppConfig;
use klaw_util::{default_data_dir, models_dir};
use time::OffsetDateTime;

use crate::{
    InstalledModelManifest, ModelError, ModelSummary, ModelUsageBinding, load_manifest,
    save_manifest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStoragePaths {
    pub root_dir: PathBuf,
    pub manifests_dir: PathBuf,
    pub snapshots_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub downloads_dir: PathBuf,
}

impl ModelStoragePaths {
    pub fn from_root(root_dir: PathBuf) -> Self {
        Self {
            manifests_dir: root_dir.join("manifests"),
            snapshots_dir: root_dir.join("snapshots"),
            cache_dir: root_dir.join("cache"),
            downloads_dir: root_dir.join("cache").join("downloads"),
            root_dir,
        }
    }

    pub fn from_config(config: &AppConfig) -> Result<Self, ModelError> {
        let root_dir =
            if let Some(root_dir) = config.models.root_dir.as_ref() {
                PathBuf::from(root_dir)
            } else if let Some(storage_root) = config.storage.root_dir.as_ref() {
                models_dir(storage_root)
            } else {
                models_dir(default_data_dir().ok_or_else(|| {
                    ModelError::Config("home directory is unavailable".to_string())
                })?)
            };
        Ok(Self::from_root(root_dir))
    }

    pub fn ensure_dirs(&self) -> Result<(), ModelError> {
        std::fs::create_dir_all(&self.manifests_dir)?;
        std::fs::create_dir_all(&self.snapshots_dir)?;
        std::fs::create_dir_all(&self.downloads_dir)?;
        Ok(())
    }

    pub fn manifest_path(&self, model_id: &str) -> PathBuf {
        self.manifests_dir.join(format!("{model_id}.json"))
    }
}

#[derive(Debug, Clone)]
pub struct ModelStorage {
    paths: ModelStoragePaths,
}

impl ModelStorage {
    pub fn new(paths: ModelStoragePaths) -> Self {
        Self { paths }
    }

    pub fn open_default(config: &AppConfig) -> Result<Self, ModelError> {
        let paths = ModelStoragePaths::from_config(config)?;
        paths.ensure_dirs()?;
        Ok(Self::new(paths))
    }

    pub fn paths(&self) -> &ModelStoragePaths {
        &self.paths
    }

    pub fn save_manifest(&self, manifest: &InstalledModelManifest) -> Result<(), ModelError> {
        save_manifest(&self.paths.manifest_path(&manifest.model_id), manifest)
    }

    pub fn list_installed(&self) -> Result<Vec<ModelSummary>, ModelError> {
        self.paths.ensure_dirs()?;
        let mut summaries = Vec::new();
        let entries = std::fs::read_dir(&self.paths.manifests_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let manifest = load_manifest(&path)?;
            summaries.push(ModelSummary {
                model_id: manifest.model_id.clone(),
                repo_id: manifest.repo_id.clone(),
                revision: manifest.revision.clone(),
                capabilities: manifest.capabilities.clone(),
                size_bytes: manifest.size_bytes,
                installed_at: manifest.installed_at.clone(),
            });
        }
        summaries.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        Ok(summaries)
    }

    pub fn load_manifest(&self, model_id: &str) -> Result<InstalledModelManifest, ModelError> {
        let path = self.paths.manifest_path(model_id);
        if !path.exists() {
            return Err(ModelError::NotFound(model_id.to_string()));
        }
        load_manifest(&path)
    }

    pub fn mark_used(&self, model_id: &str) -> Result<(), ModelError> {
        let mut manifest = self.load_manifest(model_id)?;
        manifest.last_used_at = Some(now_rfc3339());
        self.save_manifest(&manifest)
    }

    pub fn remove_model(
        &self,
        model_id: &str,
        active_bindings: &[ModelUsageBinding],
    ) -> Result<(), ModelError> {
        if !active_bindings.is_empty() {
            return Err(ModelError::InUse(model_id.to_string()));
        }
        let manifest = self.load_manifest(model_id)?;
        for file in &manifest.files {
            let path = self.paths.root_dir.join(&file.relative_path);
            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
            prune_empty_parents(&path, &self.paths.root_dir);
        }
        let manifest_path = self.paths.manifest_path(model_id);
        if manifest_path.exists() {
            std::fs::remove_file(&manifest_path)?;
        }
        Ok(())
    }
}

fn prune_empty_parents(path: &Path, stop_at: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == stop_at {
            break;
        }
        let is_empty = std::fs::read_dir(dir)
            .ok()
            .and_then(|mut entries| entries.next().transpose().ok())
            .flatten()
            .is_none();
        if !is_empty {
            break;
        }
        let _ = std::fs::remove_dir(dir);
        current = dir.parent();
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InstalledModelFile, ModelCapability, ModelFileFormat, ModelUsageBinding};

    fn sample_manifest(model_id: &str) -> InstalledModelManifest {
        InstalledModelManifest {
            model_id: model_id.to_string(),
            source: "huggingface".to_string(),
            repo_id: "Qwen/Qwen3-Embedding-0.6B-GGUF".to_string(),
            revision: "main".to_string(),
            resolved_revision: Some("abc123".to_string()),
            files: vec![InstalledModelFile {
                relative_path: format!("snapshots/{model_id}/model.gguf"),
                size_bytes: 32,
                sha256: None,
                format: ModelFileFormat::Gguf,
            }],
            capabilities: vec![ModelCapability::Embedding],
            quantization: Some("Q4_K_M".to_string()),
            size_bytes: 32,
            installed_at: "2026-04-25T00:00:00Z".to_string(),
            last_used_at: None,
        }
    }

    #[test]
    fn lists_saved_manifests_as_model_summaries() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-storage-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root.clone()));
        storage.paths.ensure_dirs().expect("dirs");
        storage
            .save_manifest(&sample_manifest("qwen-main"))
            .expect("save manifest");

        let summaries = storage.list_installed().expect("list installed");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].model_id, "qwen-main");
    }

    #[test]
    fn rejects_removal_when_model_is_bound() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-storage-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root.clone()));
        storage.paths.ensure_dirs().expect("dirs");
        let manifest = sample_manifest("qwen-main");
        let file_path = storage
            .paths
            .root_dir
            .join(&manifest.files[0].relative_path);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).expect("blob dir");
        }
        std::fs::write(&file_path, "gguf").expect("blob");
        storage.save_manifest(&manifest).expect("save manifest");

        let err = storage
            .remove_model("qwen-main", &[ModelUsageBinding::Embedding])
            .expect_err("bound model should fail");
        assert!(matches!(err, ModelError::InUse(_)));
    }
}
