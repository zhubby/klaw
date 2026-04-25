use std::path::Path;

use crate::{InstalledModelManifest, ModelError};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstalledModelsManifest {
    #[serde(default)]
    pub models: Vec<InstalledModelManifest>,
}

pub fn load_manifest(path: &Path) -> Result<InstalledModelManifest, ModelError> {
    let raw = std::fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(ModelError::from)
}

pub fn save_manifest(path: &Path, manifest: &InstalledModelManifest) -> Result<(), ModelError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(manifest)?;
    std::fs::write(path, raw)?;
    Ok(())
}

pub fn load_manifest_index(path: &Path) -> Result<InstalledModelsManifest, ModelError> {
    let raw = std::fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(ModelError::from)
}

pub fn save_manifest_index(
    path: &Path,
    manifest: &InstalledModelsManifest,
) -> Result<(), ModelError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(manifest)?;
    std::fs::write(path, raw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InstalledModelFile, ModelCapability, ModelFileFormat};

    #[test]
    fn saves_and_loads_model_manifest() {
        let root = std::env::temp_dir().join(format!("klaw-model-manifest-{}", std::process::id()));
        let path = root.join("manifest.json");
        let manifest = InstalledModelManifest {
            model_id: "qwen-main".to_string(),
            source: "huggingface".to_string(),
            repo_id: "Qwen/Qwen".to_string(),
            revision: "main".to_string(),
            resolved_revision: Some("abc123".to_string()),
            files: vec![InstalledModelFile {
                relative_path: "snapshots/qwen-main/model.gguf".to_string(),
                size_bytes: 12,
                sha256: Some("abc".to_string()),
                format: ModelFileFormat::Gguf,
            }],
            capabilities: vec![ModelCapability::Embedding],
            quantization: Some("Q4_K_M".to_string()),
            size_bytes: 12,
            installed_at: "2026-04-25T00:00:00Z".to_string(),
            last_used_at: None,
        };

        save_manifest(&path, &manifest).expect("manifest should save");
        let loaded = load_manifest(&path).expect("manifest should load");
        assert_eq!(loaded, manifest);
    }

    #[test]
    fn saves_and_loads_model_manifest_index() {
        let root = std::env::temp_dir().join(format!(
            "klaw-model-manifest-index-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.join("manifest.json");
        let manifest = InstalledModelsManifest {
            models: vec![InstalledModelManifest {
                model_id: "qwen-main".to_string(),
                source: "huggingface".to_string(),
                repo_id: "Qwen/Qwen".to_string(),
                revision: "main".to_string(),
                resolved_revision: Some("abc123".to_string()),
                files: vec![InstalledModelFile {
                    relative_path: "snapshots/qwen-main/model.gguf".to_string(),
                    size_bytes: 12,
                    sha256: Some("abc".to_string()),
                    format: ModelFileFormat::Gguf,
                }],
                capabilities: vec![ModelCapability::Embedding],
                quantization: None,
                size_bytes: 12,
                installed_at: "2026-04-25T00:00:00Z".to_string(),
                last_used_at: None,
            }],
        };

        save_manifest_index(&path, &manifest).expect("manifest index should save");
        let loaded = load_manifest_index(&path).expect("manifest index should load");

        assert_eq!(loaded, manifest);
    }
}
