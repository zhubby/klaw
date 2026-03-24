use klaw_archive::ArchiveError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceError {
    #[error("voice config error: {0}")]
    Config(String),
    #[error("unsupported provider: {0}")]
    UnsupportedProvider(String),
    #[error("unsupported operation `{operation}` for provider `{provider}`")]
    UnsupportedOperation {
        provider: &'static str,
        operation: &'static str,
    },
    #[error("missing api key for provider `{0}`")]
    MissingApiKey(&'static str),
    #[error("request failed: {0}")]
    Request(String),
    #[error("websocket failed: {0}")]
    WebSocket(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("io failed: {0}")]
    Io(String),
    #[error("archive failed: {0}")]
    Archive(#[from] ArchiveError),
}

impl From<reqwest::Error> for VoiceError {
    fn from(value: reqwest::Error) -> Self {
        Self::Request(value.to_string())
    }
}

impl From<serde_json::Error> for VoiceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}

impl From<std::io::Error> for VoiceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for VoiceError {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(value.to_string())
    }
}

impl From<tokio::task::JoinError> for VoiceError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::Io(value.to_string())
    }
}
