mod error;
mod governance;
mod prompt;
mod provider;
mod service;
mod stats;
mod types;
mod util;

pub use error::MemoryError;
pub use governance::{
    GovernedLongTermWrite, LongTermMemoryKind, LongTermMemoryStatus, govern_long_term_write,
    normalize_content as normalize_long_term_content, read_kind as read_long_term_kind,
    read_status as read_long_term_status, read_topic as read_long_term_topic,
};
pub use prompt::{LongTermMemoryPromptOptions, render_long_term_memory_section};
pub use provider::{OpenAiEmbeddingProvider, build_embedding_provider_from_config};
pub use service::SqliteMemoryService;
pub use stats::{MemoryStats, ScopeStat, SqliteMemoryStatsService};
pub use types::{
    EmbeddingProvider, MemoryHit, MemoryRecord, MemorySearchQuery, MemoryService, UpsertMemoryInput,
};

#[cfg(test)]
mod tests;
