use klaw_storage::StorageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApprovalError {
    #[error("invalid args: {0}")]
    InvalidArgs(String),
    #[error("invalid approval row: {0}")]
    InvalidApprovalRow(String),
    #[error("approval `{0}` is not a shell approval")]
    NotShellApproval(String),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}
