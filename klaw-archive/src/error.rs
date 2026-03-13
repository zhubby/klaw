use klaw_storage::StorageError;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("invalid archive query: {0}")]
    InvalidQuery(String),
    #[error("archive record not found: {0}")]
    NotFound(String),
    #[error("failed to serialize archive metadata: {0}")]
    SerializeMetadata(#[from] serde_json::Error),
    #[error("failed to read `{path}`: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write `{path}`: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to rename `{from}` to `{to}`: {source}")]
    RenameFile {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl ArchiveError {
    pub fn read_file(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::ReadFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    pub fn write_file(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::WriteFile {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    pub fn rename_file(
        from: impl AsRef<Path>,
        to: impl AsRef<Path>,
        source: std::io::Error,
    ) -> Self {
        Self::RenameFile {
            from: from.as_ref().to_path_buf(),
            to: to.as_ref().to_path_buf(),
            source,
        }
    }
}
