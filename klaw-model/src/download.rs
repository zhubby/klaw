use futures_util::StreamExt;
use reqwest::Client;
use sha2::Digest;
use tokio::io::AsyncWriteExt;

use crate::{
    HuggingFaceModelRef, InstalledModelFile, ModelError, ModelFileFormat, ModelStoragePaths,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub model_id: String,
    pub file_name: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct HuggingFaceDownloader {
    client: Client,
    endpoint: String,
    auth_token: Option<String>,
}

impl HuggingFaceDownloader {
    pub fn new(endpoint: impl Into<String>, auth_token: Option<String>) -> Result<Self, ModelError> {
        Ok(Self {
            client: Client::builder().build()?,
            endpoint: endpoint.into(),
            auth_token,
        })
    }

    pub async fn download_file<F>(
        &self,
        model_id: &str,
        model_ref: &HuggingFaceModelRef,
        file_name: &str,
        storage_paths: &ModelStoragePaths,
        mut progress: F,
    ) -> Result<InstalledModelFile, ModelError>
    where
        F: FnMut(DownloadProgress) + Send,
    {
        storage_paths.ensure_dirs()?;
        let url = format!(
            "{}/{}/resolve/{}/{}",
            self.endpoint.trim_end_matches('/'),
            model_ref.repo_id,
            model_ref.revision,
            file_name
        );
        let mut request = self.client.get(url);
        if let Some(token) = self.auth_token.as_ref() {
            request = request.bearer_auth(token);
        }
        let response = request.send().await?.error_for_status()?;
        let total_bytes = response.content_length();
        let temp_path = storage_paths
            .downloads_dir
            .join(format!("{}.{}.part", model_id, file_name.replace('/', "__")));
        let final_path = storage_paths
            .blobs_dir
            .join(&model_ref.repo_id)
            .join(&model_ref.revision)
            .join(file_name);
        if let Some(parent) = final_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(parent) = temp_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = tokio::fs::File::create(&temp_path).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded_bytes = 0_u64;
        let mut hasher = sha2::Sha256::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|err| ModelError::Download(err.to_string()))?;
            file.write_all(&chunk).await?;
            sha2::Digest::update(&mut hasher, &chunk);
            downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
            progress(DownloadProgress {
                model_id: model_id.to_string(),
                file_name: file_name.to_string(),
                downloaded_bytes,
                total_bytes,
            });
        }

        file.flush().await?;
        drop(file);
        tokio::fs::rename(&temp_path, &final_path).await?;

        Ok(InstalledModelFile {
            relative_path: relative_to_root(&storage_paths.root_dir, &final_path),
            size_bytes: downloaded_bytes,
            sha256: Some(hex::encode(sha2::Digest::finalize(hasher))),
            format: detect_format(file_name),
        })
    }
}

fn relative_to_root(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn detect_format(file_name: &str) -> ModelFileFormat {
    if file_name.ends_with(".gguf") {
        ModelFileFormat::Gguf
    } else if file_name.ends_with("tokenizer.json") {
        ModelFileFormat::TokenizerJson
    } else {
        ModelFileFormat::Other
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn detects_file_format_from_name() {
        assert_eq!(detect_format("model.gguf"), ModelFileFormat::Gguf);
        assert_eq!(
            detect_format("tokenizer.json"),
            ModelFileFormat::TokenizerJson
        );
        assert_eq!(detect_format("README.md"), ModelFileFormat::Other);
    }

    #[test]
    fn renders_relative_path_with_forward_slashes() {
        let root = PathBuf::from("/tmp/.klaw/models");
        let path = root.join("blobs/repo/main/model.gguf");
        assert_eq!(
            relative_to_root(&root, &path),
            "blobs/repo/main/model.gguf".to_string()
        );
    }
}
