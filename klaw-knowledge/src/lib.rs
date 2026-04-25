pub mod context;
pub mod error;
pub mod models;
pub mod obsidian;
pub mod provider_router;
pub mod retrieval;
pub mod types;

pub use context::{ContextBundle, ContextSection, assemble_context_bundle};
pub use error::KnowledgeError;
pub use models::{build_local_embedding_model, build_local_orchestrator, build_local_reranker};
pub use obsidian::provider::ObsidianKnowledgeProvider;
pub use provider_router::KnowledgeProviderRouter;
pub use types::{
    KnowledgeEntry, KnowledgeHit, KnowledgeProvider, KnowledgeSearchQuery, KnowledgeSourceInfo,
};
