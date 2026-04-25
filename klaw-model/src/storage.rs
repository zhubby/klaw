use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use klaw_config::AppConfig;
use klaw_util::{default_data_dir, models_dir};
use time::OffsetDateTime;

use crate::{
    InstalledModelManifest, InstalledModelsManifest, ModelError, ModelSummary, ModelUsageBinding,
    load_manifest, load_manifest_index, save_manifest_index,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStoragePaths {
    pub root_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub snapshots_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub downloads_dir: PathBuf,
}

impl ModelStoragePaths {
    pub fn from_root(root_dir: PathBuf) -> Self {
        Self {
            manifest_path: root_dir.join("manifest.json"),
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
        std::fs::create_dir_all(&self.root_dir)?;
        std::fs::create_dir_all(&self.snapshots_dir)?;
        std::fs::create_dir_all(&self.downloads_dir)?;
        Ok(())
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
        let mut manifests = self.load_all_manifests()?;
        manifests.retain(|existing| existing.model_id != manifest.model_id);
        manifests.push(manifest.clone());
        self.save_all_manifests(manifests)
    }

    pub fn list_installed(&self) -> Result<Vec<ModelSummary>, ModelError> {
        let mut summaries = self
            .load_all_manifests()?
            .into_iter()
            .map(|manifest| ModelSummary {
                model_id: manifest.model_id.clone(),
                repo_id: manifest.repo_id.clone(),
                revision: manifest.revision.clone(),
                default_gguf_model_file: manifest.default_gguf_model_file.clone(),
                capabilities: manifest.capabilities.clone(),
                size_bytes: manifest.size_bytes,
                installed_at: manifest.installed_at.clone(),
            })
            .collect::<Vec<_>>();
        summaries.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        Ok(summaries)
    }

    pub fn load_manifest(&self, model_id: &str) -> Result<InstalledModelManifest, ModelError> {
        self.load_all_manifests()?
            .into_iter()
            .find(|manifest| manifest.model_id == model_id)
            .ok_or_else(|| ModelError::NotFound(model_id.to_string()))
    }

    pub fn mark_used(&self, model_id: &str) -> Result<(), ModelError> {
        let mut manifest = self.load_manifest(model_id)?;
        manifest.last_used_at = Some(now_rfc3339());
        self.save_manifest(&manifest)
    }

    pub fn set_default_gguf_model_file(
        &self,
        model_id: &str,
        relative_path: Option<String>,
    ) -> Result<InstalledModelManifest, ModelError> {
        let mut manifest = self.load_manifest(model_id)?;
        if let Some(path) = relative_path.as_deref() {
            if path.trim().is_empty() {
                return Err(ModelError::Manifest(
                    "default GGUF model file cannot be empty".to_string(),
                ));
            }
            let matches_manifest_gguf = manifest.files.iter().any(|file| {
                file.relative_path == path
                    && file.format == crate::ModelFileFormat::Gguf
                    && file.relative_path.ends_with(".gguf")
            });
            if !matches_manifest_gguf {
                return Err(ModelError::Manifest(format!(
                    "default GGUF model file '{path}' is not a GGUF file in model '{model_id}'"
                )));
            }
        }
        manifest.default_gguf_model_file = relative_path;
        self.save_manifest(&manifest)?;
        Ok(manifest)
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
        let mut manifests = self.load_all_manifests()?;
        manifests.retain(|manifest| manifest.model_id != model_id);
        self.save_all_manifests(manifests)?;
        Ok(())
    }

    fn load_all_manifests(&self) -> Result<Vec<InstalledModelManifest>, ModelError> {
        self.paths.ensure_dirs()?;
        let mut manifests = BTreeMap::new();
        let mut should_persist = false;

        if self.paths.manifest_path.exists() {
            for manifest in load_manifest_index(&self.paths.manifest_path)?.models {
                manifests.insert(manifest.model_id.clone(), manifest);
            }
        }

        let legacy_dir = legacy_manifests_dir(&self.paths.root_dir);
        if legacy_dir.exists() {
            let entries = std::fs::read_dir(&legacy_dir)?;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let manifest = load_manifest(&path)?;
                if !manifests.contains_key(&manifest.model_id) {
                    manifests.insert(manifest.model_id.clone(), manifest);
                    should_persist = true;
                }
            }
        }

        let manifests = manifests.into_values().collect::<Vec<_>>();
        if should_persist || !self.paths.manifest_path.exists() {
            self.save_all_manifests(manifests.clone())?;
        }
        Ok(manifests)
    }

    fn save_all_manifests(
        &self,
        mut manifests: Vec<InstalledModelManifest>,
    ) -> Result<(), ModelError> {
        manifests.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        save_manifest_index(
            &self.paths.manifest_path,
            &InstalledModelsManifest { models: manifests },
        )
    }
}

fn legacy_manifests_dir(root_dir: &Path) -> PathBuf {
    root_dir.join("manifests")
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
            default_gguf_model_file: None,
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
        assert!(summaries[0].default_gguf_model_file.is_none());
        assert!(storage.paths.manifest_path.exists());
        assert!(!legacy_manifests_dir(&root).exists());
    }

    #[test]
    fn sets_default_gguf_model_file_in_manifest() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-storage-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root));
        storage.paths.ensure_dirs().expect("dirs");
        let mut manifest = sample_manifest("qwen-main");
        manifest.files.push(InstalledModelFile {
            relative_path: "snapshots/qwen-main/preferred.gguf".to_string(),
            size_bytes: 64,
            sha256: None,
            format: ModelFileFormat::Gguf,
        });
        storage.save_manifest(&manifest).expect("save manifest");

        let updated = storage
            .set_default_gguf_model_file(
                "qwen-main",
                Some("snapshots/qwen-main/preferred.gguf".to_string()),
            )
            .expect("default gguf should update");

        assert_eq!(
            updated.default_gguf_model_file.as_deref(),
            Some("snapshots/qwen-main/preferred.gguf")
        );
        let persisted = storage.load_manifest("qwen-main").expect("load manifest");
        assert_eq!(
            persisted.default_gguf_model_file,
            updated.default_gguf_model_file
        );
    }

    #[test]
    fn rejects_default_gguf_model_file_not_in_manifest() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-storage-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root));
        storage.paths.ensure_dirs().expect("dirs");
        storage
            .save_manifest(&sample_manifest("qwen-main"))
            .expect("save manifest");

        let err = storage
            .set_default_gguf_model_file("qwen-main", Some("external/preferred.gguf".to_string()))
            .expect_err("unknown gguf should fail");

        assert!(matches!(err, ModelError::Manifest(_)));
    }

    #[test]
    fn clears_default_gguf_model_file_in_manifest() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-storage-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root));
        storage.paths.ensure_dirs().expect("dirs");
        let mut manifest = sample_manifest("qwen-main");
        manifest.default_gguf_model_file = Some("snapshots/qwen-main/model.gguf".to_string());
        storage.save_manifest(&manifest).expect("save manifest");

        let updated = storage
            .set_default_gguf_model_file("qwen-main", None)
            .expect("default gguf should clear");

        assert!(updated.default_gguf_model_file.is_none());
        assert!(
            storage
                .load_manifest("qwen-main")
                .expect("load manifest")
                .default_gguf_model_file
                .is_none()
        );
    }

    #[test]
    fn rejects_default_gguf_model_file_with_non_gguf_format() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-storage-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root));
        storage.paths.ensure_dirs().expect("dirs");
        let mut manifest = sample_manifest("qwen-main");
        manifest.files.push(InstalledModelFile {
            relative_path: "snapshots/qwen-main/tokenizer.json".to_string(),
            size_bytes: 64,
            sha256: None,
            format: ModelFileFormat::TokenizerJson,
        });
        storage.save_manifest(&manifest).expect("save manifest");

        let err = storage
            .set_default_gguf_model_file(
                "qwen-main",
                Some("snapshots/qwen-main/tokenizer.json".to_string()),
            )
            .expect_err("non-gguf default should fail");

        assert!(matches!(err, ModelError::Manifest(_)));
    }

    #[test]
    fn migrates_legacy_per_model_manifests_into_root_manifest() {
        let root =
            std::env::temp_dir().join(format!("klaw-model-storage-{}", uuid::Uuid::new_v4()));
        let storage = ModelStorage::new(ModelStoragePaths::from_root(root.clone()));
        let legacy_dir = legacy_manifests_dir(&root);
        std::fs::create_dir_all(&legacy_dir).expect("legacy dir");
        crate::save_manifest(
            &legacy_dir.join("qwen-main.json"),
            &sample_manifest("qwen-main"),
        )
        .expect("legacy manifest");

        let summaries = storage.list_installed().expect("list installed");

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].model_id, "qwen-main");
        assert!(storage.paths.manifest_path.exists());
        let migrated = crate::load_manifest_index(&storage.paths.manifest_path)
            .expect("root manifest should load");
        assert_eq!(migrated.models.len(), 1);
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

    #[test]
    fn removes_model_from_root_manifest() {
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
            std::fs::create_dir_all(parent).expect("snapshot dir");
        }
        std::fs::write(&file_path, "gguf").expect("snapshot");
        storage.save_manifest(&manifest).expect("save manifest");

        storage
            .remove_model("qwen-main", &[])
            .expect("model should be removed");

        assert!(!file_path.exists());
        assert!(storage.list_installed().expect("list").is_empty());
        let root_manifest = crate::load_manifest_index(&storage.paths.manifest_path)
            .expect("root manifest should load");
        assert!(root_manifest.models.is_empty());
    }
}
