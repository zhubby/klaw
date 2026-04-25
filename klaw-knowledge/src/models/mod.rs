use std::sync::Arc;

use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_model::{
    EmbeddingRuntime as LocalEmbeddingRuntime, LlamaCppRsBackend, ModelEmbeddingRequest,
    ModelLlamaRuntime, ModelOrchestrateRequest, ModelRerankRequest, ModelService,
    OrchestratorRuntime as LocalOrchestratorRuntime, QueryIntent,
    RerankRuntime as LocalRerankRuntime,
};

use crate::KnowledgeError;

#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, KnowledgeError>;
}

#[async_trait]
pub trait RerankModel: Send + Sync {
    async fn rerank(&self, query: &str, candidates: &[String]) -> Result<Vec<f32>, KnowledgeError>;
}

#[async_trait]
pub trait OrchestratorModel: Send + Sync {
    async fn orchestrate(&self, query: &str) -> Result<KnowledgeOrchestration, KnowledgeError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeOrchestration {
    pub intent: QueryIntent,
    pub expansions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeModelBindings {
    pub embedding_model_id: Option<String>,
    pub orchestrator_model_id: Option<String>,
    pub reranker_model_id: Option<String>,
}

pub fn resolve_model_bindings(config: &AppConfig) -> KnowledgeModelBindings {
    KnowledgeModelBindings {
        embedding_model_id: config
            .knowledge
            .models
            .embedding_model_id
            .clone()
            .or_else(|| config.models.default_embedding_model_id.clone()),
        orchestrator_model_id: config.knowledge.models.orchestrator_model_id.clone(),
        reranker_model_id: config
            .knowledge
            .models
            .reranker_model_id
            .clone()
            .or_else(|| config.models.default_reranker_model_id.clone()),
    }
}

pub struct ModelBackedEmbedding {
    model_id: String,
    runtime: Arc<dyn LocalEmbeddingRuntime>,
}

impl ModelBackedEmbedding {
    pub fn new(model_id: String, runtime: Arc<dyn LocalEmbeddingRuntime>) -> Self {
        Self { model_id, runtime }
    }
}

pub struct ModelBackedReranker {
    model_id: String,
    runtime: Arc<dyn LocalRerankRuntime>,
}

impl ModelBackedReranker {
    pub fn new(model_id: String, runtime: Arc<dyn LocalRerankRuntime>) -> Self {
        Self { model_id, runtime }
    }
}

pub struct ModelBackedOrchestrator {
    model_id: String,
    runtime: Arc<dyn LocalOrchestratorRuntime>,
}

impl ModelBackedOrchestrator {
    pub fn new(model_id: String, runtime: Arc<dyn LocalOrchestratorRuntime>) -> Self {
        Self { model_id, runtime }
    }
}

pub fn build_local_embedding_model(
    config: &AppConfig,
) -> Result<Option<ModelBackedEmbedding>, KnowledgeError> {
    let bindings = resolve_model_bindings(config);
    let Some(model_id) = bindings.embedding_model_id else {
        return Ok(None);
    };
    let service = ModelService::open_default(config)
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    let runtime = ModelLlamaRuntime::new(
        service.storage().clone(),
        LlamaCppRsBackend::new(config.models.llama_cpp.default_ctx_size),
    );
    Ok(Some(ModelBackedEmbedding::new(model_id, Arc::new(runtime))))
}

pub fn build_local_reranker(
    config: &AppConfig,
) -> Result<Option<ModelBackedReranker>, KnowledgeError> {
    let bindings = resolve_model_bindings(config);
    let Some(model_id) = bindings.reranker_model_id else {
        return Ok(None);
    };
    let service = ModelService::open_default(config)
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    let runtime = ModelLlamaRuntime::new(
        service.storage().clone(),
        LlamaCppRsBackend::new(config.models.llama_cpp.default_ctx_size),
    );
    Ok(Some(ModelBackedReranker::new(model_id, Arc::new(runtime))))
}

pub fn build_local_orchestrator(
    config: &AppConfig,
) -> Result<Option<ModelBackedOrchestrator>, KnowledgeError> {
    let bindings = resolve_model_bindings(config);
    let Some(model_id) = bindings.orchestrator_model_id else {
        return Ok(None);
    };
    let service = ModelService::open_default(config)
        .map_err(|err| KnowledgeError::Provider(err.to_string()))?;
    let runtime = ModelLlamaRuntime::new(
        service.storage().clone(),
        LlamaCppRsBackend::new(config.models.llama_cpp.default_ctx_size),
    );
    Ok(Some(ModelBackedOrchestrator::new(
        model_id,
        Arc::new(runtime),
    )))
}

#[derive(Debug, Default, Clone)]
pub struct HeuristicOrchestrator;

#[async_trait]
impl OrchestratorModel for HeuristicOrchestrator {
    async fn orchestrate(&self, query: &str) -> Result<KnowledgeOrchestration, KnowledgeError> {
        Ok(KnowledgeOrchestration {
            intent: QueryIntent::Exploratory,
            expansions: vec![query.to_string()],
        })
    }
}

#[async_trait]
impl EmbeddingModel for ModelBackedEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, KnowledgeError> {
        self.runtime
            .embed(ModelEmbeddingRequest {
                model_id: self.model_id.clone(),
                text: text.to_string(),
            })
            .await
            .map(|response| response.vector)
            .map_err(|err| KnowledgeError::Provider(err.to_string()))
    }
}

#[async_trait]
impl RerankModel for ModelBackedReranker {
    async fn rerank(&self, query: &str, candidates: &[String]) -> Result<Vec<f32>, KnowledgeError> {
        self.runtime
            .rerank(ModelRerankRequest {
                model_id: self.model_id.clone(),
                query: query.to_string(),
                candidates: candidates.to_vec(),
            })
            .await
            .map(|response| response.scores)
            .map_err(|err| KnowledgeError::Provider(err.to_string()))
    }
}

#[async_trait]
impl OrchestratorModel for ModelBackedOrchestrator {
    async fn orchestrate(&self, query: &str) -> Result<KnowledgeOrchestration, KnowledgeError> {
        self.runtime
            .orchestrate(ModelOrchestrateRequest {
                model_id: self.model_id.clone(),
                query: query.to_string(),
            })
            .await
            .map(|response| KnowledgeOrchestration {
                intent: response.intent,
                expansions: response.expansions,
            })
            .map_err(|err| KnowledgeError::Provider(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_knowledge_model_bindings_with_global_fallbacks() {
        let mut config = AppConfig::default();
        config.models.default_embedding_model_id = Some("global-embed".to_string());
        config.models.default_reranker_model_id = Some("global-rerank".to_string());
        config.knowledge.models.embedding_model_id = Some("knowledge-embed".to_string());
        config.knowledge.models.orchestrator_model_id = Some("knowledge-orchestrator".to_string());

        let bindings = resolve_model_bindings(&config);
        assert_eq!(
            bindings.embedding_model_id.as_deref(),
            Some("knowledge-embed")
        );
        assert_eq!(bindings.reranker_model_id.as_deref(), Some("global-rerank"));
        assert_eq!(
            bindings.orchestrator_model_id.as_deref(),
            Some("knowledge-orchestrator")
        );
    }
}
