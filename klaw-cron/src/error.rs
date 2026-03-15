use klaw_core::TransportError;
use klaw_storage::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CronError {
    #[error("invalid schedule: {0}")]
    InvalidSchedule(String),
    #[error("invalid cron row: {0}")]
    InvalidCronRow(String),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("invalid inbound payload json: {0}")]
    InvalidPayload(#[from] serde_json::Error),
}
