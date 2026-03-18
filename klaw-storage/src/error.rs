use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("cannot resolve home directory for klaw data path")]
    HomeDirUnavailable,
    #[error("failed to create data directory: {0}")]
    CreateDataDir(#[source] std::io::Error),
    #[error("failed to create temporary data directory: {0}")]
    CreateTmpDir(#[source] std::io::Error),
    #[error("failed to create sessions directory: {0}")]
    CreateSessionsDir(#[source] std::io::Error),
    #[error("failed to create archives directory: {0}")]
    CreateArchivesDir(#[source] std::io::Error),
    #[error("failed to append JSONL record: {0}")]
    WriteJsonl(#[source] std::io::Error),
    #[error("failed to read JSONL record: {0}")]
    ReadJsonl(#[source] std::io::Error),
    #[error("failed to serialize JSONL record: {0}")]
    SerializeJson(#[source] serde_json::Error),
    #[error("backend error: {0}")]
    Backend(String),
}

impl StorageError {
    pub fn backend<E: std::fmt::Display>(err: E) -> Self {
        Self::Backend(err.to_string())
    }
}
