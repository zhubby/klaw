use crate::StorageError;
use std::{env, path::PathBuf};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub root_dir: PathBuf,
    pub db_path: PathBuf,
    pub memory_db_path: PathBuf,
    pub archive_db_path: PathBuf,
    pub tmp_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub archives_dir: PathBuf,
}

impl StoragePaths {
    pub fn from_home_dir() -> Result<Self, StorageError> {
        let home = env::var_os("HOME").ok_or(StorageError::HomeDirUnavailable)?;
        Ok(Self::from_root(PathBuf::from(home).join(".klaw")))
    }

    pub fn from_root(root_dir: PathBuf) -> Self {
        Self {
            db_path: root_dir.join("klaw.db"),
            memory_db_path: root_dir.join("memory.db"),
            archive_db_path: root_dir.join("archive.db"),
            tmp_dir: root_dir.join("tmp"),
            sessions_dir: root_dir.join("sessions"),
            archives_dir: root_dir.join("archives"),
            root_dir,
        }
    }

    pub async fn ensure_dirs(&self) -> Result<(), StorageError> {
        fs::create_dir_all(&self.root_dir)
            .await
            .map_err(StorageError::CreateDataDir)?;
        fs::create_dir_all(&self.tmp_dir)
            .await
            .map_err(StorageError::CreateTmpDir)?;
        fs::create_dir_all(&self.sessions_dir)
            .await
            .map_err(StorageError::CreateSessionsDir)?;
        fs::create_dir_all(&self.archives_dir)
            .await
            .map_err(StorageError::CreateArchivesDir)?;
        Ok(())
    }
}
