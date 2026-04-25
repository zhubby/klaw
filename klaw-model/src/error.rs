#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("config error: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("download error: {0}")]
    Download(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("model '{0}' not found")]
    NotFound(String),
    #[error("model '{0}' is currently in use")]
    InUse(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
}

impl From<std::io::Error> for ModelError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<reqwest::Error> for ModelError {
    fn from(value: reqwest::Error) -> Self {
        Self::Network(value.to_string())
    }
}

impl From<serde_json::Error> for ModelError {
    fn from(value: serde_json::Error) -> Self {
        Self::Manifest(value.to_string())
    }
}
