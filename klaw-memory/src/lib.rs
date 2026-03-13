mod error;
mod provider;
mod service;
mod types;
mod util;

pub use error::MemoryError;
pub use provider::{build_embedding_provider_from_config, OpenAiEmbeddingProvider};
pub use service::SqliteMemoryService;
pub use types::{
    EmbeddingProvider, MemoryHit, MemoryRecord, MemorySearchQuery, MemoryService, UpsertMemoryInput,
};

#[cfg(test)]
mod tests;
