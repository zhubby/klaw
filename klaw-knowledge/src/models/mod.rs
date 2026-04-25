use async_trait::async_trait;

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
    async fn expand_query(&self, query: &str) -> Result<Vec<String>, KnowledgeError>;
}

#[derive(Debug, Default, Clone)]
pub struct HeuristicOrchestrator;

#[async_trait]
impl OrchestratorModel for HeuristicOrchestrator {
    async fn expand_query(&self, query: &str) -> Result<Vec<String>, KnowledgeError> {
        Ok(vec![query.to_string()])
    }
}
