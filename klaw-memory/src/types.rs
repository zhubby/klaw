use crate::MemoryError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub scope: String,
    pub content: String,
    pub metadata: Value,
    pub pinned: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct UpsertMemoryInput {
    pub id: Option<String>,
    pub scope: String,
    pub content: String,
    pub metadata: Value,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub struct MemorySearchQuery {
    pub scope: Option<String>,
    pub text: String,
    pub limit: usize,
    pub fts_limit: usize,
    pub vector_limit: usize,
    pub use_vector: bool,
}

impl Default for MemorySearchQuery {
    fn default() -> Self {
        Self {
            scope: None,
            text: String::new(),
            limit: 8,
            fts_limit: 20,
            vector_limit: 20,
            use_vector: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub record: MemoryRecord,
    pub fused_score: f64,
    pub bm25_rank: Option<usize>,
    pub vector_rank: Option<usize>,
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    fn model(&self) -> &str;
    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, MemoryError>;
}

#[async_trait]
pub trait MemoryService: Send + Sync {
    async fn upsert(&self, input: UpsertMemoryInput) -> Result<MemoryRecord, MemoryError>;
    async fn search(&self, query: MemorySearchQuery) -> Result<Vec<MemoryHit>, MemoryError>;
    async fn get(&self, id: &str) -> Result<Option<MemoryRecord>, MemoryError>;
    async fn delete(&self, id: &str) -> Result<bool, MemoryError>;
    async fn pin(&self, id: &str, pinned: bool) -> Result<Option<MemoryRecord>, MemoryError>;
}
