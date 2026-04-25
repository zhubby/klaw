use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    Embedding,
    Rerank,
    Chat,
    Orchestrator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFileFormat {
    Gguf,
    TokenizerJson,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelUsageBinding {
    Embedding,
    Reranker,
    Chat,
    KnowledgeEmbedding,
    KnowledgeOrchestrator,
    KnowledgeReranker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledModelFile {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub format: ModelFileFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledModelManifest {
    pub model_id: String,
    pub source: String,
    pub repo_id: String,
    pub revision: String,
    pub files: Vec<InstalledModelFile>,
    pub capabilities: Vec<ModelCapability>,
    pub quantization: Option<String>,
    pub size_bytes: u64,
    pub installed_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSummary {
    pub model_id: String,
    pub repo_id: String,
    pub revision: String,
    pub capabilities: Vec<ModelCapability>,
    pub size_bytes: u64,
    pub installed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInstallRequest {
    pub repo_id: String,
    pub revision: String,
    pub files: Vec<String>,
    pub capabilities: Vec<ModelCapability>,
    pub quantization: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInstallResult {
    pub manifest: InstalledModelManifest,
    pub downloaded_files: usize,
}
