mod catalog;
mod download;
mod error;
mod llama_cpp;
mod manifest;
mod service;
mod storage;
mod types;

pub use catalog::{HuggingFaceModelRef, normalize_model_id};
pub use download::{DownloadProgress, HuggingFaceDownloader};
pub use error::ModelError;
pub use llama_cpp::{
    ChatRuntime, EmbeddingRuntime, LlamaBackendKind, LlamaCppBackend, LlamaCppCommandBackend,
    LlamaCppRsBackend, ModelChatRequest, ModelChatResponse, ModelEmbeddingRequest,
    ModelEmbeddingResponse, ModelLlamaRuntime, ModelOrchestrateRequest, ModelOrchestrateResponse,
    ModelRerankRequest, ModelRerankResponse, OrchestratorRuntime, PromptFormat, QueryIntent,
    RerankRuntime, default_backend_kind,
};
pub use manifest::{load_manifest, save_manifest};
pub use service::ModelService;
pub use storage::{ModelStorage, ModelStoragePaths};
pub use types::{
    InstalledModelFile, InstalledModelManifest, ModelCapability, ModelFileFormat,
    ModelInstallRequest, ModelInstallResult, ModelSummary, ModelUsageBinding,
};
