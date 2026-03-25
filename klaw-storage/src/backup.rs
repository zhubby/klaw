use crate::{StorageError, StoragePaths};
use async_trait::async_trait;
use futures_util::TryStreamExt;
use opendal::Operator;
use opendal::services::S3;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use time::OffsetDateTime;
use tokio::fs;
use uuid::Uuid;
use walkdir::WalkDir;

const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    ManifestVersioned,
}

impl Default for SnapshotMode {
    fn default() -> Self {
        Self::ManifestVersioned
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BackupItem {
    Session,
    Skills,
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
            mode: SnapshotMode::ManifestVersioned,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManifestEntryKind {
    File,
    SqliteSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestEntry {
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub kind: ManifestEntryKind,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncManifest {
    pub schema_version: u32,
    pub manifest_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_manifest_id: Option<String>,
    pub created_at: i64,
    pub device_id: String,
    pub app_version: String,
    pub mode: SnapshotMode,
    pub included_items: Vec<BackupItem>,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LatestRef {
    pub schema_version: u32,
    pub manifest_id: String,
    pub updated_at: i64,
    pub device_id: String,
}

#[derive(Debug, Clone)]
pub struct SnapshotPrepareResult {
    pub manifest: SyncManifest,
    pub latest_ref: LatestRef,
    staging_dir: PathBuf,
    staged_files: BTreeMap<String, StagedFile>,
}

#[derive(Debug, Clone)]
pub struct BackupResult {
    pub manifest_id: String,
    pub manifest: SyncManifest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupProgressStage {
    ReconcilingRemote,
    PreparingManifest,
    UploadingBlobs,
    UploadingManifest,
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
    pub manifest_id: String,
    pub created_at: i64,
    pub device_id: String,
    pub app_version: String,
    pub mode: SnapshotMode,
    pub included_items: Vec<BackupItem>,
}

#[derive(Debug, Clone)]
pub struct SnapshotRestoreResult {
    pub manifest_id: String,
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
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
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
        self.create_upload_and_cleanup_snapshot_with_progress(plan, 1, &mut noop)
            .await
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
        self.paths.ensure_dirs().await?;
        self.reject_legacy_snapshot_layout().await?;

        let remote_manifest = self.load_latest_manifest().await?;
        report_progress(
            progress,
            BackupProgressStage::ReconcilingRemote,
            0.05,
            "Reconciling local files with remote manifest".to_string(),
        );
        if let Some(manifest) = remote_manifest.as_ref() {
            self.reconcile_local_from_remote(manifest, progress).await?;
        }

        let prepared = self
            .prepare_manifest(plan, remote_manifest.as_ref(), progress)
            .await?;
        let result = self
            .upload_prepared_manifest_with_progress(&prepared, progress)
            .await?;
        report_progress(
            progress,
            BackupProgressStage::CleaningUpRemote,
            0.96,
            format!("Keeping the latest {} remote manifest(s)", keep_last.max(1)),
        );
        self.cleanup_remote_snapshots(keep_last).await?;
        let _ = fs::remove_dir_all(&prepared.staging_dir).await;
        report_progress(
            progress,
            BackupProgressStage::Completed,
            1.0,
            format!("Manifest {} uploaded successfully", result.manifest_id),
        );
        Ok(result)
    }

    pub async fn list_remote_snapshots(&self) -> Result<Vec<SnapshotListItem>, StorageError> {
        self.reject_legacy_snapshot_layout().await?;
        let keys = self.store.list_keys(&manifests_prefix()).await?;
        let mut items = Vec::new();
        for key in keys {
            if !key.ends_with(".json") {
                continue;
            }
            let bytes = self.store.get_bytes(&key).await?;
            let manifest: SyncManifest =
                serde_json::from_slice(&bytes).map_err(StorageError::backend)?;
            items.push(SnapshotListItem::from_manifest(&manifest));
        }
        items.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.manifest_id.cmp(&left.manifest_id))
        });
        Ok(items)
    }

    pub async fn latest_remote_snapshot(&self) -> Result<Option<SnapshotListItem>, StorageError> {
        self.reject_legacy_snapshot_layout().await?;
        let Some(manifest) = self.load_latest_manifest().await? else {
            return Ok(None);
        };
        Ok(Some(SnapshotListItem::from_manifest(&manifest)))
    }

    pub async fn cleanup_remote_snapshots(&self, keep_last: u32) -> Result<(), StorageError> {
        self.reject_legacy_snapshot_layout().await?;

        let keep_last = keep_last.max(1) as usize;
        let manifests = self.load_all_remote_manifests().await?;
        let kept = manifests
            .iter()
            .take(keep_last)
            .cloned()
            .collect::<Vec<_>>();
        let removed = manifests
            .iter()
            .skip(keep_last)
            .cloned()
            .collect::<Vec<_>>();

        let mut referenced_hashes = BTreeSet::new();
        for manifest in &kept {
            for entry in manifest.entries.iter().filter(|entry| !entry.deleted) {
                referenced_hashes.insert(entry.sha256.clone());
            }
        }

        for manifest in &removed {
            self.store
                .delete(&manifest_object_key(&manifest.manifest_id))
                .await?;
        }

        let blob_keys = self.store.list_keys(&blobs_prefix()).await?;
        for key in blob_keys {
            let Some(hash) = key.strip_prefix("blobs/sha256/") else {
                continue;
            };
            if !referenced_hashes.contains(hash) {
                self.store.delete(&key).await?;
            }
        }

        match kept.first() {
            Some(manifest) => {
                let latest = LatestRef::from_manifest(manifest);
                self.store
                    .put_bytes(
                        &latest_object_key(),
                        serde_json::to_vec_pretty(&latest).map_err(StorageError::backend)?,
                        "application/json",
                    )
                    .await?;
            }
            None => {
                if self.store.exists(&latest_object_key()).await? {
                    self.store.delete(&latest_object_key()).await?;
                }
            }
        }

        Ok(())
    }

    pub async fn restore_snapshot(
        &self,
        manifest_id: &str,
    ) -> Result<SnapshotRestoreResult, StorageError> {
        self.paths.ensure_dirs().await?;
        self.reject_legacy_snapshot_layout().await?;
        let manifest = self.load_manifest(manifest_id).await?;
        self.restore_manifest(&manifest).await
    }

    async fn prepare_manifest<F>(
        &self,
        plan: &BackupPlan,
        remote_manifest: Option<&SyncManifest>,
        progress: &mut F,
    ) -> Result<SnapshotPrepareResult, StorageError>
    where
        F: FnMut(BackupProgress),
    {
        let stage_id = Uuid::new_v4().simple().to_string();
        let staging_dir = self.paths.tmp_dir.join("sync-staging").join(stage_id);
        fs::create_dir_all(&staging_dir)
            .await
            .map_err(StorageError::CreateTmpDir)?;

        let sources = self.resolve_sources(plan)?;
        let total_sources = sources.len();
        report_progress(
            progress,
            BackupProgressStage::PreparingManifest,
            0.12,
            if total_sources == 0 {
                "No local files matched the selected backup scope".to_string()
            } else {
                format!("Preparing {total_sources} staged file(s)")
            },
        );

        let mut staged_files = BTreeMap::new();
        for (index, source) in sources.into_iter().enumerate() {
            let staged = self.stage_source(&staging_dir, source).await?;
            report_preparing_progress(progress, index + 1, total_sources, &staged.relative_path);
            staged_files.insert(staged.relative_path.clone(), staged);
        }

        let mut entries = staged_files
            .values()
            .map(|staged| ManifestEntry {
                relative_path: staged.relative_path.clone(),
                sha256: staged.sha256.clone(),
                size_bytes: staged.size_bytes,
                kind: staged.kind,
                deleted: false,
                modified_at_ms: staged.modified_at_ms,
            })
            .collect::<Vec<_>>();

        if let Some(remote) = remote_manifest {
            let current_paths = staged_files.keys().cloned().collect::<BTreeSet<_>>();
            for remote_entry in &remote.entries {
                if current_paths.contains(&remote_entry.relative_path) {
                    continue;
                }
                entries.push(ManifestEntry {
                    relative_path: remote_entry.relative_path.clone(),
                    sha256: String::new(),
                    size_bytes: 0,
                    kind: remote_entry.kind,
                    deleted: true,
                    modified_at_ms: Some(now_ms()),
                });
            }
        }

        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let manifest = SyncManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            manifest_id: build_manifest_id(),
            parent_manifest_id: remote_manifest.map(|manifest| manifest.manifest_id.clone()),
            created_at: now_ms(),
            device_id: self.device_id.clone(),
            app_version: self.app_version.clone(),
            mode: plan.mode,
            included_items: dedup_items(&plan.items),
            entries,
        };
        let latest_ref = LatestRef::from_manifest(&manifest);

        Ok(SnapshotPrepareResult {
            manifest,
            latest_ref,
            staging_dir,
            staged_files,
        })
    }

    async fn upload_prepared_manifest_with_progress<F>(
        &self,
        prepared: &SnapshotPrepareResult,
        progress: &mut F,
    ) -> Result<BackupResult, StorageError>
    where
        F: FnMut(BackupProgress),
    {
        let upload_targets = prepared
            .manifest
            .entries
            .iter()
            .filter(|entry| !entry.deleted)
            .collect::<Vec<_>>();
        let total = upload_targets.len().max(1);
        for (index, entry) in upload_targets.into_iter().enumerate() {
            let blob_key = blob_object_key(&entry.sha256);
            if !self.store.exists(&blob_key).await? {
                let staged = prepared
                    .staged_files
                    .get(&entry.relative_path)
                    .ok_or_else(|| StorageError::backend("missing staged file for upload"))?;
                let bytes = fs::read(&staged.staged_path)
                    .await
                    .map_err(StorageError::backend)?;
                report_progress(
                    progress,
                    BackupProgressStage::UploadingBlobs,
                    0.55 + ((index as f32) / (total as f32)) * 0.25,
                    format!("Uploading blob {}", entry.sha256),
                );
                self.store
                    .put_bytes(&blob_key, bytes, "application/octet-stream")
                    .await?;
            }
        }

        report_progress(
            progress,
            BackupProgressStage::UploadingManifest,
            0.84,
            format!("Uploading manifest {}", prepared.manifest.manifest_id),
        );
        self.store
            .put_bytes(
                &manifest_object_key(&prepared.manifest.manifest_id),
                serde_json::to_vec_pretty(&prepared.manifest).map_err(StorageError::backend)?,
                "application/json",
            )
            .await?;

        report_progress(
            progress,
            BackupProgressStage::UpdatingLatestPointer,
            0.9,
            "Updating latest.json".to_string(),
        );
        self.store
            .put_bytes(
                &latest_object_key(),
                serde_json::to_vec_pretty(&prepared.latest_ref).map_err(StorageError::backend)?,
                "application/json",
            )
            .await?;

        Ok(BackupResult {
            manifest_id: prepared.manifest.manifest_id.clone(),
            manifest: prepared.manifest.clone(),
        })
    }

    async fn reconcile_local_from_remote<F>(
        &self,
        manifest: &SyncManifest,
        progress: &mut F,
    ) -> Result<(), StorageError>
    where
        F: FnMut(BackupProgress),
    {
        let total = manifest.entries.len().max(1);
        for (index, entry) in manifest.entries.iter().enumerate() {
            let target = snapshot_relative_to_root(&self.paths.root_dir, &entry.relative_path)?;
            if entry.deleted {
                if path_exists(&target).await {
                    remove_file_and_sidecars(&target).await?;
                }
                continue;
            }

            let current_hash = self.current_local_hash(entry).await?;
            if current_hash.as_deref() == Some(entry.sha256.as_str()) {
                continue;
            }

            let blob_key = blob_object_key(&entry.sha256);
            let bytes = self.store.get_bytes(&blob_key).await?;
            let actual_hash = hex::encode(Sha256::digest(&bytes));
            if actual_hash != entry.sha256 {
                return Err(StorageError::backend(format!(
                    "remote blob verification failed for {}",
                    entry.relative_path
                )));
            }

            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(StorageError::CreateDataDir)?;
            }
            remove_file_and_sidecars(&target).await?;
            fs::write(&target, bytes)
                .await
                .map_err(StorageError::backend)?;
            report_progress(
                progress,
                BackupProgressStage::ReconcilingRemote,
                0.05 + ((index as f32) / (total as f32)) * 0.05,
                format!("Reconciled {}", entry.relative_path),
            );
        }
        Ok(())
    }

    async fn restore_manifest(
        &self,
        manifest: &SyncManifest,
    ) -> Result<SnapshotRestoreResult, StorageError> {
        self.clear_restore_scope(&manifest.included_items).await?;

        let mut restored = BTreeSet::new();
        for entry in &manifest.entries {
            let target = snapshot_relative_to_root(&self.paths.root_dir, &entry.relative_path)?;
            if entry.deleted {
                remove_file_and_sidecars(&target).await?;
                continue;
            }
            let bytes = self
                .store
                .get_bytes(&blob_object_key(&entry.sha256))
                .await?;
            let actual_hash = hex::encode(Sha256::digest(&bytes));
            if actual_hash != entry.sha256 {
                return Err(StorageError::backend(format!(
                    "manifest verification failed for {}",
                    entry.relative_path
                )));
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(StorageError::CreateDataDir)?;
            }
            remove_file_and_sidecars(&target).await?;
            fs::write(&target, bytes)
                .await
                .map_err(StorageError::backend)?;
            restored.insert(target);
        }

        Ok(SnapshotRestoreResult {
            manifest_id: manifest.manifest_id.clone(),
            restored_paths: restored.into_iter().collect(),
        })
    }

    async fn clear_restore_scope(&self, items: &[BackupItem]) -> Result<(), StorageError> {
        for item in dedup_items(items) {
            match item {
                BackupItem::Session => {
                    if path_exists(&self.paths.sessions_dir).await {
                        fs::remove_dir_all(&self.paths.sessions_dir)
                            .await
                            .map_err(StorageError::backend)?;
                    }
                    remove_file_and_sidecars(&self.paths.db_path).await?;
                }
                BackupItem::Memory => {
                    remove_file_and_sidecars(&self.paths.memory_db_path).await?;
                }
                BackupItem::Archive => {
                    if path_exists(&self.paths.archives_dir).await {
                        fs::remove_dir_all(&self.paths.archives_dir)
                            .await
                            .map_err(StorageError::backend)?;
                    }
                    remove_file_and_sidecars(&self.paths.archive_db_path).await?;
                }
                BackupItem::Config => {
                    remove_file_and_sidecars(&klaw_util::config_path(&self.paths.root_dir)).await?;
                }
                BackupItem::GuiSettings => {
                    remove_file_and_sidecars(&klaw_util::settings_path(&self.paths.root_dir))
                        .await?;
                }
                BackupItem::Skills => {
                    if path_exists(&self.paths.skills_dir).await {
                        fs::remove_dir_all(&self.paths.skills_dir)
                            .await
                            .map_err(StorageError::backend)?;
                    }
                }
                BackupItem::SkillsRegistry => {
                    remove_file_and_sidecars(&klaw_util::skills_registry_manifest_path(
                        &self.paths.root_dir,
                    ))
                    .await?;
                }
                BackupItem::UserWorkspace => {
                    if path_exists(&self.paths.workspace_dir).await {
                        fs::remove_dir_all(&self.paths.workspace_dir)
                            .await
                            .map_err(StorageError::backend)?;
                    }
                }
            }
        }
        self.paths.ensure_dirs().await?;
        Ok(())
    }

    async fn current_local_hash(
        &self,
        entry: &ManifestEntry,
    ) -> Result<Option<String>, StorageError> {
        let target = snapshot_relative_to_root(&self.paths.root_dir, &entry.relative_path)?;
        match entry.kind {
            ManifestEntryKind::File => {
                if !path_exists(&target).await {
                    return Ok(None);
                }
                Ok(Some(hash_file(&target).await?))
            }
            ManifestEntryKind::SqliteSnapshot => {
                if !path_exists(&target).await {
                    return Ok(None);
                }
                let export_dir = self.paths.tmp_dir.join("sync-verify");
                fs::create_dir_all(&export_dir)
                    .await
                    .map_err(StorageError::CreateTmpDir)?;
                let snapshot_path = export_dir.join(format!("{}.db", Uuid::new_v4().simple()));
                let exported = self
                    .exporter
                    .export_snapshot(&target, &snapshot_path)
                    .await?;
                if !exported {
                    return Ok(None);
                }
                let hash = hash_file(&snapshot_path).await?;
                let _ = fs::remove_file(&snapshot_path).await;
                Ok(Some(hash))
            }
        }
    }

    async fn load_latest_manifest(&self) -> Result<Option<SyncManifest>, StorageError> {
        if !self.store.exists(&latest_object_key()).await? {
            return Ok(None);
        }
        let latest_bytes = self.store.get_bytes(&latest_object_key()).await?;
        let latest: LatestRef =
            serde_json::from_slice(&latest_bytes).map_err(StorageError::backend)?;
        Ok(Some(self.load_manifest(&latest.manifest_id).await?))
    }

    async fn load_manifest(&self, manifest_id: &str) -> Result<SyncManifest, StorageError> {
        let bytes = self
            .store
            .get_bytes(&manifest_object_key(manifest_id))
            .await?;
        serde_json::from_slice(&bytes).map_err(StorageError::backend)
    }

    async fn load_all_remote_manifests(&self) -> Result<Vec<SyncManifest>, StorageError> {
        let manifest_ids = self
            .store
            .list_keys(&manifests_prefix())
            .await?
            .into_iter()
            .filter_map(|key| {
                key.strip_prefix("manifests/")
                    .and_then(|value| value.strip_suffix(".json"))
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        let mut out = Vec::with_capacity(manifest_ids.len());
        for manifest_id in manifest_ids {
            out.push(self.load_manifest(&manifest_id).await?);
        }
        out.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.manifest_id.cmp(&left.manifest_id))
        });
        Ok(out)
    }

    async fn reject_legacy_snapshot_layout(&self) -> Result<(), StorageError> {
        let has_latest = self.store.exists(&latest_object_key()).await?;
        let manifests = self.store.list_keys(&manifests_prefix()).await?;
        if has_latest || !manifests.is_empty() {
            return Ok(());
        }
        let legacy = self.store.list_keys(&legacy_snapshots_prefix()).await?;
        if legacy.is_empty() {
            return Ok(());
        }
        Err(StorageError::backend(
            "legacy bundle.tar.zst snapshots are not supported by the manifest sync engine",
        ))
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
                }
                BackupItem::Skills => {
                    sources.push(LocalSource::Directory {
                        source_dir: self.paths.skills_dir.clone(),
                        relative_root: PathBuf::from("files/skills"),
                    });
                }
                BackupItem::SkillsRegistry => {
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
                        resolved.push(ResolvedLocalSource {
                            source_path,
                            relative_path,
                            kind: ManifestEntryKind::SqliteSnapshot,
                        });
                    }
                }
                LocalSource::File {
                    source_path,
                    relative_path,
                } => {
                    if source_path.exists() {
                        resolved.push(ResolvedLocalSource {
                            source_path,
                            relative_path,
                            kind: ManifestEntryKind::File,
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
                        resolved.push(ResolvedLocalSource {
                            source_path: file_path,
                            relative_path: relative_root.join(&rel),
                            kind: ManifestEntryKind::File,
                        });
                    }
                }
            }
        }
        Ok(resolved)
    }

    async fn stage_source(
        &self,
        staging_dir: &Path,
        source: ResolvedLocalSource,
    ) -> Result<StagedFile, StorageError> {
        let staged_path = staging_dir.join(&source.relative_path);
        if let Some(parent) = staged_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(StorageError::CreateTmpDir)?;
        }
        match source.kind {
            ManifestEntryKind::SqliteSnapshot => {
                let exported = self
                    .exporter
                    .export_snapshot(&source.source_path, &staged_path)
                    .await?;
                if !exported {
                    return Err(StorageError::backend("failed to export sqlite snapshot"));
                }
            }
            ManifestEntryKind::File => {
                fs::copy(&source.source_path, &staged_path)
                    .await
                    .map_err(StorageError::backend)?;
            }
        }
        let metadata = fs::metadata(&staged_path)
            .await
            .map_err(StorageError::backend)?;
        let modified_at_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64);
        let sha256 = hash_file_from_bytes_path(&staged_path).await?;
        Ok(StagedFile {
            relative_path: source.relative_path.to_string_lossy().replace('\\', "/"),
            staged_path,
            kind: source.kind,
            size_bytes: metadata.len(),
            sha256,
            modified_at_ms,
        })
    }
}

impl SnapshotListItem {
    fn from_manifest(manifest: &SyncManifest) -> Self {
        Self {
            manifest_id: manifest.manifest_id.clone(),
            created_at: manifest.created_at,
            device_id: manifest.device_id.clone(),
            app_version: manifest.app_version.clone(),
            mode: manifest.mode,
            included_items: manifest.included_items.clone(),
        }
    }
}

impl LatestRef {
    fn from_manifest(manifest: &SyncManifest) -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            manifest_id: manifest.manifest_id.clone(),
            updated_at: manifest.created_at,
            device_id: manifest.device_id.clone(),
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

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.operator
            .exists(&self.key(key))
            .await
            .map_err(StorageError::backend)
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
struct ResolvedLocalSource {
    source_path: PathBuf,
    relative_path: PathBuf,
    kind: ManifestEntryKind,
}

#[derive(Debug, Clone)]
struct StagedFile {
    relative_path: String,
    staged_path: PathBuf,
    kind: ManifestEntryKind,
    size_bytes: u64,
    sha256: String,
    modified_at_ms: Option<i64>,
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

fn build_manifest_id() -> String {
    format!(
        "{}-{}",
        OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "manifest".to_string())
            .replace(':', "-"),
        Uuid::new_v4().simple()
    )
}

fn now_ms() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() * 1000
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
    relative_path: &str,
) where
    F: FnMut(BackupProgress),
{
    let fraction = if total == 0 {
        0.45
    } else {
        0.12 + (completed as f32 / total as f32) * 0.35
    };
    report_progress(
        progress,
        BackupProgressStage::PreparingManifest,
        fraction,
        format!("Prepared {completed}/{total}: {relative_path}"),
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

async fn hash_file(path: &Path) -> Result<String, StorageError> {
    let bytes = fs::read(path).await.map_err(StorageError::backend)?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}

async fn hash_file_from_bytes_path(path: &Path) -> Result<String, StorageError> {
    hash_file(path).await
}

fn latest_object_key() -> String {
    "latest.json".to_string()
}

fn manifest_object_key(manifest_id: &str) -> String {
    format!("manifests/{manifest_id}.json")
}

fn manifests_prefix() -> String {
    "manifests/".to_string()
}

fn blob_object_key(sha256: &str) -> String {
    format!("blobs/sha256/{sha256}")
}

fn blobs_prefix() -> String {
    "blobs/sha256/".to_string()
}

fn legacy_snapshots_prefix() -> String {
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

async fn remove_file_and_sidecars(path: &Path) -> Result<(), StorageError> {
    if path_exists(path).await {
        if fs::metadata(path)
            .await
            .map_err(StorageError::backend)?
            .is_dir()
        {
            fs::remove_dir_all(path)
                .await
                .map_err(StorageError::backend)?;
        } else {
            fs::remove_file(path).await.map_err(StorageError::backend)?;
        }
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
        get_counts: Mutex<BTreeMap<String, usize>>,
    }

    impl MockStore {
        fn get_count(&self, key: &str) -> usize {
            self.get_counts
                .lock()
                .expect("get_counts lock")
                .get(key)
                .copied()
                .unwrap_or(0)
        }
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
            self.get_counts
                .lock()
                .expect("get_counts lock")
                .entry(key.to_string())
                .and_modify(|count| *count += 1)
                .or_insert(1);
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

        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            Ok(self.objects.lock().expect("objects lock").contains_key(key))
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

    async fn create_service_with_mock_store(
        name: &str,
        device_id: &str,
        store: Arc<MockStore>,
    ) -> BackupService {
        let paths = StoragePaths::from_root(test_root(name));
        paths.ensure_dirs().await.expect("ensure dirs");
        BackupService::with_store(
            paths,
            store,
            Arc::new(DefaultDatabaseSnapshotExporter),
            device_id.to_string(),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn initial_sync_uploads_manifest_and_blobs() {
        let service = create_service("initial-sync", "device-a").await;
        write_file(&service.paths.db_path, "db").await;
        write_file(&service.paths.sessions_dir.join("demo.jsonl"), "[]").await;

        let result = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Session],
                },
                5,
            )
            .await
            .expect("sync");

        assert!(!result.manifest_id.is_empty());
        assert_eq!(result.manifest.parent_manifest_id, None);
        assert!(
            service
                .store
                .exists(&latest_object_key())
                .await
                .expect("latest")
        );
        assert!(
            service
                .store
                .exists(&manifest_object_key(&result.manifest_id))
                .await
                .expect("manifest")
        );
        for entry in result
            .manifest
            .entries
            .iter()
            .filter(|entry| !entry.deleted)
        {
            assert!(
                service
                    .store
                    .exists(&blob_object_key(&entry.sha256))
                    .await
                    .expect("blob")
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn repeat_sync_deduplicates_existing_blobs() {
        let service = create_service("repeat-sync", "device-a").await;
        write_file(&service.paths.db_path, "db").await;
        let first = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Session],
                },
                5,
            )
            .await
            .expect("first sync");
        let blob_keys_before = service
            .store
            .list_keys(&blobs_prefix())
            .await
            .expect("keys");

        let second = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Session],
                },
                5,
            )
            .await
            .expect("second sync");
        let blob_keys_after = service
            .store
            .list_keys(&blobs_prefix())
            .await
            .expect("keys");

        assert_ne!(first.manifest_id, second.manifest_id);
        assert_eq!(blob_keys_before, blob_keys_after);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_manifest_wins_on_local_content_change() {
        let service = create_service("changed-blob", "device-a").await;
        write_file(&service.paths.db_path, "remote").await;
        let first = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Session],
                },
                5,
            )
            .await
            .expect("first sync");

        write_file(&service.paths.db_path, "local").await;
        let second = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Session],
                },
                5,
            )
            .await
            .expect("second sync");

        let current = fs::read_to_string(&service.paths.db_path)
            .await
            .expect("read db");
        assert_eq!(current, "remote");
        assert_eq!(
            first.manifest.entries[0].sha256,
            second.manifest.entries[0].sha256
        );
        assert_eq!(
            service
                .store
                .list_keys(&blobs_prefix())
                .await
                .expect("keys")
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn duplicate_content_is_stored_once() {
        let service = create_service("dedupe", "device-a").await;
        write_file(&service.paths.skills_dir.join("a.txt"), "same").await;
        write_file(&service.paths.skills_dir.join("b.txt"), "same").await;

        let result = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("sync");

        assert_eq!(result.manifest.entries.len(), 2);
        let unique_hashes = result
            .manifest
            .entries
            .iter()
            .map(|entry| entry.sha256.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(unique_hashes.len(), 1);
        assert_eq!(
            service
                .store
                .list_keys(&blobs_prefix())
                .await
                .expect("keys")
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_hash_mismatch_reconciles_local_from_remote() {
        let service = create_service("remote-wins", "device-a").await;
        write_file(&service.paths.db_path, "remote").await;
        let remote = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Session],
                },
                5,
            )
            .await
            .expect("remote sync");

        write_file(&service.paths.db_path, "local").await;
        let next = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Session],
                },
                5,
            )
            .await
            .expect("reconciled sync");

        let current = fs::read_to_string(&service.paths.db_path)
            .await
            .expect("read db");
        assert_eq!(current, "remote");
        assert_eq!(
            next.manifest.entries[0].sha256,
            remote.manifest.entries[0].sha256
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_manifest_recreates_locally_deleted_file() {
        let service = create_service("tombstone", "device-a").await;
        write_file(&service.paths.skills_dir.join("note.txt"), "keep").await;
        let first = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("first sync");

        fs::remove_file(service.paths.skills_dir.join("note.txt"))
            .await
            .expect("remove local");
        let second = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("second sync");

        let current = fs::read_to_string(service.paths.skills_dir.join("note.txt"))
            .await
            .expect("recreated local file");
        assert_eq!(current, "keep");
        assert!(first.manifest.entries.iter().all(|entry| !entry.deleted));
        assert!(second.manifest.entries.iter().all(|entry| !entry.deleted));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restore_manifest_replays_historic_state() {
        let service = create_service("restore-history", "device-a").await;
        write_file(&service.paths.skills_dir.join("note.txt"), "v1").await;
        let first = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("first sync");
        write_file(&service.paths.skills_dir.join("note.txt"), "v2").await;
        let _second = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("second sync");

        service
            .restore_snapshot(&first.manifest_id)
            .await
            .expect("restore old manifest");
        let current = fs::read_to_string(service.paths.skills_dir.join("note.txt"))
            .await
            .expect("restored body");
        assert_eq!(current, "v1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cleanup_keeps_latest_manifests_and_live_blobs() {
        let service = create_service("cleanup", "device-a").await;
        write_file(&service.paths.skills_dir.join("note.txt"), "v1").await;
        let first = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                10,
            )
            .await
            .expect("first sync");
        write_file(&service.paths.skills_dir.join("note.txt"), "v2").await;
        let second = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                10,
            )
            .await
            .expect("second sync");

        service.cleanup_remote_snapshots(1).await.expect("cleanup");

        assert!(
            !service
                .store
                .exists(&manifest_object_key(&first.manifest_id))
                .await
                .expect("first manifest")
        );
        assert!(
            service
                .store
                .exists(&manifest_object_key(&second.manifest_id))
                .await
                .expect("second manifest")
        );
        assert_eq!(
            service
                .store
                .list_keys(&blobs_prefix())
                .await
                .expect("blobs")
                .len(),
            1
        );
        let latest: LatestRef = serde_json::from_slice(
            &service
                .store
                .get_bytes(&latest_object_key())
                .await
                .expect("latest"),
        )
        .expect("decode latest");
        assert_eq!(latest.manifest_id, second.manifest_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restore_rejects_blob_checksum_mismatch() {
        let service = create_service("bad-blob", "device-a").await;
        write_file(&service.paths.skills_dir.join("note.txt"), "body").await;
        let result = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("sync");

        let entry = result.manifest.entries[0].clone();
        service
            .store
            .put_bytes(
                &blob_object_key(&entry.sha256),
                b"corrupted".to_vec(),
                "application/octet-stream",
            )
            .await
            .expect("overwrite blob");

        let err = service
            .restore_snapshot(&result.manifest_id)
            .await
            .expect_err("restore should fail");
        assert!(err.to_string().contains("verification failed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_snapshot_layout_is_rejected() {
        let service = create_service("legacy", "device-a").await;
        service
            .store
            .put_bytes(
                "snapshots/old/bundle.tar.zst",
                b"bundle".to_vec(),
                "application/zstd",
            )
            .await
            .expect("put bundle");

        let err = service
            .list_remote_snapshots()
            .await
            .expect_err("legacy should fail");
        assert!(err.to_string().contains("legacy bundle.tar.zst"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_tombstone_removes_local_file_before_publish() {
        let service = create_service("remote-tombstone", "device-a").await;
        write_file(&service.paths.skills_dir.join("note.txt"), "body").await;
        let initial = service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("initial sync");

        let tombstone = SyncManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            manifest_id: build_manifest_id(),
            parent_manifest_id: Some(initial.manifest_id.clone()),
            created_at: now_ms(),
            device_id: "device-b".to_string(),
            app_version: "test".to_string(),
            mode: SnapshotMode::ManifestVersioned,
            included_items: vec![BackupItem::Skills],
            entries: vec![ManifestEntry {
                relative_path: "files/skills/note.txt".to_string(),
                sha256: String::new(),
                size_bytes: 0,
                kind: ManifestEntryKind::File,
                deleted: true,
                modified_at_ms: Some(now_ms()),
            }],
        };
        service
            .store
            .put_bytes(
                &manifest_object_key(&tombstone.manifest_id),
                serde_json::to_vec_pretty(&tombstone).expect("manifest json"),
                "application/json",
            )
            .await
            .expect("put tombstone manifest");
        service
            .store
            .put_bytes(
                &latest_object_key(),
                serde_json::to_vec_pretty(&LatestRef::from_manifest(&tombstone)).expect("latest"),
                "application/json",
            )
            .await
            .expect("put latest");

        service
            .create_upload_and_cleanup_snapshot(
                &BackupPlan {
                    mode: SnapshotMode::ManifestVersioned,
                    items: vec![BackupItem::Skills],
                },
                5,
            )
            .await
            .expect("sync after tombstone");
        assert!(!path_exists(&service.paths.skills_dir.join("note.txt")).await);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn latest_remote_snapshot_reads_only_latest_manifest() {
        let store = Arc::new(MockStore::default());
        let service =
            create_service_with_mock_store("latest-remote", "device-a", store.clone()).await;
        let older_manifest = SyncManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            manifest_id: "manifest-older".to_string(),
            parent_manifest_id: None,
            created_at: 1_000,
            device_id: "device-a".to_string(),
            app_version: "test".to_string(),
            mode: SnapshotMode::ManifestVersioned,
            included_items: vec![BackupItem::Skills],
            entries: Vec::new(),
        };
        let latest_manifest = SyncManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            manifest_id: "manifest-latest".to_string(),
            parent_manifest_id: Some("manifest-older".to_string()),
            created_at: 2_000,
            device_id: "device-b".to_string(),
            app_version: "test".to_string(),
            mode: SnapshotMode::ManifestVersioned,
            included_items: vec![BackupItem::Skills],
            entries: Vec::new(),
        };
        store
            .put_bytes(
                &manifest_object_key(&older_manifest.manifest_id),
                serde_json::to_vec_pretty(&older_manifest).expect("older manifest json"),
                "application/json",
            )
            .await
            .expect("put older manifest");
        store
            .put_bytes(
                &manifest_object_key(&latest_manifest.manifest_id),
                serde_json::to_vec_pretty(&latest_manifest).expect("latest manifest json"),
                "application/json",
            )
            .await
            .expect("put latest manifest");
        store
            .put_bytes(
                &latest_object_key(),
                serde_json::to_vec_pretty(&LatestRef::from_manifest(&latest_manifest))
                    .expect("latest ref json"),
                "application/json",
            )
            .await
            .expect("put latest ref");

        let snapshot = service
            .latest_remote_snapshot()
            .await
            .expect("latest snapshot should load")
            .expect("latest snapshot should exist");

        assert_eq!(snapshot.manifest_id, latest_manifest.manifest_id);
        assert_eq!(store.get_count(&latest_object_key()), 1);
        assert_eq!(
            store.get_count(&manifest_object_key(&older_manifest.manifest_id)),
            0
        );
        assert_eq!(
            store.get_count(&manifest_object_key(&latest_manifest.manifest_id)),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn load_all_remote_manifests_reads_each_manifest_once() {
        let store = Arc::new(MockStore::default());
        let service =
            create_service_with_mock_store("load-all-manifests", "device-a", store.clone()).await;
        for (manifest_id, created_at) in [("manifest-a", 1_000_i64), ("manifest-b", 2_000_i64)] {
            let manifest = SyncManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                manifest_id: manifest_id.to_string(),
                parent_manifest_id: None,
                created_at,
                device_id: "device-a".to_string(),
                app_version: "test".to_string(),
                mode: SnapshotMode::ManifestVersioned,
                included_items: vec![BackupItem::Skills],
                entries: Vec::new(),
            };
            store
                .put_bytes(
                    &manifest_object_key(manifest_id),
                    serde_json::to_vec_pretty(&manifest).expect("manifest json"),
                    "application/json",
                )
                .await
                .expect("put manifest");
        }

        let manifests = service
            .load_all_remote_manifests()
            .await
            .expect("manifests should load");

        assert_eq!(manifests.len(), 2);
        assert_eq!(store.get_count(&manifest_object_key("manifest-a")), 1);
        assert_eq!(store.get_count(&manifest_object_key("manifest-b")), 1);
    }
}
