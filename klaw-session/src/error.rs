use klaw_storage::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}
