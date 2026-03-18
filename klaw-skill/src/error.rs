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
    #[error("failed to parse json at `{path}`: {source}")]
    JsonParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("git command failed ({context}): `{command}`: {stderr}")]
    GitCommand {
        context: &'static str,
        command: String,
        stderr: String,
    },
    #[error(
        "skill `{skill_name}` from registry `{registry}` not found at `{path}`; expected skills/<name>/SKILL.md"
    )]
    RegistrySkillNotFound {
        registry: String,
        skill_name: String,
        path: PathBuf,
    },
    #[error("skills registry `{registry}` is unavailable at `{path}`")]
    RegistryUnavailable { registry: String, path: PathBuf },
    #[error(
        "cannot install managed skill `{skill_name}` because `{path}` already exists and is not managed by registry"
    )]
    LocalSkillConflict { skill_name: String, path: PathBuf },
}
