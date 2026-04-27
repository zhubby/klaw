#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("invalid note path: {0}")]
    InvalidNotePath(String),
    #[error("note already exists: {0}")]
    NoteAlreadyExists(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("source unavailable: {0}")]
    SourceUnavailable(String),
}
