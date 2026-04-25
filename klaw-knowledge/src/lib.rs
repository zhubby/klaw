pub mod context;
pub mod error;
pub mod models;
pub mod obsidian;
pub mod provider_router;
pub mod retrieval;
pub mod types;

use std::{path::PathBuf, sync::Arc};

use klaw_config::AppConfig;
use klaw_storage::open_default_knowledge_db;

pub use context::{ContextBundle, ContextSection, assemble_context_bundle};
pub use error::KnowledgeError;
pub use models::{build_local_embedding_model, build_local_orchestrator, build_local_reranker};
pub use obsidian::provider::ObsidianKnowledgeProvider;
pub use provider_router::KnowledgeProviderRouter;
pub use types::{
    KnowledgeEntry, KnowledgeHit, KnowledgeProvider, KnowledgeSearchQuery, KnowledgeSourceInfo,
    KnowledgeStatus, KnowledgeSyncResult,
};

pub async fn open_configured_obsidian_provider(
    config: &AppConfig,
    index_on_open: bool,
) -> Result<ObsidianKnowledgeProvider, KnowledgeError> {
    let provider_name = config.knowledge.provider.trim();
    if provider_name != "obsidian" {
        return Err(KnowledgeError::InvalidConfig(format!(
            "unsupported knowledge provider '{provider_name}'"
        )));
    }
    let vault_path = configured_vault_path(config)?;
    let db = Arc::new(
        open_default_knowledge_db()
            .await
            .map_err(|err| KnowledgeError::Provider(err.to_string()))?,
    );
    let provider = ObsidianKnowledgeProvider::open(
        db,
        vault_path,
        config.knowledge.obsidian.exclude_folders.clone(),
        config.knowledge.obsidian.max_excerpt_length,
        index_on_open,
        "Obsidian Vault",
    )
    .await?;
    let provider = if let Some(embedder) = build_local_embedding_model(config)? {
        provider.with_embedding_model(Arc::new(embedder))
    } else {
        provider
    };
    let provider = if let Some(reranker) = build_local_reranker(config)? {
        provider.with_reranker(Arc::new(reranker))
    } else {
        provider
    };
    let provider = if let Some(orchestrator) = build_local_orchestrator(config)? {
        provider.with_orchestrator(Arc::new(orchestrator))
    } else {
        provider
    };
    Ok(provider)
}

pub async fn configured_knowledge_status(
    config: &AppConfig,
) -> Result<KnowledgeStatus, KnowledgeError> {
    if config.knowledge.provider.trim() != "obsidian" {
        return Ok(KnowledgeStatus {
            enabled: config.knowledge.enabled,
            provider: config.knowledge.provider.clone(),
            source_name: String::new(),
            vault_path: config.knowledge.obsidian.vault_path.clone(),
            entry_count: 0,
            chunk_count: 0,
            embedded_chunk_count: 0,
            missing_embedding_count: 0,
        });
    }
    let vault_path = config
        .knowledge
        .obsidian
        .vault_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let Some(vault_path) = vault_path else {
        return Ok(KnowledgeStatus {
            enabled: config.knowledge.enabled,
            provider: "obsidian".to_string(),
            source_name: "Obsidian Vault".to_string(),
            vault_path: None,
            entry_count: 0,
            chunk_count: 0,
            embedded_chunk_count: 0,
            missing_embedding_count: 0,
        });
    };
    if !PathBuf::from(vault_path).exists() {
        return Ok(KnowledgeStatus {
            enabled: config.knowledge.enabled,
            provider: "obsidian".to_string(),
            source_name: "Obsidian Vault".to_string(),
            vault_path: Some(vault_path.to_string()),
            entry_count: 0,
            chunk_count: 0,
            embedded_chunk_count: 0,
            missing_embedding_count: 0,
        });
    }
    let provider = open_configured_obsidian_provider(config, false).await?;
    provider.status(config.knowledge.enabled).await
}

pub async fn sync_configured_knowledge(
    config: &AppConfig,
) -> Result<KnowledgeSyncResult, KnowledgeError> {
    let provider = open_configured_obsidian_provider(config, false).await?;
    let indexed_notes = provider.reindex().await?;
    let embedded_chunks = provider.embed_missing_chunks().await?;
    let status = provider.status(config.knowledge.enabled).await?;
    Ok(KnowledgeSyncResult {
        indexed_notes,
        embedded_chunks,
        status,
    })
}

fn configured_vault_path(config: &AppConfig) -> Result<PathBuf, KnowledgeError> {
    config
        .knowledge
        .obsidian
        .vault_path
        .as_ref()
        .map(|path| PathBuf::from(path.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            KnowledgeError::InvalidConfig(
                "knowledge.obsidian.vault_path must be configured".to_string(),
            )
        })
}

#[cfg(test)]
mod tests {
    use klaw_config::AppConfig;

    use super::*;

    #[tokio::test]
    async fn status_allows_disabled_unconfigured_knowledge() {
        let config = AppConfig::default();
        let status = configured_knowledge_status(&config)
            .await
            .expect("status should not require a vault when knowledge is disabled");

        assert!(!status.enabled);
        assert_eq!(status.provider, "obsidian");
        assert_eq!(status.entry_count, 0);
        assert_eq!(status.chunk_count, 0);
        assert_eq!(status.vault_path, None);
    }
}
