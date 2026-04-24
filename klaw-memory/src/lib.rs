mod error;
mod governance;
mod maintenance;
mod prompt;
mod provider;
mod service;
mod stats;
mod types;
mod util;

pub use error::MemoryError;
pub use governance::{
    GovernedLongTermWrite, LongTermMemoryKind, LongTermMemoryPriority, LongTermMemoryStatus,
    default_priority_for_kind, effective_priority as effective_long_term_priority,
    govern_long_term_write, is_inactive_long_term_record, is_summary_record,
    normalize_content as normalize_long_term_content, read_kind as read_long_term_kind,
    read_archived_at as read_long_term_archived_at,
    read_priority as read_long_term_priority, read_status as read_long_term_status,
    read_topic as read_long_term_topic,
};
pub use maintenance::{
    LongTermArchiveConfig, LongTermArchiveOutcome, archive_stale_long_term_memories,
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
