use klaw_storage::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("embedding provider error: {0}")]
    Provider(String),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("capability unavailable: {0}")]
    CapabilityUnavailable(String),
}
