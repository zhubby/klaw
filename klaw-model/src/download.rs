use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::Digest;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::{
    HuggingFaceModelRef, InstalledModelFile, ModelError, ModelFileFormat, ModelStoragePaths,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub model_id: String,
    pub file_name: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub file_index: usize,
    pub total_files: usize,
}

#[derive(Debug, Deserialize)]
struct HuggingFaceTreeEntry {
    path: String,
    #[serde(rename = "type")]
    entry_type: String,
}

#[derive(Debug, Deserialize)]
struct HuggingFaceModelInfo {
    sha: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HuggingFaceDownloader {
    client: Client,
    endpoint: String,
    auth_token: Option<String>,
}

impl HuggingFaceDownloader {
    pub fn new(
        endpoint: impl Into<String>,
        auth_token: Option<String>,
    ) -> Result<Self, ModelError> {
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
        file_index: usize,
        total_files: usize,
        storage_paths: &ModelStoragePaths,
        cancellation: &CancellationToken,
        mut progress: F,
    ) -> Result<InstalledModelFile, ModelError>
    where
        F: FnMut(DownloadProgress) + Send,
    {
        check_cancelled(cancellation)?;
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
        check_cancelled(cancellation)?;
        let total_bytes = response.content_length();
        let temp_path = storage_paths.downloads_dir.join(format!(
            "{}.{}.part",
            model_id,
            file_name.replace('/', "__")
        ));
        let final_path = storage_paths.snapshots_dir.join(model_id).join(file_name);
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
            if cancellation.is_cancelled() {
                drop(file);
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(ModelError::Cancelled);
            }
            let chunk = chunk.map_err(|err| ModelError::Download(err.to_string()))?;
            file.write_all(&chunk).await?;
            sha2::Digest::update(&mut hasher, &chunk);
            downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
            progress(DownloadProgress {
                model_id: model_id.to_string(),
                file_name: file_name.to_string(),
                downloaded_bytes,
                total_bytes,
                file_index,
                total_files,
            });
        }

        check_cancelled(cancellation)?;
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

    pub async fn list_repo_files(
        &self,
        model_ref: &HuggingFaceModelRef,
        cancellation: &CancellationToken,
    ) -> Result<Vec<String>, ModelError> {
        check_cancelled(cancellation)?;
        let url = format!(
            "{}/api/models/{}/tree/{}",
            self.endpoint.trim_end_matches('/'),
            model_ref.repo_id,
            model_ref.revision
        );
        let mut request = self.client.get(url).query(&[("recursive", "true")]);
        if let Some(token) = self.auth_token.as_ref() {
            request = request.bearer_auth(token);
        }
        let body = request.send().await?.error_for_status()?.text().await?;
        check_cancelled(cancellation)?;
        collect_tree_file_paths(&body)
    }

    pub async fn resolve_revision_sha(
        &self,
        model_ref: &HuggingFaceModelRef,
        cancellation: &CancellationToken,
    ) -> Result<Option<String>, ModelError> {
        check_cancelled(cancellation)?;
        let url = format!(
            "{}/api/models/{}/revision/{}",
            self.endpoint.trim_end_matches('/'),
            model_ref.repo_id,
            model_ref.revision
        );
        let mut request = self.client.get(url);
        if let Some(token) = self.auth_token.as_ref() {
            request = request.bearer_auth(token);
        }
        let body = request.send().await?.error_for_status()?.text().await?;
        check_cancelled(cancellation)?;
        parse_revision_sha(&body)
    }
}

fn collect_tree_file_paths(body: &str) -> Result<Vec<String>, ModelError> {
    let entries = serde_json::from_str::<Vec<HuggingFaceTreeEntry>>(body)
        .map_err(|err| ModelError::Download(format!("failed to parse Hugging Face tree: {err}")))?;
    let files = entries
        .into_iter()
        .filter(|entry| entry.entry_type == "file")
        .map(|entry| entry.path)
        .filter(|path| !path.trim().is_empty())
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Err(ModelError::Download(
            "Hugging Face repository tree contains no files".to_string(),
        ));
    }
    Ok(files)
}

fn parse_revision_sha(body: &str) -> Result<Option<String>, ModelError> {
    let info = serde_json::from_str::<HuggingFaceModelInfo>(body).map_err(|err| {
        ModelError::Download(format!("failed to parse Hugging Face model info: {err}"))
    })?;
    Ok(info.sha.filter(|sha| !sha.trim().is_empty()))
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), ModelError> {
    if cancellation.is_cancelled() {
        return Err(ModelError::Cancelled);
    }
    Ok(())
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
        let path = root.join("snapshots/repo-main/model.gguf");
        assert_eq!(
            relative_to_root(&root, &path),
            "snapshots/repo-main/model.gguf".to_string()
        );
    }

    #[test]
    fn collect_tree_file_paths_keeps_only_files() {
        let body = r#"[
            {"path":"model.gguf","type":"file"},
            {"path":"nested/tokenizer.json","type":"file"},
            {"path":"nested","type":"directory"}
        ]"#;

        let files = collect_tree_file_paths(body).expect("tree should parse");

        assert_eq!(files, vec!["model.gguf", "nested/tokenizer.json"]);
    }

    #[test]
    fn collect_tree_file_paths_rejects_empty_file_list() {
        let body = r#"[{"path":"nested","type":"directory"}]"#;

        let err = collect_tree_file_paths(body).expect_err("empty file list should fail");

        assert!(matches!(err, ModelError::Download(_)));
    }

    #[test]
    fn parse_revision_sha_reads_model_info_sha() {
        let body = r#"{"id":"Qwen/Qwen3","sha":"abcdef123456"}"#;

        let sha = parse_revision_sha(body).expect("model info should parse");

        assert_eq!(sha.as_deref(), Some("abcdef123456"));
    }
}
