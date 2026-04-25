use crate::StorageError;
use klaw_util::{
    archive_db_path, archives_dir, db_path, default_data_dir, knowledge_db_path, logs_dir,
    memory_db_path, sessions_dir, skills_dir, skills_registry_dir, tmp_dir, workspace_dir,
};
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Clone)]
pub struct StoragePaths {
    pub root_dir: PathBuf,
    pub db_path: PathBuf,
    pub memory_db_path: PathBuf,
    pub knowledge_db_path: PathBuf,
    pub archive_db_path: PathBuf,
    pub tmp_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub archives_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub skills_registry_dir: PathBuf,
}

impl StoragePaths {
    pub fn from_home_dir() -> Result<Self, StorageError> {
        let root_dir = default_data_dir().ok_or(StorageError::HomeDirUnavailable)?;
        Ok(Self::from_root(root_dir))
    }

    pub fn from_root(root_dir: PathBuf) -> Self {
        Self {
            db_path: db_path(&root_dir),
            memory_db_path: memory_db_path(&root_dir),
            knowledge_db_path: knowledge_db_path(&root_dir),
            archive_db_path: archive_db_path(&root_dir),
            tmp_dir: tmp_dir(&root_dir),
            workspace_dir: workspace_dir(&root_dir),
            sessions_dir: sessions_dir(&root_dir),
            archives_dir: archives_dir(&root_dir),
            logs_dir: logs_dir(&root_dir),
            skills_dir: skills_dir(&root_dir),
            skills_registry_dir: skills_registry_dir(&root_dir),
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
        fs::create_dir_all(&self.workspace_dir)
            .await
            .map_err(StorageError::CreateDataDir)?;
        fs::create_dir_all(&self.sessions_dir)
            .await
            .map_err(StorageError::CreateSessionsDir)?;
        fs::create_dir_all(&self.archives_dir)
            .await
            .map_err(StorageError::CreateArchivesDir)?;
        fs::create_dir_all(&self.logs_dir)
            .await
            .map_err(StorageError::CreateDataDir)?;
        fs::create_dir_all(&self.skills_dir)
            .await
            .map_err(StorageError::CreateDataDir)?;
        fs::create_dir_all(&self.skills_registry_dir)
            .await
            .map_err(StorageError::CreateDataDir)?;
        Ok(())
    }
}
