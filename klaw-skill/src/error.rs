use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("invalid skill name `{0}`")]
    InvalidSkillName(String),
    #[error("cannot resolve home directory for klaw data path")]
    HomeDirUnavailable,
    #[error("skill `{0}` not found")]
    SkillNotFound(String),
    #[error("network request failed for `{url}`: {source}")]
    Network {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("remote returned non-success status {status} for `{url}`")]
    RemoteStatus { url: String, status: u16 },
    #[error("io `{op}` failed at `{path}`: {source}")]
    Io {
        op: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
