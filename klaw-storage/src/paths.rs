use crate::StorageError;
use std::{env, path::PathBuf};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub root_dir: PathBuf,
    pub db_path: PathBuf,
    pub sessions_dir: PathBuf,
}

impl StoragePaths {
    pub fn from_home_dir() -> Result<Self, StorageError> {
        let home = env::var_os("HOME").ok_or(StorageError::HomeDirUnavailable)?;
        Ok(Self::from_root(PathBuf::from(home).join(".klaw")))
    }

    pub fn from_root(root_dir: PathBuf) -> Self {
        Self {
            db_path: root_dir.join("klaw.db"),
            sessions_dir: root_dir.join("sessions"),
            root_dir,
        }
    }

    pub async fn ensure_dirs(&self) -> Result<(), StorageError> {
        fs::create_dir_all(&self.root_dir)
            .await
            .map_err(StorageError::CreateDataDir)?;
        fs::create_dir_all(&self.sessions_dir)
            .await
            .map_err(StorageError::CreateSessionsDir)?;
        Ok(())
    }
}
