use crate::{EmbeddingProvider, MemoryError};
use async_trait::async_trait;
use klaw_config::{AppConfig, ModelProviderConfig};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{env, sync::Arc};

pub fn build_embedding_provider_from_config(
    config: &AppConfig,
) -> Result<Arc<dyn EmbeddingProvider>, MemoryError> {
    let provider_id = config.memory.embedding.provider.trim();
    if provider_id.is_empty() {
        return Err(MemoryError::InvalidConfig(
            "memory.embedding.provider cannot be empty".to_string(),
        ));
    }
    let model = config.memory.embedding.model.trim();
    if model.is_empty() {
        return Err(MemoryError::InvalidConfig(
            "memory.embedding.model cannot be empty".to_string(),
        ));
    }
    let provider_cfg = config.model_providers.get(provider_id).ok_or_else(|| {
        MemoryError::InvalidConfig(format!(
            "memory.embedding.provider '{}' not found in model_providers",
            provider_id
        ))
    })?;
    let api_key = resolve_api_key(provider_cfg).ok_or_else(|| {
        MemoryError::InvalidConfig(format!(
            "provider '{}' requires api_key or env_key",
            provider_id
        ))
    })?;

    Ok(Arc::new(OpenAiEmbeddingProvider {
        provider_name: provider_id.to_string(),
        base_url: provider_cfg.base_url.clone(),
        model: model.to_string(),
        api_key,
        client: Client::new(),
    }))
}

#[derive(Debug, Clone)]
pub struct OpenAiEmbeddingProvider {
    provider_name: String,
    base_url: String,
    model: String,
    api_key: String,
    client: Client,
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingProvider {
    fn provider_name(&self) -> &str {
        &self.provider_name
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, MemoryError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let endpoint = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.api_key)
            .json(&EmbeddingRequest {
                model: &self.model,
                input: &texts,
            })
            .send()
            .await
            .map_err(|err| MemoryError::Provider(format!("request failed: {err}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(MemoryError::Provider(format!(
                "embedding API returned {status}: {body}"
            )));
        }

        let parsed: EmbeddingResponse = response
            .json()
            .await
            .map_err(|err| MemoryError::Provider(format!("invalid response payload: {err}")))?;
        Ok(parsed.data.into_iter().map(|item| item.embedding).collect())
    }
}

fn resolve_api_key(provider: &ModelProviderConfig) -> Option<String> {
    provider.api_key.clone().or_else(|| {
        provider
            .env_key
            .as_ref()
            .and_then(|env_name| env::var(env_name).ok())
    })
}
