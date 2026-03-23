use crate::{StorageError, StoragePaths};
use async_trait::async_trait;
use futures_util::TryStreamExt;
use opendal::services::S3;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tar::{Archive, Builder as TarBuilder};
use time::OffsetDateTime;
use tokio::fs;
use uuid::Uuid;
use walkdir::WalkDir;
use zstd::stream::{Decoder as ZstdDecoder, Encoder as ZstdEncoder};

const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    SnapshotPrimary,
}

impl Default for SnapshotMode {
    fn default() -> Self {
        Self::SnapshotPrimary
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BackupItem {
    Session,
    Skills,
    Mcp,
    SkillsRegistry,
    GuiSettings,
    Archive,
    UserWorkspace,
    Memory,
    Config,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotSchedule {
    #[serde(default)]
    pub auto_backup: bool,
    #[serde(default = "default_snapshot_interval_minutes")]
    pub interval_minutes: u32,
}

impl Default for SnapshotSchedule {
    fn default() -> Self {
        Self {
            auto_backup: false,
            interval_minutes: default_snapshot_interval_minutes(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupPlan {
    #[serde(default)]
    pub mode: SnapshotMode,
    #[serde(default = "default_backup_items")]
    pub items: Vec<BackupItem>,
}

impl Default for BackupPlan {
    fn default() -> Self {
        Self {
            mode: SnapshotMode::SnapshotPrimary,
            items: default_backup_items(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3SnapshotStoreConfig {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_s3_region")]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub session_token: String,
    #[serde(default = "default_access_key_env")]
    pub access_key_env: String,
    #[serde(default = "default_secret_key_env")]
    pub secret_key_env: String,
    #[serde(default)]
    pub session_token_env: String,
    #[serde(default)]
    pub force_path_style: bool,
}

impl Default for S3SnapshotStoreConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            region: default_s3_region(),
            bucket: String::new(),
            prefix: String::new(),
            access_key: String::new(),
            secret_key: String::new(),
            session_token: String::new(),
            access_key_env: default_access_key_env(),
            secret_key_env: default_secret_key_env(),
            session_token_env: String::new(),
            force_path_style: false,
        }
    }
}

impl S3SnapshotStoreConfig {
    pub fn validate(&self) -> Result<(), StorageError> {
        if self.bucket.trim().is_empty() {
            return Err(StorageError::backend("sync.s3.bucket cannot be empty"));
        }
        if self.region.trim().is_empty() {
            return Err(StorageError::backend("sync.s3.region cannot be empty"));
        }
        let access_key = self.access_key.trim();
        let secret_key = self.secret_key.trim();
        if access_key.is_empty() ^ secret_key.is_empty() {
            return Err(StorageError::backend(
                "sync.s3 access_key and secret_key must both be set or both be empty",
            ));
        }
        let access = self.access_key_env.trim();
        let secret = self.secret_key_env.trim();
        if access.is_empty() ^ secret.is_empty() {
            return Err(StorageError::backend(
                "sync.s3 access_key_env and secret_key_env must both be set or both be empty",
            ));
        }
        if !self.endpoint.trim().is_empty() && access_key.is_empty() && secret_key.is_empty() {
            if access.is_empty() && secret.is_empty() {
                return Err(StorageError::backend(
                    "sync.s3 custom endpoint requires explicit credentials via access_key/secret_key or access_key_env/secret_key_env; R2 does not use AWS shared profile files",
                ));
            }

            let missing_envs = [access, secret]
                .into_iter()
                .filter(|name| std::env::var_os(name).is_none())
                .collect::<Vec<_>>();
            if !missing_envs.is_empty() {
                return Err(StorageError::backend(format!(
                    "sync.s3 custom endpoint requires credential env vars to exist: {}; otherwise set access_key and secret_key directly",
                    missing_envs.join(", ")
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub created_at: i64,
    pub device_id: String,
    pub app_version: String,
    pub mode: SnapshotMode,
    pub included_items: Vec<BackupItem>,
    pub entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Clone)]
pub struct SnapshotPrepareResult {
    pub manifest: SnapshotManifest,
    pub snapshot_dir: PathBuf,
    pub bundle_path: PathBuf,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BackupResult {
    pub snapshot_id: String,
    pub manifest: SnapshotManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupProgressStage {
    PreparingSnapshot,
    UploadingManifest,
    UploadingBundle,
    UpdatingLatestPointer,
    CleaningUpRemote,
    Completed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BackupProgress {
    pub stage: BackupProgressStage,
    pub fraction: f32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotListItem {
    pub snapshot_id: String,
    pub created_at: i64,
    pub device_id: String,
    pub app_version: String,
    pub mode: SnapshotMode,
    pub included_items: Vec<BackupItem>,
}

#[derive(Debug, Clone)]
pub struct SnapshotRestoreResult {
    pub snapshot_id: String,
    pub restored_paths: Vec<PathBuf>,
}

#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn put_bytes(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), StorageError>;
    async fn get_bytes(&self, key: &str) -> Result<Vec<u8>, StorageError>;
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
}

#[async_trait]
pub trait DatabaseSnapshotExporter: Send + Sync {
    async fn export_snapshot(
        &self,
        source_path: &Path,
        target_path: &Path,
    ) -> Result<bool, StorageError>;
}

pub struct BackupService {
    paths: StoragePaths,
    store: Arc<dyn SnapshotStore>,
    exporter: Arc<dyn DatabaseSnapshotExporter>,
    device_id: String,
    app_version: String,
}

impl BackupService {
    pub async fn open_s3_default(
        config: S3SnapshotStoreConfig,
        device_id: impl Into<String>,
    ) -> Result<Self, StorageError> {
        let paths = StoragePaths::from_home_dir()?;
        Self::open_s3(paths, config, device_id).await
    }

    pub async fn open_s3(
        paths: StoragePaths,
        config: S3SnapshotStoreConfig,
        device_id: impl Into<String>,
    ) -> Result<Self, StorageError> {
        let store = Arc::new(S3SnapshotStore::new(config).await?);
        Ok(Self::with_store(
            paths,
            store,
            Arc::new(DefaultDatabaseSnapshotExporter),
            device_id,
        ))
    }

    pub fn with_store(
        paths: StoragePaths,
        store: Arc<dyn SnapshotStore>,
        exporter: Arc<dyn DatabaseSnapshotExporter>,
        device_id: impl Into<String>,
    ) -> Self {
        Self {
            paths,
            store,
            exporter,
            device_id: normalize_device_id(device_id.into()),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub async fn create_and_upload_snapshot(
        &self,
        plan: &BackupPlan,
    ) -> Result<BackupResult, StorageError> {
        let mut noop = |_| {};
        let prepared = self.create_snapshot_with_progress(plan, &mut noop).await?;
        let result = self
            .upload_snapshot_with_progress(&prepared, &mut noop)
            .await?;
        let _ = fs::remove_dir_all(&prepared.snapshot_dir).await;
        Ok(result)
    }

    pub async fn create_upload_and_cleanup_snapshot(
        &self,
        plan: &BackupPlan,
        keep_last: u32,
    ) -> Result<BackupResult, StorageError> {
        let mut noop = |_| {};
        self.create_upload_and_cleanup_snapshot_with_progress(plan, keep_last, &mut noop)
            .await
    }

    pub async fn create_upload_and_cleanup_snapshot_with_progress<F>(
        &self,
        plan: &BackupPlan,
        keep_last: u32,
        progress: &mut F,
    ) -> Result<BackupResult, StorageError>
    where
        F: FnMut(BackupProgress),
    {
        let prepared = self.create_snapshot_with_progress(plan, progress).await?;
        let result = self
            .upload_snapshot_with_progress(&prepared, progress)
            .await?;
        report_progress(
            progress,
            BackupProgressStage::CleaningUpRemote,
            0.98,
            format!("Keeping the latest {} remote snapshot(s)", keep_last.max(1)),
        );
        self.cleanup_remote_snapshots(keep_last).await?;
        report_progress(
            progress,
            BackupProgressStage::Completed,
            1.0,
            format!("Snapshot {} uploaded successfully", result.snapshot_id),
        );
        Ok(result)
    }

    pub async fn create_snapshot(
        &self,
        plan: &BackupPlan,
    ) -> Result<SnapshotPrepareResult, StorageError> {
        let mut noop = |_| {};
        self.create_snapshot_with_progress(plan, &mut noop).await
    }

    pub async fn create_snapshot_with_progress<F>(
        &self,
        plan: &BackupPlan,
        progress: &mut F,
    ) -> Result<SnapshotPrepareResult, StorageError>
    where
        F: FnMut(BackupProgress),
    {
        self.paths.ensure_dirs().await?;

        let snapshot_id = build_snapshot_id();
        let snapshot_dir = self
            .paths
            .tmp_dir
            .join("snapshots")
            .join(snapshot_id.as_str());
        let db_dir = snapshot_dir.join("db");
        let files_dir = snapshot_dir.join("files");
        fs::create_dir_all(&db_dir)
            .await
            .map_err(StorageError::CreateTmpDir)?;
        fs::create_dir_all(&files_dir)
            .await
            .map_err(StorageError::CreateTmpDir)?;

        let sources = self.resolve_sources(plan)?;
        let total_sources = sources.len();
        report_progress(
            progress,
            BackupProgressStage::PreparingSnapshot,
            0.05,
            if total_sources == 0 {
                "No local files matched the selected backup scope".to_string()
            } else {
                format!("Preparing {total_sources} snapshot item(s)")
            },
        );

        let mut entries = Vec::new();
        for (index, source) in sources.into_iter().enumerate() {
            match source {
                ResolvedLocalSource::Database {
                    source_path,
                    relative_path,
                } => {
                    let dest = snapshot_dir.join(&relative_path);
                    if let Some(parent) = dest.parent() {
                        fs::create_dir_all(parent)
                            .await
                            .map_err(StorageError::CreateTmpDir)?;
                    }
                    let exported = self.exporter.export_snapshot(&source_path, &dest).await?;
                    if !exported {
                        continue;
                    }
                    entries.push(build_entry(&snapshot_dir, &dest).await?);
                    report_preparing_progress(progress, index + 1, total_sources, &relative_path);
                }
                ResolvedLocalSource::File {
                    source_path,
                    relative_path,
                } => {
                    let dest = snapshot_dir.join(&relative_path);
                    if let Some(parent) = dest.parent() {
                        fs::create_dir_all(parent)
                            .await
                            .map_err(StorageError::CreateTmpDir)?;
                    }
                    fs::copy(&source_path, &dest)
                        .await
                        .map_err(StorageError::backend)?;
                    entries.push(build_entry(&snapshot_dir, &dest).await?);
                    report_preparing_progress(progress, index + 1, total_sources, &relative_path);
                }
            }
        }

        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let manifest = SnapshotManifest {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            snapshot_id: snapshot_id.clone(),
            created_at: OffsetDateTime::now_utc().unix_timestamp() * 1000,
            device_id: self.device_id.clone(),
            app_version: self.app_version.clone(),
            mode: plan.mode,
            included_items: dedup_items(&plan.items),
            entries,
        };

        let manifest_path = snapshot_dir.join("manifest.json");
        write_json(&manifest_path, &manifest).await?;
        report_progress(
            progress,
            BackupProgressStage::PreparingSnapshot,
            0.72,
            "Writing snapshot manifest".to_string(),
        );
        let bundle_path = snapshot_dir.join("bundle.tar.zst");
        create_bundle(&snapshot_dir, &bundle_path).await?;
        report_progress(
            progress,
            BackupProgressStage::PreparingSnapshot,
            0.8,
            "Compressing snapshot bundle".to_string(),
        );

        Ok(SnapshotPrepareResult {
            manifest,
            snapshot_dir,
            bundle_path,
            manifest_path,
        })
    }

    pub async fn upload_snapshot(
        &self,
        prepared: &SnapshotPrepareResult,
    ) -> Result<BackupResult, StorageError> {
        let mut noop = |_| {};
        self.upload_snapshot_with_progress(prepared, &mut noop)
            .await
    }

    pub async fn upload_snapshot_with_progress<F>(
        &self,
        prepared: &SnapshotPrepareResult,
        progress: &mut F,
    ) -> Result<BackupResult, StorageError>
    where
        F: FnMut(BackupProgress),
    {
        let manifest_bytes = fs::read(&prepared.manifest_path)
            .await
            .map_err(StorageError::backend)?;
        let bundle_bytes = fs::read(&prepared.bundle_path)
            .await
            .map_err(StorageError::backend)?;
        let manifest_key = manifest_object_key(&prepared.manifest.snapshot_id);
        let bundle_key = bundle_object_key(&prepared.manifest.snapshot_id);
        report_progress(
            progress,
            BackupProgressStage::UploadingManifest,
            0.86,
            format!("Uploading {}", manifest_key),
        );
        self.store
            .put_bytes(&manifest_key, manifest_bytes, "application/json")
            .await?;
        report_progress(
            progress,
            BackupProgressStage::UploadingBundle,
            0.94,
            format!("Uploading {}", bundle_key),
        );
        self.store
            .put_bytes(&bundle_key, bundle_bytes, "application/zstd")
            .await?;
        let latest = SnapshotListItem::from_manifest(&prepared.manifest);
        report_progress(
            progress,
            BackupProgressStage::UpdatingLatestPointer,
            0.97,
            "Updating latest.json".to_string(),
        );
        self.store
            .put_bytes(
                &latest_object_key(),
                serde_json::to_vec_pretty(&latest).map_err(StorageError::backend)?,
                "application/json",
            )
            .await?;
        Ok(BackupResult {
            snapshot_id: prepared.manifest.snapshot_id.clone(),
            manifest: prepared.manifest.clone(),
        })
    }

    pub async fn list_remote_snapshots(&self) -> Result<Vec<SnapshotListItem>, StorageError> {
        let keys = self.store.list_keys(&snapshots_prefix()).await?;
        let mut items = Vec::new();
        for key in keys {
            if !key.ends_with("/manifest.json") {
                continue;
            }
            let bytes = self.store.get_bytes(&key).await?;
            let manifest: SnapshotManifest =
                serde_json::from_slice(&bytes).map_err(StorageError::backend)?;
            items.push(SnapshotListItem::from_manifest(&manifest));
        }
        items.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.snapshot_id.cmp(&left.snapshot_id))
        });
        Ok(items)
    }

    pub async fn cleanup_remote_snapshots(&self, keep_last: u32) -> Result<(), StorageError> {
        let keep_last = keep_last.max(1) as usize;
        let snapshots = self.list_remote_snapshots().await?;
        for snapshot in snapshots.into_iter().skip(keep_last) {
            self.store
                .delete(&manifest_object_key(&snapshot.snapshot_id))
                .await?;
            self.store
                .delete(&bundle_object_key(&snapshot.snapshot_id))
                .await?;
        }
        self.refresh_latest_snapshot_pointer().await?;
        Ok(())
    }

    pub async fn refresh_latest_snapshot_pointer(&self) -> Result<(), StorageError> {
        let newest = self.list_remote_snapshots().await?.into_iter().next();
        match newest {
            Some(snapshot) => {
                self.store
                    .put_bytes(
                        &latest_object_key(),
                        serde_json::to_vec_pretty(&snapshot).map_err(StorageError::backend)?,
                        "application/json",
                    )
                    .await?;
            }
            None => {
                self.store.delete(&latest_object_key()).await?;
            }
        }
        Ok(())
    }

    pub async fn restore_snapshot(
        &self,
        snapshot_id: &str,
    ) -> Result<SnapshotRestoreResult, StorageError> {
        self.paths.ensure_dirs().await?;
        let manifest_bytes = self
            .store
            .get_bytes(&manifest_object_key(snapshot_id))
            .await?;
        let manifest: SnapshotManifest =
            serde_json::from_slice(&manifest_bytes).map_err(StorageError::backend)?;
        let bundle_bytes = self
            .store
            .get_bytes(&bundle_object_key(snapshot_id))
            .await?;

        let restore_dir = self
            .paths
            .tmp_dir
            .join("snapshot-restore")
            .join(snapshot_id);
        if path_exists(&restore_dir).await {
            let _ = fs::remove_dir_all(&restore_dir).await;
        }
        fs::create_dir_all(&restore_dir)
            .await
            .map_err(StorageError::CreateTmpDir)?;

        let bundle_path = restore_dir.join("bundle.tar.zst");
        fs::write(&bundle_path, bundle_bytes)
            .await
            .map_err(StorageError::backend)?;
        extract_bundle(&bundle_path, &restore_dir).await?;
        verify_manifest_entries(&restore_dir, &manifest).await?;

        let restored_paths = self.apply_restore(&restore_dir, &manifest).await?;
        let _ = fs::remove_dir_all(&restore_dir).await;
        Ok(SnapshotRestoreResult {
            snapshot_id: snapshot_id.to_string(),
            restored_paths,
        })
    }

    fn collect_sources(&self, plan: &BackupPlan) -> Vec<LocalSource> {
        let mut sources = Vec::new();
        let root = &self.paths.root_dir;
        for item in dedup_items(&plan.items) {
            match item {
                BackupItem::Session => {
                    sources.push(LocalSource::Database {
                        source_path: self.paths.db_path.clone(),
                        relative_path: PathBuf::from("db/klaw.db"),
                    });
                    sources.push(LocalSource::Directory {
                        source_dir: self.paths.sessions_dir.clone(),
                        relative_root: PathBuf::from("files/sessions"),
                    });
                }
                BackupItem::Memory => {
                    sources.push(LocalSource::Database {
                        source_path: self.paths.memory_db_path.clone(),
                        relative_path: PathBuf::from("db/memory.db"),
                    });
                }
                BackupItem::Archive => {
                    sources.push(LocalSource::Database {
                        source_path: self.paths.archive_db_path.clone(),
                        relative_path: PathBuf::from("db/archive.db"),
                    });
                    sources.push(LocalSource::Directory {
                        source_dir: self.paths.archives_dir.clone(),
                        relative_root: PathBuf::from("files/archives"),
                    });
                }
                BackupItem::Config => {
                    sources.push(LocalSource::File {
                        source_path: klaw_util::config_path(root),
                        relative_path: PathBuf::from("files/config.toml"),
                    });
                }
                BackupItem::GuiSettings => {
                    sources.push(LocalSource::File {
                        source_path: klaw_util::settings_path(root),
                        relative_path: PathBuf::from("files/settings.json"),
                    });
                    sources.push(LocalSource::File {
                        source_path: klaw_util::gui_state_path(root),
                        relative_path: PathBuf::from("files/gui_state.json"),
                    });
                }
                BackupItem::Skills => {
                    sources.push(LocalSource::Directory {
                        source_dir: self.paths.skills_dir.clone(),
                        relative_root: PathBuf::from("files/skills"),
                    });
                }
                BackupItem::SkillsRegistry => {
                    sources.push(LocalSource::Directory {
                        source_dir: self.paths.skills_registry_dir.clone(),
                        relative_root: PathBuf::from("files/skills-registry"),
                    });
                    sources.push(LocalSource::File {
                        source_path: klaw_util::skills_registry_manifest_path(root),
                        relative_path: PathBuf::from("files/skills-registry-manifest.json"),
                    });
                }
                BackupItem::UserWorkspace => {
                    sources.push(LocalSource::Directory {
                        source_dir: self.paths.workspace_dir.clone(),
                        relative_root: PathBuf::from("files/workspace"),
                    });
                }
                BackupItem::Mcp => {}
            }
        }
        sources
    }

    fn resolve_sources(&self, plan: &BackupPlan) -> Result<Vec<ResolvedLocalSource>, StorageError> {
        let mut resolved = Vec::new();
        for source in self.collect_sources(plan) {
            match source {
                LocalSource::Database {
                    source_path,
                    relative_path,
                } => {
                    if source_path.exists() {
                        resolved.push(ResolvedLocalSource::Database {
                            source_path,
                            relative_path,
                        });
                    }
                }
                LocalSource::File {
                    source_path,
                    relative_path,
                } => {
                    if source_path.exists() {
                        resolved.push(ResolvedLocalSource::File {
                            source_path,
                            relative_path,
                        });
                    }
                }
                LocalSource::Directory {
                    source_dir,
                    relative_root,
                } => {
                    if !source_dir.exists() {
                        continue;
                    }
                    for file_path in walk_files(&source_dir)? {
                        let rel = file_path
                            .strip_prefix(&source_dir)
                            .map_err(StorageError::backend)?
                            .to_path_buf();
                        resolved.push(ResolvedLocalSource::File {
                            source_path: file_path,
                            relative_path: relative_root.join(&rel),
                        });
                    }
                }
            }
        }
        Ok(resolved)
    }

    async fn apply_restore(
        &self,
        restore_dir: &Path,
        manifest: &SnapshotManifest,
    ) -> Result<Vec<PathBuf>, StorageError> {
        let mut file_targets = BTreeSet::new();
        let mut dir_targets = BTreeSet::new();
        for entry in &manifest.entries {
            let target = snapshot_relative_to_root(&self.paths.root_dir, &entry.relative_path)?;
            if let Some(dir) = restore_root_dir(&self.paths, &entry.relative_path) {
                dir_targets.insert(dir);
            } else {
                file_targets.insert(target);
            }
        }

        for dir in &dir_targets {
            if path_exists(dir).await {
                fs::remove_dir_all(dir)
                    .await
                    .map_err(StorageError::backend)?;
            }
        }
        for file in &file_targets {
            remove_file_and_sidecars(file).await?;
        }

        let mut restored = BTreeSet::new();
        for entry in &manifest.entries {
            let source = restore_dir.join(&entry.relative_path);
            let target = snapshot_relative_to_root(&self.paths.root_dir, &entry.relative_path)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(StorageError::CreateDataDir)?;
            }
            fs::copy(&source, &target)
                .await
                .map_err(StorageError::backend)?;
            restored.insert(target);
        }
        Ok(restored.into_iter().collect())
    }
}

impl SnapshotListItem {
    fn from_manifest(manifest: &SnapshotManifest) -> Self {
        Self {
            snapshot_id: manifest.snapshot_id.clone(),
            created_at: manifest.created_at,
            device_id: manifest.device_id.clone(),
            app_version: manifest.app_version.clone(),
            mode: manifest.mode,
            included_items: manifest.included_items.clone(),
        }
    }
}

#[derive(Debug)]
pub struct S3SnapshotStore {
    operator: Operator,
    config: S3SnapshotStoreConfig,
}

impl S3SnapshotStore {
    pub async fn new(config: S3SnapshotStoreConfig) -> Result<Self, StorageError> {
        config.validate()?;

        let mut builder = S3::default()
            .root("/")
            .bucket(&config.bucket)
            .region(&config.region);
        if !config.endpoint.trim().is_empty() {
            builder = builder.endpoint(&config.endpoint);
        }

        let (access_key, secret_key) = resolve_s3_credentials(&config)?;
        if let (Some(access_key), Some(secret_key)) = (access_key, secret_key) {
            builder = builder
                .access_key_id(&access_key)
                .secret_access_key(&secret_key);
        }

        if let Some(session_token) = resolve_s3_session_token(&config)? {
            builder = builder.session_token(&session_token);
        }

        if !config.force_path_style {
            builder = builder.enable_virtual_host_style();
        }

        let operator = Operator::new(builder)
            .map_err(StorageError::backend)?
            .finish();
        Ok(Self { operator, config })
    }

    fn key(&self, key: &str) -> String {
        let prefix = self.config.prefix.trim_matches('/');
        if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}/{key}")
        }
    }
}

#[async_trait]
impl SnapshotStore for S3SnapshotStore {
    async fn put_bytes(
        &self,
        key: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<(), StorageError> {
        self.operator
            .write_with(&self.key(key), bytes)
            .content_type(content_type)
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }

    async fn get_bytes(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        let response = self
            .operator
            .read(&self.key(key))
            .await
            .map_err(StorageError::backend)?;
        Ok(response.to_vec())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let prefix = self.key(prefix);
        let trim_prefix = self.config.prefix.trim_matches('/').to_string();
        let mut keys = Vec::new();
        let mut lister = self
            .operator
            .lister(&prefix)
            .await
            .map_err(StorageError::backend)?;
        while let Some(entry) = lister.try_next().await.map_err(StorageError::backend)? {
            let key = entry.path().to_string();
            if trim_prefix.is_empty() {
                keys.push(key);
            } else if let Some(stripped) = key.strip_prefix(&format!("{trim_prefix}/")) {
                keys.push(stripped.to_string());
            }
        }
        Ok(keys)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.operator
            .delete(&self.key(key))
            .await
            .map_err(StorageError::backend)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct DefaultDatabaseSnapshotExporter;

#[async_trait]
impl DatabaseSnapshotExporter for DefaultDatabaseSnapshotExporter {
    async fn export_snapshot(
        &self,
        source_path: &Path,
        target_path: &Path,
    ) -> Result<bool, StorageError> {
        if !path_exists(source_path).await {
            return Ok(false);
        }
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(StorageError::CreateTmpDir)?;
        }
        #[cfg(feature = "sqlx")]
        if export_sqlite_snapshot_sqlx(source_path, target_path)
            .await
            .is_ok()
        {
            return Ok(true);
        }
        #[cfg(feature = "turso")]
        if export_sqlite_snapshot_turso(source_path, target_path)
            .await
            .is_ok()
        {
            return Ok(true);
        }

        fallback_copy_sqlite(source_path, target_path).await?;
        Ok(true)
    }
}

#[cfg(feature = "sqlx")]
async fn export_sqlite_snapshot_sqlx(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), StorageError> {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::time::Duration;

    let source = source_path.to_path_buf();
    let target = target_path.to_path_buf();
    let escaped = escape_sqlite_path(&target);
    let options = SqliteConnectOptions::new()
        .filename(&source)
        .create_if_missing(false)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(StorageError::backend)?;
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&pool)
        .await
        .map_err(StorageError::backend)?;
    let _ = fs::remove_file(&target).await;
    sqlx::query(&format!("VACUUM INTO '{escaped}'"))
        .execute(&pool)
        .await
        .map_err(StorageError::backend)?;
    pool.close().await;
    Ok(())
}

#[cfg(feature = "turso")]
async fn export_sqlite_snapshot_turso(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), StorageError> {
    use turso::Builder;

    let source = source_path.to_string_lossy().to_string();
    let target = target_path.to_path_buf();
    let escaped = escape_sqlite_path(&target);
    let db = Builder::new_local(&source)
        .build()
        .await
        .map_err(StorageError::backend)?;
    let conn = db.connect().map_err(StorageError::backend)?;
    conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", ())
        .await
        .map_err(StorageError::backend)?;
    let _ = fs::remove_file(&target).await;
    conn.execute(&format!("VACUUM INTO '{escaped}'"), ())
        .await
        .map_err(StorageError::backend)?;
    Ok(())
}

async fn fallback_copy_sqlite(source_path: &Path, target_path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(StorageError::CreateTmpDir)?;
    }
    let _ = fs::remove_file(target_path).await;
    fs::copy(source_path, target_path)
        .await
        .map_err(StorageError::backend)?;
    Ok(())
}

#[derive(Debug, Clone)]
enum LocalSource {
    Database {
        source_path: PathBuf,
        relative_path: PathBuf,
    },
    File {
        source_path: PathBuf,
        relative_path: PathBuf,
    },
    Directory {
        source_dir: PathBuf,
        relative_root: PathBuf,
    },
}

#[derive(Debug, Clone)]
enum ResolvedLocalSource {
    Database {
        source_path: PathBuf,
        relative_path: PathBuf,
    },
    File {
        source_path: PathBuf,
        relative_path: PathBuf,
    },
}

fn default_snapshot_interval_minutes() -> u32 {
    60
}

fn default_backup_items() -> Vec<BackupItem> {
    vec![
        BackupItem::Session,
        BackupItem::Memory,
        BackupItem::Archive,
        BackupItem::Config,
        BackupItem::GuiSettings,
        BackupItem::Skills,
        BackupItem::UserWorkspace,
    ]
}

fn default_s3_region() -> String {
    "us-east-1".to_string()
}

fn default_access_key_env() -> String {
    "AWS_ACCESS_KEY_ID".to_string()
}

fn default_secret_key_env() -> String {
    "AWS_SECRET_ACCESS_KEY".to_string()
}

fn build_snapshot_id() -> String {
    format!(
        "{}-{}",
        OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "snapshot".to_string())
            .replace(':', "-"),
        Uuid::new_v4().simple()
    )
}

fn normalize_device_id(device_id: String) -> String {
    let trimmed = device_id.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("device-{}", Uuid::new_v4().simple()))
}

fn resolve_s3_credentials(
    config: &S3SnapshotStoreConfig,
) -> Result<(Option<String>, Option<String>), StorageError> {
    let direct_access_key = config.access_key.trim();
    let direct_secret_key = config.secret_key.trim();
    if !direct_access_key.is_empty() && !direct_secret_key.is_empty() {
        return Ok((
            Some(direct_access_key.to_string()),
            Some(direct_secret_key.to_string()),
        ));
    }

    let access_key_env = config.access_key_env.trim();
    let secret_key_env = config.secret_key_env.trim();
    if access_key_env.is_empty() && secret_key_env.is_empty() {
        return Ok((None, None));
    }

    let access_key = std::env::var(access_key_env).map_err(StorageError::backend)?;
    let secret_key = std::env::var(secret_key_env).map_err(StorageError::backend)?;
    Ok((Some(access_key), Some(secret_key)))
}

fn resolve_s3_session_token(
    config: &S3SnapshotStoreConfig,
) -> Result<Option<String>, StorageError> {
    let direct_session_token = config.session_token.trim();
    if !direct_session_token.is_empty() {
        return Ok(Some(direct_session_token.to_string()));
    }

    let session_token_env = config.session_token_env.trim();
    if session_token_env.is_empty() {
        return Ok(None);
    }

    let session_token = std::env::var(session_token_env).map_err(StorageError::backend)?;
    Ok(Some(session_token))
}

fn dedup_items(items: &[BackupItem]) -> Vec<BackupItem> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in items.iter().copied() {
        if seen.insert(item) {
            out.push(item);
        }
    }
    out
}

fn report_progress<F>(progress: &mut F, stage: BackupProgressStage, fraction: f32, detail: String)
where
    F: FnMut(BackupProgress),
{
    progress(BackupProgress {
        stage,
        fraction: fraction.clamp(0.0, 1.0),
        detail,
    });
}

fn report_preparing_progress<F>(
    progress: &mut F,
    completed: usize,
    total: usize,
    relative_path: &Path,
) where
    F: FnMut(BackupProgress),
{
    let fraction = if total == 0 {
        0.7
    } else {
        0.1 + (completed as f32 / total as f32) * 0.6
    };
    report_progress(
        progress,
        BackupProgressStage::PreparingSnapshot,
        fraction,
        format!(
            "Prepared {completed}/{total}: {}",
            relative_path.to_string_lossy().replace('\\', "/")
        ),
    );
}

async fn path_exists(path: &Path) -> bool {
    fs::metadata(path).await.is_ok()
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>, StorageError> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root) {
        let entry = entry.map_err(StorageError::backend)?;
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

async fn build_entry(snapshot_dir: &Path, file_path: &Path) -> Result<SnapshotEntry, StorageError> {
    let relative_path = file_path
        .strip_prefix(snapshot_dir)
        .map_err(StorageError::backend)?
        .to_string_lossy()
        .replace('\\', "/");
    let metadata = fs::metadata(file_path)
        .await
        .map_err(StorageError::backend)?;
    let sha256 = hash_file(file_path).await?;
    Ok(SnapshotEntry {
        relative_path,
        size_bytes: metadata.len(),
        sha256,
    })
}

async fn hash_file(path: &Path) -> Result<String, StorageError> {
    let bytes = fs::read(path).await.map_err(StorageError::backend)?;
    let digest = Sha256::digest(&bytes);
    Ok(hex::encode(digest))
}

async fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(StorageError::backend)?;
    fs::write(path, bytes)
        .await
        .map_err(StorageError::backend)?;
    Ok(())
}

async fn create_bundle(snapshot_dir: &Path, bundle_path: &Path) -> Result<(), StorageError> {
    let snapshot_dir = snapshot_dir.to_path_buf();
    let bundle_path = bundle_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
        let bundle_file = std::fs::File::create(&bundle_path).map_err(StorageError::backend)?;
        let encoder = ZstdEncoder::new(bundle_file, 3).map_err(StorageError::backend)?;
        let mut tar = TarBuilder::new(encoder.auto_finish());
        tar.append_dir_all("db", snapshot_dir.join("db"))
            .map_err(StorageError::backend)?;
        tar.append_dir_all("files", snapshot_dir.join("files"))
            .map_err(StorageError::backend)?;
        tar.append_path_with_name(snapshot_dir.join("manifest.json"), "manifest.json")
            .map_err(StorageError::backend)?;
        tar.finish().map_err(StorageError::backend)?;
        Ok(())
    })
    .await
    .map_err(StorageError::backend)?
}

async fn extract_bundle(bundle_path: &Path, output_dir: &Path) -> Result<(), StorageError> {
    let bundle_path = bundle_path.to_path_buf();
    let output_dir = output_dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), StorageError> {
        let bundle_file = std::fs::File::open(&bundle_path).map_err(StorageError::backend)?;
        let decoder = ZstdDecoder::new(bundle_file).map_err(StorageError::backend)?;
        let mut archive = Archive::new(decoder);
        archive.unpack(&output_dir).map_err(StorageError::backend)?;
        Ok(())
    })
    .await
    .map_err(StorageError::backend)?
}

async fn verify_manifest_entries(
    restore_dir: &Path,
    manifest: &SnapshotManifest,
) -> Result<(), StorageError> {
    for entry in &manifest.entries {
        let file_path = restore_dir.join(&entry.relative_path);
        let actual_hash = hash_file(&file_path).await?;
        if actual_hash != entry.sha256 {
            return Err(StorageError::backend(format!(
                "snapshot verification failed for {}",
                entry.relative_path
            )));
        }
    }
    Ok(())
}

fn manifest_object_key(snapshot_id: &str) -> String {
    format!("snapshots/{snapshot_id}/manifest.json")
}

fn bundle_object_key(snapshot_id: &str) -> String {
    format!("snapshots/{snapshot_id}/bundle.tar.zst")
}

fn latest_object_key() -> String {
    "latest.json".to_string()
}

fn snapshots_prefix() -> String {
    "snapshots/".to_string()
}

fn snapshot_relative_to_root(
    root_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, StorageError> {
    let rel = Path::new(relative_path);
    if let Ok(stripped) = rel.strip_prefix("db") {
        return Ok(root_dir.join(stripped));
    }
    if let Ok(stripped) = rel.strip_prefix("files") {
        return Ok(root_dir.join(stripped));
    }
    Err(StorageError::backend(format!(
        "unsupported snapshot path: {relative_path}"
    )))
}

fn restore_root_dir(paths: &StoragePaths, relative_path: &str) -> Option<PathBuf> {
    if relative_path.starts_with("files/sessions/") {
        return Some(paths.sessions_dir.clone());
    }
    if relative_path.starts_with("files/archives/") {
        return Some(paths.archives_dir.clone());
    }
    if relative_path.starts_with("files/workspace/") {
        return Some(paths.workspace_dir.clone());
    }
    if relative_path.starts_with("files/skills/") {
        return Some(paths.skills_dir.clone());
    }
    if relative_path.starts_with("files/skills-registry/") {
        return Some(paths.skills_registry_dir.clone());
    }
    None
}

async fn remove_file_and_sidecars(path: &Path) -> Result<(), StorageError> {
    if path_exists(path).await {
        fs::remove_file(path).await.map_err(StorageError::backend)?;
    }
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        let wal = path.with_file_name(format!("{name}-wal"));
        let shm = path.with_file_name(format!("{name}-shm"));
        if path_exists(&wal).await {
            fs::remove_file(wal).await.map_err(StorageError::backend)?;
        }
        if path_exists(&shm).await {
            fs::remove_file(shm).await.map_err(StorageError::backend)?;
        }
    }
    Ok(())
}

fn escape_sqlite_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct MockStore {
        objects: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    #[async_trait]
    impl SnapshotStore for MockStore {
        async fn put_bytes(
            &self,
            key: &str,
            bytes: Vec<u8>,
            _content_type: &str,
        ) -> Result<(), StorageError> {
            self.objects
                .lock()
                .expect("objects lock")
                .insert(key.to_string(), bytes);
            Ok(())
        }

        async fn get_bytes(&self, key: &str) -> Result<Vec<u8>, StorageError> {
            self.objects
                .lock()
                .expect("objects lock")
                .get(key)
                .cloned()
                .ok_or_else(|| StorageError::backend(format!("missing mock object: {key}")))
        }

        async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
            Ok(self
                .objects
                .lock()
                .expect("objects lock")
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect())
        }

        async fn delete(&self, key: &str) -> Result<(), StorageError> {
            self.objects.lock().expect("objects lock").remove(key);
            Ok(())
        }
    }

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "klaw-backup-test-{name}-{}",
            Uuid::new_v4().simple()
        ))
    }

    async fn write_file(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.expect("create dir");
        }
        fs::write(path, body).await.expect("write file");
    }

    async fn create_service(name: &str, device_id: &str) -> BackupService {
        let paths = StoragePaths::from_root(test_root(name));
        paths.ensure_dirs().await.expect("ensure dirs");
        BackupService::with_store(
            paths,
            Arc::new(MockStore::default()),
            Arc::new(DefaultDatabaseSnapshotExporter),
            device_id.to_string(),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn snapshot_manifest_tracks_selected_items() {
        let service = create_service("manifest", "device-a").await;
        write_file(&service.paths.db_path, "db").await;
        write_file(&service.paths.sessions_dir.join("demo.jsonl"), "[]").await;
        write_file(
            &klaw_util::config_path(&service.paths.root_dir),
            "name = 'demo'",
        )
        .await;

        let plan = BackupPlan {
            mode: SnapshotMode::SnapshotPrimary,
            items: vec![BackupItem::Session, BackupItem::Config],
        };
        let prepared = service.create_snapshot(&plan).await.expect("snapshot");

        assert_eq!(
            prepared.manifest.included_items,
            vec![BackupItem::Session, BackupItem::Config]
        );
        assert!(prepared
            .manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "db/klaw.db"));
        assert!(prepared
            .manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "files/sessions/demo.jsonl"));
        assert!(prepared
            .manifest
            .entries
            .iter()
            .any(|entry| entry.relative_path == "files/config.toml"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn snapshot_entries_have_matching_hashes() {
        let service = create_service("hash", "device-a").await;
        write_file(&service.paths.db_path, "db").await;

        let prepared = service
            .create_snapshot(&BackupPlan {
                mode: SnapshotMode::SnapshotPrimary,
                items: vec![BackupItem::Session],
            })
            .await
            .expect("snapshot");

        let entry = prepared
            .manifest
            .entries
            .iter()
            .find(|entry| entry.relative_path == "db/klaw.db")
            .expect("db entry");
        let actual_hash = hash_file(&prepared.snapshot_dir.join("db/klaw.db"))
            .await
            .expect("hash");
        assert_eq!(entry.sha256, actual_hash);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restore_aborts_on_checksum_mismatch() {
        let service = create_service("restore-fail", "device-a").await;
        write_file(&service.paths.db_path, "old").await;

        let prepared = service
            .create_snapshot(&BackupPlan {
                mode: SnapshotMode::SnapshotPrimary,
                items: vec![BackupItem::Session],
            })
            .await
            .expect("snapshot");
        service.upload_snapshot(&prepared).await.expect("upload");

        let store = service.store.clone();
        let manifest_key = manifest_object_key(&prepared.manifest.snapshot_id);
        let mut manifest: SnapshotManifest =
            serde_json::from_slice(&store.get_bytes(&manifest_key).await.expect("manifest"))
                .expect("decode manifest");
        manifest.entries[0].sha256 = "deadbeef".to_string();
        store
            .put_bytes(
                &manifest_key,
                serde_json::to_vec_pretty(&manifest).expect("manifest json"),
                "application/json",
            )
            .await
            .expect("put manifest");

        let err = service
            .restore_snapshot(&prepared.manifest.snapshot_id)
            .await
            .expect_err("restore should fail");
        assert!(err.to_string().contains("verification failed"));

        let current = fs::read_to_string(&service.paths.db_path)
            .await
            .expect("read db");
        assert_eq!(current, "old");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_snapshot_list_sorts_latest_first() {
        let store = Arc::new(MockStore::default());
        let paths_a = StoragePaths::from_root(test_root("list-a"));
        paths_a.ensure_dirs().await.expect("dirs a");
        let service_a = BackupService::with_store(
            paths_a,
            store.clone(),
            Arc::new(DefaultDatabaseSnapshotExporter),
            "device-a".to_string(),
        );
        let paths_b = StoragePaths::from_root(test_root("list-b"));
        paths_b.ensure_dirs().await.expect("dirs b");
        let service_b = BackupService::with_store(
            paths_b,
            store.clone(),
            Arc::new(DefaultDatabaseSnapshotExporter),
            "device-b".to_string(),
        );

        write_file(&service_a.paths.db_path, "a").await;
        write_file(&service_b.paths.db_path, "b").await;

        let first = service_a
            .create_snapshot(&BackupPlan {
                mode: SnapshotMode::SnapshotPrimary,
                items: vec![BackupItem::Session],
            })
            .await
            .expect("first");
        service_a
            .upload_snapshot(&first)
            .await
            .expect("upload first");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = service_b
            .create_snapshot(&BackupPlan {
                mode: SnapshotMode::SnapshotPrimary,
                items: vec![BackupItem::Session],
            })
            .await
            .expect("second");
        service_b
            .upload_snapshot(&second)
            .await
            .expect("upload second");

        let listed = service_a.list_remote_snapshots().await.expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].device_id, "device-b");
        assert_eq!(listed[1].device_id, "device-a");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_remote_snapshots_keeps_only_latest_n() {
        let store = Arc::new(MockStore::default());
        let paths_a = StoragePaths::from_root(test_root("cleanup-a"));
        paths_a.ensure_dirs().await.expect("dirs a");
        let service = BackupService::with_store(
            paths_a,
            store.clone(),
            Arc::new(DefaultDatabaseSnapshotExporter),
            "device-a".to_string(),
        );

        write_file(&service.paths.db_path, "a").await;
        let first = service
            .create_snapshot(&BackupPlan {
                mode: SnapshotMode::SnapshotPrimary,
                items: vec![BackupItem::Session],
            })
            .await
            .expect("first");
        service.upload_snapshot(&first).await.expect("upload first");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = service
            .create_snapshot(&BackupPlan {
                mode: SnapshotMode::SnapshotPrimary,
                items: vec![BackupItem::Session],
            })
            .await
            .expect("second");
        service
            .upload_snapshot(&second)
            .await
            .expect("upload second");

        service
            .cleanup_remote_snapshots(1)
            .await
            .expect("cleanup snapshots");

        let listed = service.list_remote_snapshots().await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].snapshot_id, second.manifest.snapshot_id);
        assert!(!store
            .objects
            .lock()
            .expect("objects lock")
            .contains_key(&manifest_object_key(&first.manifest.snapshot_id)));
        let latest: SnapshotListItem = serde_json::from_slice(
            &store
                .get_bytes(&latest_object_key())
                .await
                .expect("latest object"),
        )
        .expect("decode latest");
        assert_eq!(latest.snapshot_id, second.manifest.snapshot_id);
    }

    #[test]
    fn s3_config_requires_bucket() {
        let config = S3SnapshotStoreConfig {
            bucket: String::new(),
            ..Default::default()
        };
        let err = config.validate().expect_err("config should fail");
        assert!(err.to_string().contains("bucket"));
    }

    #[test]
    fn s3_config_requires_direct_credentials_in_pairs() {
        let config = S3SnapshotStoreConfig {
            bucket: "demo".to_string(),
            access_key: "only-access".to_string(),
            ..Default::default()
        };
        let err = config.validate().expect_err("config should fail");
        assert!(err.to_string().contains("access_key and secret_key"));
    }

    #[test]
    fn s3_custom_endpoint_requires_explicit_credentials() {
        let config = S3SnapshotStoreConfig {
            endpoint: "https://example.r2.cloudflarestorage.com".to_string(),
            bucket: "demo".to_string(),
            ..Default::default()
        };
        let err = config.validate().expect_err("config should fail");
        assert!(err.to_string().contains("custom endpoint requires"));
    }
}
