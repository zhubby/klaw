#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("source unavailable: {0}")]
    SourceUnavailable(String),
}
