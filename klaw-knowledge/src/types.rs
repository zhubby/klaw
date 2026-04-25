use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::KnowledgeError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeHit {
    pub id: String,
    pub title: String,
    pub excerpt: String,
    pub score: f64,
    pub tags: Vec<String>,
    pub uri: String,
    pub source: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeEntry {
    pub id: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub uri: String,
    pub source: String,
    pub metadata: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeSourceInfo {
    pub provider: String,
    pub name: String,
    pub description: String,
    pub entry_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeStatus {
    pub enabled: bool,
    pub provider: String,
    pub source_name: String,
    pub vault_path: Option<String>,
    pub entry_count: usize,
    pub chunk_count: usize,
    pub embedded_chunk_count: usize,
    pub missing_embedding_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeSyncResult {
    pub indexed_notes: usize,
    pub embedded_chunks: usize,
    pub status: KnowledgeStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KnowledgeSyncProgressStage {
    IndexingNotes,
    EmbeddingChunks,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeSyncProgress {
    pub stage: KnowledgeSyncProgressStage,
    pub completed: usize,
    pub total: Option<usize>,
    pub current_item: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum KnowledgeRuntimeState {
    Disabled,
    Unconfigured,
    Loading,
    Ready,
    Syncing,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnowledgeRuntimeSnapshot {
    pub state: KnowledgeRuntimeState,
    pub status: Option<KnowledgeStatus>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeSearchQuery {
    pub text: String,
    pub tags: Option<Vec<String>>,
    pub source: Option<String>,
    pub limit: usize,
    pub mode: Option<String>,
}

impl Default for KnowledgeSearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            tags: None,
            source: None,
            limit: 5,
            mode: None,
        }
    }
}

#[async_trait]
pub trait KnowledgeProvider: Send + Sync {
    fn provider_name(&self) -> &str;

    async fn search(
        &self,
        query: KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeHit>, KnowledgeError>;

    async fn get(&self, id: &str) -> Result<Option<KnowledgeEntry>, KnowledgeError>;

    async fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError>;
}
