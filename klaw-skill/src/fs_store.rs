use async_trait::async_trait;
use klaw_util::{
    SKILLS_REGISTRY_MANIFEST_FILE_NAME, default_data_dir, skills_dir, skills_registry_dir,
    skills_registry_manifest_path,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::task::JoinSet;
use tokio::time::{Duration, timeout};
use tracing::{info, warn};

use crate::{
    RegistrySkillMatch, RegistrySkillSummary, ReqwestSkillFetcher, SkillError, SkillFetcher,
    SkillRecord, SkillSource, SkillSourceKind, SkillSummary, SkillsManager, SkillsRegistry,
};

#[cfg(test)]
use klaw_util::{SKILLS_DIR_NAME, SKILLS_REGISTRY_DIR_NAME};

const SKILL_MARKDOWN_FILE: &str = "SKILL.md";
const SKILL_MARKDOWN_FILE_LOWER: &str = "skill.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistrySource {
    pub name: String,
    pub address: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledSkill {
    pub registry: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistrySyncStatus {
    pub registry_name: String,
    pub commit: Option<String>,
    pub is_stale: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RegistrySyncReport {
    pub synced_registries: Vec<String>,
    pub installed_skills: Vec<String>,
    pub removed_skills: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
struct InstalledSkillsManifest {
    #[serde(default)]
    managed: Vec<InstalledSkill>,
    #[serde(default)]
    registry_commits: BTreeMap<String, String>,
    #[serde(default)]
    stale_registries: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RegistrySkillEntry {
    id: String,
    name: String,
    description: String,
    skill_dir: PathBuf,
    markdown_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillUninstallResult {
    pub removed_managed: bool,
    pub removed_local: bool,
}

#[derive(Clone, Debug)]
pub struct FileSystemSkillStore<F = ReqwestSkillFetcher> {
    root_dir: PathBuf,
    skills_dir: PathBuf,
    _fetcher: F,
}

impl FileSystemSkillStore<ReqwestSkillFetcher> {
    pub fn from_home_dir() -> Result<Self, SkillError> {
        let root = default_data_dir().ok_or(SkillError::HomeDirUnavailable)?;
        Ok(Self::from_root_dir(root))
    }

    pub fn from_root_dir(root_dir: PathBuf) -> Self {
        Self::with_fetcher(root_dir, ReqwestSkillFetcher::default())
    }
}

impl<F> FileSystemSkillStore<F>
where
    F: SkillFetcher,
{
    pub fn with_fetcher(root_dir: PathBuf, fetcher: F) -> Self {
        let skills_dir = skills_dir(&root_dir);
        Self {
            root_dir,
            skills_dir,
            _fetcher: fetcher,
        }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    pub fn skills_registry_dir(&self) -> PathBuf {
        skills_registry_dir(&self.root_dir)
    }

    fn installed_manifest_path(&self) -> PathBuf {
        skills_registry_manifest_path(&self.root_dir)
    }

    pub(crate) fn validate_skill_name(input: &str) -> Result<String, SkillError> {
        let value = input.trim();
        if value.is_empty() {
            return Err(SkillError::InvalidSkillName(input.to_string()));
        }
        let valid = value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
        if !valid {
            return Err(SkillError::InvalidSkillName(value.to_string()));
        }
        Ok(value.to_string())
    }

    fn skill_markdown_path(&self, skill_name: &str) -> PathBuf {
        self.skills_dir
            .join(skill_name)
            .join(Path::new(SKILL_MARKDOWN_FILE))
    }

    async fn ensure_skills_dir(&self) -> Result<(), SkillError> {
        fs::create_dir_all(&self.skills_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "create_dir_all",
                path: self.skills_dir.clone(),
                source,
            })
    }

    async fn ensure_registry_dir(&self) -> Result<(), SkillError> {
        let dir = self.skills_registry_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "create_dir_all",
                path: dir,
                source,
            })
    }

    async fn load_installed_manifest(&self) -> Result<InstalledSkillsManifest, SkillError> {
        let path = self.installed_manifest_path();
        let exists = fs::try_exists(&path)
            .await
            .map_err(|source| SkillError::Io {
                op: "try_exists",
                path: path.clone(),
                source,
            })?;
        if !exists {
            return Ok(InstalledSkillsManifest::default());
        }
        let raw = fs::read_to_string(&path)
            .await
            .map_err(|source| SkillError::Io {
                op: "read_to_string",
                path: path.clone(),
                source,
            })?;
        serde_json::from_str(&raw).map_err(|source| SkillError::JsonParse { path, source })
    }

    async fn write_installed_manifest(
        &self,
        manifest: &InstalledSkillsManifest,
    ) -> Result<(), SkillError> {
        fs::create_dir_all(&self.root_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "create_dir_all",
                path: self.root_dir.clone(),
                source,
            })?;
        let path = self.installed_manifest_path();
        let temp = self.root_dir.join(format!(
            "{}.tmp-{}",
            SKILLS_REGISTRY_MANIFEST_FILE_NAME,
            now_ms()
        ));
        let payload =
            serde_json::to_string_pretty(manifest).map_err(|source| SkillError::JsonParse {
                path: path.clone(),
                source,
            })?;
        fs::write(&temp, payload)
            .await
            .map_err(|source| SkillError::Io {
                op: "write",
                path: temp.clone(),
                source,
            })?;
        fs::rename(&temp, &path)
            .await
            .map_err(|source| SkillError::Io {
                op: "rename",
                path: path.clone(),
                source,
            })?;
        Ok(())
    }

    pub async fn sync_registry_installed_skills(
        &self,
        sources: &[RegistrySource],
        installed: &[InstalledSkill],
        sync_timeout_secs: u64,
    ) -> Result<RegistrySyncReport, SkillError> {
        self.ensure_skills_dir().await?;
        self.ensure_registry_dir().await?;
        let current_manifest = self.load_installed_manifest().await?;

        let mut source_map = BTreeMap::new();
        for source in sources {
            let source_name = source.name.trim();
            let source_address = source.address.trim();
            if source_name.is_empty() || source_address.is_empty() {
                return Err(SkillError::InvalidSkillName(
                    "skills.<registry>.address cannot be empty".to_string(),
                ));
            }
            if source_map
                .insert(source_name.to_string(), source_address.to_string())
                .is_some()
            {
                return Err(SkillError::InvalidSkillName(format!(
                    "duplicate source name `{source_name}`"
                )));
            }
        }

        let mut requested = Vec::new();
        for item in installed {
            let registry = item.registry.trim();
            let name = item.name.trim();
            if registry.is_empty() {
                return Err(SkillError::InvalidSkillName(
                    "installed skills registry cannot be empty".to_string(),
                ));
            }
            if name.is_empty() {
                return Err(SkillError::InvalidSkillName(
                    "installed skill name cannot be empty".to_string(),
                ));
            }
            if !source_map.contains_key(registry) {
                return Err(SkillError::InvalidSkillName(format!(
                    "installed registry `{registry}` is not configured in skills.<registry>"
                )));
            }
            requested.push((registry.to_string(), name.to_string()));
        }

        let registry_root = self.skills_registry_dir();
        let timeout_duration = Duration::from_secs(sync_timeout_secs.max(1));
        let mut report = RegistrySyncReport::default();
        let mut join_set = JoinSet::new();
        for (name, address) in &source_map {
            let registry_root = registry_root.clone();
            let source = RegistrySource {
                name: name.clone(),
                address: address.clone(),
            };
            join_set.spawn(async move {
                let registry_name = source.name.clone();
                let result = timeout(
                    timeout_duration,
                    sync_source_repository(&registry_root, &source),
                )
                .await;
                (registry_name, result)
            });
        }

        let mut available_registries = BTreeSet::new();
        let mut current_registry_commits = BTreeMap::new();
        while let Some(task_result) = join_set.join_next().await {
            match task_result {
                Ok((registry_name, Ok(Ok(())))) => {
                    if let Some(commit) =
                        read_registry_head_commit(&registry_root.join(&registry_name)).await?
                    {
                        current_registry_commits.insert(registry_name.clone(), commit);
                    }
                    info!(registry = %registry_name, "skills registry sync completed");
                    report.synced_registries.push(registry_name.clone());
                    available_registries.insert(registry_name);
                }
                Ok((registry_name, Ok(Err(err)))) => {
                    warn!(
                        registry = %registry_name,
                        error = %err,
                        "skills registry sync failed, skipping"
                    );
                    let path = registry_root.join(&registry_name);
                    if fs::try_exists(&path)
                        .await
                        .map_err(|source| SkillError::Io {
                            op: "try_exists",
                            path: path.clone(),
                            source,
                        })?
                    {
                        if let Some(commit) = read_registry_head_commit(&path).await? {
                            current_registry_commits.insert(registry_name.clone(), commit);
                        }
                        available_registries.insert(registry_name);
                    }
                }
                Ok((registry_name, Err(_elapsed))) => {
                    warn!(
                        registry = %registry_name,
                        timeout_secs = sync_timeout_secs.max(1),
                        "skills registry sync timed out, skipping"
                    );
                    let path = registry_root.join(&registry_name);
                    if fs::try_exists(&path)
                        .await
                        .map_err(|source| SkillError::Io {
                            op: "try_exists",
                            path: path.clone(),
                            source,
                        })?
                    {
                        if let Some(commit) = read_registry_head_commit(&path).await? {
                            current_registry_commits.insert(registry_name.clone(), commit);
                        }
                        available_registries.insert(registry_name);
                    }
                }
                Err(join_err) => {
                    warn!(error = %join_err, "skills registry sync task join failed");
                }
            }
        }

        let targeted_registries: BTreeSet<String> = source_map.keys().cloned().collect();
        let previous_targeted_managed: BTreeSet<(String, String)> = current_manifest
            .managed
            .iter()
            .filter(|item| targeted_registries.contains(&item.registry))
            .map(|item| (item.registry.clone(), item.name.clone()))
            .collect();
        let mut stale_registries = BTreeSet::new();
        for source_name in source_map.keys() {
            if !report.synced_registries.iter().any(|it| it == source_name)
                && available_registries.contains(source_name)
            {
                stale_registries.insert(source_name.clone());
            }
        }

        let mut desired = BTreeSet::new();
        let mut desired_names = BTreeSet::new();
        for (registry_name, requested_name) in requested {
            if !available_registries.contains(&registry_name) {
                warn!(
                    registry = %registry_name,
                    skill_name = %requested_name,
                    "skip installed skill because registry is unavailable"
                );
                continue;
            }
            let repo_dir = registry_root.join(&registry_name);
            let (_source_dir, target_name) =
                resolve_registry_skill_dir(&repo_dir, &requested_name, &registry_name).await?;
            if !desired_names.insert(target_name.clone()) {
                return Err(SkillError::InvalidSkillName(format!(
                    "duplicate installed skill target `{target_name}`"
                )));
            }
            if !desired.insert((registry_name.clone(), target_name.clone())) {
                return Err(SkillError::InvalidSkillName(format!(
                    "duplicate installed skill `{target_name}` in registry `{registry_name}`"
                )));
            }
        }

        let mut next_manifest = current_manifest.clone();
        next_manifest
            .managed
            .retain(|item| !targeted_registries.contains(&item.registry));
        for (registry_name, target_name) in &desired {
            next_manifest.managed.push(InstalledSkill {
                registry: registry_name.clone(),
                name: target_name.clone(),
            });
            if !previous_targeted_managed.contains(&(registry_name.clone(), target_name.clone())) {
                report.installed_skills.push(target_name.clone());
            }
        }
        for (registry_name, skill_name) in &previous_targeted_managed {
            if !desired.contains(&(registry_name.clone(), skill_name.clone())) {
                report.removed_skills.push(skill_name.clone());
            }
        }

        next_manifest.managed.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.registry.cmp(&b.registry))
        });
        next_manifest
            .registry_commits
            .retain(|registry, _| !targeted_registries.contains(registry));
        next_manifest
            .registry_commits
            .extend(current_registry_commits);
        next_manifest
            .stale_registries
            .retain(|registry| !targeted_registries.contains(registry));
        next_manifest.stale_registries.extend(stale_registries);
        self.write_installed_manifest(&next_manifest).await?;
        report.synced_registries.sort();
        report.installed_skills.sort();
        report.removed_skills.sort();
        Ok(report)
    }

    async fn read_local_skill_record(&self, skill_name: &str) -> Result<SkillRecord, SkillError> {
        let path = self.skill_markdown_path(skill_name);
        let exists = fs::try_exists(&path)
            .await
            .map_err(|source| SkillError::Io {
                op: "try_exists",
                path: path.clone(),
                source,
            })?;
        if !exists {
            return Err(SkillError::SkillNotFound(skill_name.to_string()));
        }

        let content = fs::read_to_string(&path)
            .await
            .map_err(|source| SkillError::Io {
                op: "read_to_string",
                path: path.clone(),
                source,
            })?;
        let metadata = fs::metadata(&path).await.map_err(|source| SkillError::Io {
            op: "metadata",
            path: path.clone(),
            source,
        })?;

        Ok(SkillRecord {
            name: skill_name.to_string(),
            source: SkillSource::configured("local", skill_name, ""),
            local_path: path,
            content,
            updated_at_ms: modified_time_ms(&metadata).unwrap_or_default(),
            source_kind: SkillSourceKind::Local,
            registry: None,
            stale: None,
        })
    }

    async fn read_registry_skill_record(
        &self,
        registry: &str,
        skill_name: &str,
        stale: bool,
    ) -> Result<SkillRecord, SkillError> {
        let repo_dir = self.skills_registry_dir().join(registry);
        let (skill_dir, resolved_name) =
            resolve_registry_skill_dir(&repo_dir, skill_name, registry).await?;
        let path = skill_dir.join(SKILL_MARKDOWN_FILE);
        let content = fs::read_to_string(&path)
            .await
            .map_err(|source| SkillError::Io {
                op: "read_to_string",
                path: path.clone(),
                source,
            })?;
        let metadata = fs::metadata(&path).await.map_err(|source| SkillError::Io {
            op: "metadata",
            path: path.clone(),
            source,
        })?;

        Ok(SkillRecord {
            name: resolved_name.clone(),
            source: SkillSource::configured(registry, &resolved_name, ""),
            local_path: path,
            content,
            updated_at_ms: modified_time_ms(&metadata).unwrap_or_default(),
            source_kind: SkillSourceKind::Registry,
            registry: Some(registry.to_string()),
            stale: Some(stale),
        })
    }

    async fn load_managed_index(
        &self,
    ) -> Result<(InstalledSkillsManifest, BTreeMap<String, (String, bool)>), SkillError> {
        let manifest = self.load_installed_manifest().await?;
        let mut managed = BTreeMap::new();
        for item in &manifest.managed {
            let stale = manifest.stale_registries.contains(&item.registry);
            if managed
                .insert(item.name.clone(), (item.registry.clone(), stale))
                .is_some()
            {
                return Err(SkillError::InvalidSkillName(format!(
                    "duplicate managed skill `{}` in manifest",
                    item.name
                )));
            }
        }
        Ok((manifest, managed))
    }

    fn to_summary(record: &SkillRecord) -> SkillSummary {
        SkillSummary {
            name: record.name.clone(),
            local_path: record.local_path.clone(),
            updated_at_ms: record.updated_at_ms,
            source_kind: record.source_kind,
            registry: record.registry.clone(),
            stale: record.stale,
        }
    }

    async fn list_local_summaries(
        &self,
        managed_by_name: &BTreeMap<String, (String, bool)>,
    ) -> Result<Vec<SkillSummary>, SkillError> {
        self.ensure_skills_dir().await?;
        let mut items = Vec::new();
        let mut entries =
            fs::read_dir(&self.skills_dir)
                .await
                .map_err(|source| SkillError::Io {
                    op: "read_dir",
                    path: self.skills_dir.clone(),
                    source,
                })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|source| SkillError::Io {
                op: "next_entry",
                path: self.skills_dir.clone(),
                source,
            })?
        {
            let path = entry.path();
            if !is_directory(&path, &entry).await? {
                continue;
            }
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if managed_by_name.contains_key(name) {
                warn!(skill_name = %name, "skip local skill because managed registry skill has higher priority");
                continue;
            }
            let record = self.read_local_skill_record(name).await?;
            items.push(Self::to_summary(&record));
        }
        Ok(items)
    }

    pub async fn install_from_registry(
        &self,
        registry: &str,
        skill_name: &str,
    ) -> Result<(SkillRecord, bool), SkillError> {
        let requested_name = validate_registry_skill_selector(skill_name)?;
        let registry_name = registry.trim();
        if registry_name.is_empty() {
            return Err(SkillError::InvalidSkillName(
                "installed skills registry cannot be empty".to_string(),
            ));
        }
        let mut manifest = self.load_installed_manifest().await?;
        let stale = manifest.stale_registries.contains(registry_name);
        let record = self
            .read_registry_skill_record(registry_name, &requested_name, stale)
            .await?;
        if manifest
            .managed
            .iter()
            .any(|it| it.name == record.name && it.registry != registry_name)
        {
            return Err(SkillError::InvalidSkillName(format!(
                "managed skill `{}` already indexed from another registry",
                record.name
            )));
        }
        let already_installed = manifest
            .managed
            .iter()
            .any(|it| it.registry == registry_name && it.name == record.name);
        if !already_installed {
            manifest.managed.push(InstalledSkill {
                registry: registry_name.to_string(),
                name: record.name.clone(),
            });
            manifest.managed.sort_by(|a, b| {
                a.name
                    .cmp(&b.name)
                    .then_with(|| a.registry.cmp(&b.registry))
            });
            self.write_installed_manifest(&manifest).await?;
        }
        Ok((record, already_installed))
    }

    pub async fn list_source_skills(
        &self,
        registry: &str,
    ) -> Result<Vec<RegistrySkillSummary>, SkillError> {
        let registry_name = registry.trim();
        if registry_name.is_empty() {
            return Err(SkillError::InvalidSkillName(
                "installed skills registry cannot be empty".to_string(),
            ));
        }

        let repo_dir = self.skills_registry_dir().join(registry_name);
        let exists = fs::try_exists(&repo_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "try_exists",
                path: repo_dir.clone(),
                source,
            })?;
        if !exists {
            return Err(SkillError::RegistryUnavailable {
                registry: registry_name.to_string(),
                path: repo_dir,
            });
        }

        let mut entries = discover_registry_skills(&repo_dir).await?;
        let mut items = Vec::new();
        for entry in entries.drain(..) {
            items.push(RegistrySkillSummary {
                id: entry.id,
                name: entry.name,
                local_path: entry.markdown_path,
            });
        }
        items.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        items.dedup_by(|a, b| a.id == b.id);
        Ok(items)
    }

    pub async fn uninstall_from_registry(
        &self,
        registry: &str,
        skill_name: &str,
    ) -> Result<(), SkillError> {
        let registry_name = registry.trim();
        if registry_name.is_empty() {
            return Err(SkillError::InvalidSkillName(
                "installed skills registry cannot be empty".to_string(),
            ));
        }
        let name = Self::validate_skill_name(skill_name)?;

        let mut manifest = self.load_installed_manifest().await?;
        let before_len = manifest.managed.len();
        manifest
            .managed
            .retain(|item| !(item.registry == registry_name && item.name == name));
        if manifest.managed.len() == before_len {
            return Err(SkillError::SkillNotFound(name));
        }
        self.write_installed_manifest(&manifest).await?;
        Ok(())
    }

    pub async fn cleanup_registry(&self, registry: &str) -> Result<usize, SkillError> {
        let registry_name = registry.trim();
        if registry_name.is_empty() {
            return Err(SkillError::InvalidSkillName(
                "registry name cannot be empty".to_string(),
            ));
        }

        let mut manifest = self.load_installed_manifest().await?;
        let before_len = manifest.managed.len();
        manifest
            .managed
            .retain(|item| item.registry != registry_name);
        manifest.registry_commits.remove(registry_name);
        manifest.stale_registries.remove(registry_name);
        let removed_count = before_len - manifest.managed.len();
        if removed_count > 0 {
            self.write_installed_manifest(&manifest).await?;
        }
        Ok(removed_count)
    }

    pub async fn uninstall(&self, skill_name: &str) -> Result<SkillUninstallResult, SkillError> {
        let name = Self::validate_skill_name(skill_name)?;
        let mut manifest = self.load_installed_manifest().await?;
        let before_len = manifest.managed.len();
        manifest.managed.retain(|item| item.name != name);
        let removed_managed = manifest.managed.len() != before_len;
        if removed_managed {
            self.write_installed_manifest(&manifest).await?;
        }

        let skill_dir = self.skills_dir.join(&name);
        let exists = fs::try_exists(&skill_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "try_exists",
                path: skill_dir.clone(),
                source,
            })?;
        let removed_local = if exists {
            fs::remove_dir_all(&skill_dir)
                .await
                .map_err(|source| SkillError::Io {
                    op: "remove_dir_all",
                    path: skill_dir,
                    source,
                })?;
            true
        } else {
            false
        };

        if !removed_managed && !removed_local {
            return Err(SkillError::SkillNotFound(name));
        }
        Ok(SkillUninstallResult {
            removed_managed,
            removed_local,
        })
    }
    pub async fn search_source_skills(
        &self,
        source_name: &str,
        query: &str,
    ) -> Result<Vec<RegistrySkillMatch>, SkillError> {
        let query_terms = tokenize_query(query);
        if query_terms.is_empty() {
            return Ok(Vec::new());
        }

        let repo_dir = self.skills_registry_dir().join(source_name);
        let exists = fs::try_exists(&repo_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "try_exists",
                path: repo_dir.clone(),
                source,
            })?;
        if !exists {
            return Ok(Vec::new());
        }

        let entries = discover_registry_skills(&repo_dir).await?;
        let mut matches = Vec::new();
        for entry in entries {
            let Some((_score, matched_fields)) =
                score_registry_match(&query_terms, &entry.name, &entry.description)
            else {
                continue;
            };
            matches.push(RegistrySkillMatch {
                source: source_name.to_string(),
                skill_name: entry.name,
                description: entry.description,
                local_path: entry.markdown_path,
                matched_fields,
            });
        }

        Ok(matches)
    }

    pub async fn get_registry_statuses(
        &self,
        registry_names: &[String],
    ) -> Result<Vec<RegistrySyncStatus>, SkillError> {
        let manifest = self.load_installed_manifest().await?;
        let mut statuses = Vec::with_capacity(registry_names.len());
        for name in registry_names {
            let commit = manifest.registry_commits.get(name).cloned();
            let is_stale = manifest.stale_registries.contains(name);
            statuses.push(RegistrySyncStatus {
                registry_name: name.clone(),
                commit,
                is_stale,
            });
        }
        Ok(statuses)
    }
}

#[async_trait]
impl<F> SkillsManager for FileSystemSkillStore<F>
where
    F: SkillFetcher,
{
    async fn install_from_registry(
        &self,
        source_name: &str,
        skill_name: &str,
    ) -> Result<(SkillRecord, bool), SkillError> {
        FileSystemSkillStore::install_from_registry(self, source_name, skill_name).await
    }

    async fn uninstall_from_registry(
        &self,
        source_name: &str,
        skill_name: &str,
    ) -> Result<(), SkillError> {
        FileSystemSkillStore::uninstall_from_registry(self, source_name, skill_name).await
    }

    async fn uninstall(&self, skill_name: &str) -> Result<SkillUninstallResult, SkillError> {
        FileSystemSkillStore::uninstall(self, skill_name).await
    }

    async fn list_installed(&self) -> Result<Vec<SkillSummary>, SkillError> {
        let mut items = Vec::new();
        let (manifest, managed_by_name) = self.load_managed_index().await?;
        for item in &manifest.managed {
            match self
                .read_registry_skill_record(
                    &item.registry,
                    &item.name,
                    manifest.stale_registries.contains(&item.registry),
                )
                .await
            {
                Ok(record) => items.push(Self::to_summary(&record)),
                Err(err) => warn!(
                    registry = %item.registry,
                    skill_name = %item.name,
                    error = %err,
                    "skip managed registry skill in list"
                ),
            }
        }
        items.extend(self.list_local_summaries(&managed_by_name).await?);

        items.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(items)
    }

    async fn get_installed(&self, skill_name: &str) -> Result<SkillRecord, SkillError> {
        let name = Self::validate_skill_name(skill_name)?;
        let (_manifest, managed_by_name) = self.load_managed_index().await?;
        if let Some((registry, stale)) = managed_by_name.get(&name) {
            return self
                .read_registry_skill_record(registry, &name, *stale)
                .await;
        }
        self.read_local_skill_record(&name).await
    }

    async fn load_all_installed_skill_markdowns(&self) -> Result<Vec<SkillRecord>, SkillError> {
        let (manifest, managed_by_name) = self.load_managed_index().await?;
        let mut records = Vec::new();
        for item in &manifest.managed {
            match self
                .read_registry_skill_record(
                    &item.registry,
                    &item.name,
                    manifest.stale_registries.contains(&item.registry),
                )
                .await
            {
                Ok(record) => records.push(record),
                Err(err) => warn!(
                    registry = %item.registry,
                    skill_name = %item.name,
                    error = %err,
                    "skip managed registry skill in load_all"
                ),
            }
        }
        self.ensure_skills_dir().await?;
        let mut entries =
            fs::read_dir(&self.skills_dir)
                .await
                .map_err(|source| SkillError::Io {
                    op: "read_dir",
                    path: self.skills_dir.clone(),
                    source,
                })?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|source| SkillError::Io {
                op: "next_entry",
                path: self.skills_dir.clone(),
                source,
            })?
        {
            let path = entry.path();
            if !is_directory(&path, &entry).await? {
                continue;
            }
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if managed_by_name.contains_key(name) {
                warn!(skill_name = %name, "skip local skill in load_all because managed registry skill has higher priority");
                continue;
            }
            records.push(self.read_local_skill_record(name).await?);
        }
        records.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(records)
    }
}

#[async_trait]
impl<F> SkillsRegistry for FileSystemSkillStore<F>
where
    F: SkillFetcher,
{
    async fn list_source_skills(
        &self,
        source_name: &str,
    ) -> Result<Vec<RegistrySkillSummary>, SkillError> {
        FileSystemSkillStore::list_source_skills(self, source_name).await
    }

    async fn get_source_skill(
        &self,
        source_name: &str,
        skill_name: &str,
    ) -> Result<SkillRecord, SkillError> {
        let registry_name = source_name.trim();
        if registry_name.is_empty() {
            return Err(SkillError::InvalidSkillName(
                "installed skills registry cannot be empty".to_string(),
            ));
        }
        let manifest = self.load_installed_manifest().await?;
        self.read_registry_skill_record(
            registry_name,
            skill_name,
            manifest.stale_registries.contains(registry_name),
        )
        .await
    }

    async fn search_source_skills(
        &self,
        source_name: &str,
        query: &str,
    ) -> Result<Vec<RegistrySkillMatch>, SkillError> {
        FileSystemSkillStore::search_source_skills(self, source_name, query).await
    }
}

pub fn open_default_skills_manager() -> Result<FileSystemSkillStore<ReqwestSkillFetcher>, SkillError>
{
    FileSystemSkillStore::from_home_dir()
}

pub fn open_default_skill_registry() -> Result<FileSystemSkillStore<ReqwestSkillFetcher>, SkillError>
{
    FileSystemSkillStore::from_home_dir()
}

async fn is_directory(path: &Path, entry: &tokio::fs::DirEntry) -> Result<bool, SkillError> {
    let ty = entry.file_type().await.map_err(|source| SkillError::Io {
        op: "file_type",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(ty.is_dir())
}

async fn sync_source_repository(
    registry_root: &Path,
    source: &RegistrySource,
) -> Result<(), SkillError> {
    let target = registry_root.join(&source.name);
    let target_exists = fs::try_exists(&target)
        .await
        .map_err(|source| SkillError::Io {
            op: "try_exists",
            path: target.clone(),
            source,
        })?;
    if !target_exists {
        let target_str = target.to_string_lossy().to_string();
        run_git(
            "clone",
            None,
            &["clone", "--depth", "1", &source.address, &target_str],
        )
        .await?;
        return Ok(());
    }

    let git_dir = target.join(".git");
    let git_dir_exists = fs::try_exists(&git_dir)
        .await
        .map_err(|source| SkillError::Io {
            op: "try_exists",
            path: git_dir.clone(),
            source,
        })?;
    if !git_dir_exists {
        return Err(SkillError::GitCommand {
            context: "sync",
            command: format!("expected git repository at {}", target.display()),
            stderr: "missing .git directory".to_string(),
        });
    }

    run_git(
        "set-url",
        Some(&target),
        &["remote", "set-url", "origin", &source.address],
    )
    .await?;
    run_git("fetch", Some(&target), &["fetch", "--depth", "1", "origin"]).await?;

    let head_ref = run_git_capture(
        "origin-head",
        Some(&target),
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .await
    .unwrap_or_else(|_| "origin/main".to_string());

    if run_git(
        "reset",
        Some(&target),
        &["reset", "--hard", head_ref.trim()],
    )
    .await
    .is_err()
    {
        run_git(
            "reset",
            Some(&target),
            &["reset", "--hard", "origin/master"],
        )
        .await?;
    }
    Ok(())
}

async fn read_registry_head_commit(repo_dir: &Path) -> Result<Option<String>, SkillError> {
    let git_dir = repo_dir.join(".git");
    let git_dir_exists = fs::try_exists(&git_dir)
        .await
        .map_err(|source| SkillError::Io {
            op: "try_exists",
            path: git_dir.clone(),
            source,
        })?;
    if !git_dir_exists {
        return Ok(None);
    }
    let commit = run_git_capture("rev-parse", Some(repo_dir), &["rev-parse", "HEAD"]).await?;
    let commit = commit.trim().to_string();
    if commit.is_empty() {
        return Ok(None);
    }
    Ok(Some(commit))
}

async fn resolve_registry_skill_dir(
    registry_repo_dir: &Path,
    requested_name: &str,
    registry_name: &str,
) -> Result<(PathBuf, String), SkillError> {
    let entries = discover_registry_skills(registry_repo_dir).await?;
    let mut matched: Option<(PathBuf, String)> = None;
    for entry in entries {
        if entry.id != requested_name && entry.name != requested_name {
            continue;
        }
        if matched.is_some() {
            return Err(SkillError::InvalidSkillName(format!(
                "skill `{requested_name}` in registry `{registry_name}` is ambiguous"
            )));
        }
        matched = Some((entry.skill_dir, entry.name));
    }

    matched.ok_or_else(|| SkillError::RegistrySkillNotFound {
        registry: registry_name.to_string(),
        skill_name: requested_name.to_string(),
        path: registry_repo_dir
            .join(requested_name)
            .join(SKILL_MARKDOWN_FILE),
    })
}

async fn discover_registry_skills(
    registry_repo_dir: &Path,
) -> Result<Vec<RegistrySkillEntry>, SkillError> {
    let mut pending = vec![registry_repo_dir.to_path_buf()];
    let mut items = Vec::new();

    while let Some(dir) = pending.pop() {
        let mut entries = fs::read_dir(&dir).await.map_err(|source| SkillError::Io {
            op: "read_dir",
            path: dir.clone(),
            source,
        })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|source| SkillError::Io {
                op: "next_entry",
                path: dir.clone(),
                source,
            })?
        {
            let path = entry.path();
            let file_type = entry.file_type().await.map_err(|source| SkillError::Io {
                op: "file_type",
                path: path.clone(),
                source,
            })?;

            if file_type.is_dir() {
                if path.file_name().and_then(OsStr::to_str) == Some(".git") {
                    continue;
                }
                pending.push(path);
                continue;
            }

            if !file_type.is_file() || !is_skill_markdown_path(&path) {
                continue;
            }

            let skill_dir = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| registry_repo_dir.to_path_buf());
            let content = fs::read_to_string(&path)
                .await
                .map_err(|source| SkillError::Io {
                    op: "read_to_string",
                    path: path.clone(),
                    source,
                })?;
            let fallback_name = skill_dir
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_string();
            let name = parse_skill_name_from_markdown(&content)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(fallback_name);
            if name.is_empty() {
                continue;
            }
            let relative_dir = skill_dir
                .strip_prefix(registry_repo_dir)
                .ok()
                .unwrap_or(skill_dir.as_path());
            let relative_dir_text = relative_dir.to_string_lossy().replace('\\', "/");
            let id = relative_dir_text
                .strip_prefix("skills/")
                .unwrap_or(&relative_dir_text)
                .to_string();
            items.push(RegistrySkillEntry {
                id,
                name,
                description: extract_skill_description(&content),
                skill_dir,
                markdown_path: path,
            });
        }
    }

    items.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.name.cmp(&b.name)));
    items.dedup_by(|a, b| a.id == b.id);
    Ok(items)
}

fn is_skill_markdown_path(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            name.eq_ignore_ascii_case(SKILL_MARKDOWN_FILE)
                || name.eq_ignore_ascii_case(SKILL_MARKDOWN_FILE_LOWER)
        })
}

fn parse_skill_name_from_markdown(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("# ") {
            let value = name.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        if let Some(name) = trimmed.strip_prefix("name:") {
            let value = name.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn validate_registry_skill_selector(input: &str) -> Result<String, SkillError> {
    let value = input.trim();
    if value.is_empty() {
        return Err(SkillError::InvalidSkillName(input.to_string()));
    }
    if !value.contains('/') {
        return FileSystemSkillStore::<ReqwestSkillFetcher>::validate_skill_name(value);
    }

    let path = Path::new(value);
    if path.is_absolute() {
        return Err(SkillError::InvalidSkillName(value.to_string()));
    }

    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                let Some(part) = part.to_str() else {
                    return Err(SkillError::InvalidSkillName(value.to_string()));
                };
                if part.is_empty() {
                    return Err(SkillError::InvalidSkillName(value.to_string()));
                }
            }
            _ => return Err(SkillError::InvalidSkillName(value.to_string())),
        }
    }

    Ok(value.to_string())
}

fn extract_skill_description(markdown: &str) -> String {
    markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_default()
        .to_string()
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn score_registry_match(
    query_terms: &[String],
    skill_name: &str,
    description: &str,
) -> Option<(i32, Vec<String>)> {
    if query_terms.is_empty() {
        return None;
    }

    let normalized_name = skill_name.to_lowercase();
    let normalized_description = description.to_lowercase();
    let mut score = 0;
    let mut matched_fields = Vec::new();

    for term in query_terms {
        let mut matched = false;
        if normalized_name.contains(term) {
            score += if normalized_name == *term { 100 } else { 60 };
            matched = true;
            if !matched_fields.iter().any(|field| field == "skill_name") {
                matched_fields.push("skill_name".to_string());
            }
        }
        if normalized_description.contains(term) {
            score += 20;
            matched = true;
            if !matched_fields.iter().any(|field| field == "description") {
                matched_fields.push("description".to_string());
            }
        }
        if !matched {
            return None;
        }
    }

    Some((score, matched_fields))
}

async fn run_git(
    context: &'static str,
    cwd: Option<&Path>,
    args: &[&str],
) -> Result<(), SkillError> {
    run_git_capture(context, cwd, args).await.map(|_| ())
}

async fn run_git_capture(
    context: &'static str,
    cwd: Option<&Path>,
    args: &[&str],
) -> Result<String, SkillError> {
    let args_owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let cwd_owned = cwd.map(Path::to_path_buf);
    let first = run_git_capture_once(context, cwd_owned.clone(), args_owned.clone()).await;
    match first {
        Ok(output) => Ok(output),
        Err(SkillError::GitCommand {
            context,
            command,
            stderr,
        }) => {
            let Some(lock_path) = parse_git_lock_path(&stderr) else {
                return Err(SkillError::GitCommand {
                    context,
                    command,
                    stderr,
                });
            };

            if !lock_path.exists() {
                return Err(SkillError::GitCommand {
                    context,
                    command,
                    stderr,
                });
            }

            tracing::warn!(
                lock_path = %lock_path.display(),
                "removing stale git lock file and retrying"
            );
            std::fs::remove_file(&lock_path).map_err(|source| SkillError::Io {
                op: "remove_file",
                path: lock_path.clone(),
                source,
            })?;

            run_git_capture_once(context, cwd_owned, args_owned).await
        }
        Err(err) => Err(err),
    }
}

async fn run_git_capture_once(
    context: &'static str,
    cwd: Option<PathBuf>,
    args: Vec<String>,
) -> Result<String, SkillError> {
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new("git");
        command.args(&args);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let output = command.output().map_err(|source| SkillError::Io {
            op: "spawn git",
            path: PathBuf::from("git"),
            source,
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(SkillError::GitCommand {
                context,
                command: format!("git {}", args.join(" ")),
                stderr,
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    })
    .await
    .map_err(|source| SkillError::Io {
        op: "join git task",
        path: PathBuf::from("git"),
        source: std::io::Error::other(source.to_string()),
    })?
}

fn parse_git_lock_path(stderr: &str) -> Option<PathBuf> {
    if !stderr.contains("Another git process seems to be running in this repository") {
        return None;
    }

    let prefix = "Unable to create '";
    let suffix = "': File exists.";
    stderr.lines().find_map(|line| {
        let start = line.find(prefix)?;
        let path = &line[start + prefix.len()..];
        let end = path.find(suffix)?;
        let candidate = &path[..end];
        if !candidate.ends_with(".lock") {
            return None;
        }
        Some(PathBuf::from(candidate))
    })
}

fn modified_time_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct MockSkillFetcher {
        payloads: Arc<Mutex<BTreeMap<String, String>>>,
    }

    #[async_trait]
    impl SkillFetcher for MockSkillFetcher {
        async fn fetch_markdown(&self, source: &SkillSource) -> Result<String, SkillError> {
            let name = source.skill_name();
            self.payloads
                .lock()
                .expect("lock payloads")
                .get(name)
                .cloned()
                .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))
        }
    }

    fn test_root() -> PathBuf {
        let nonce = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("klaw-skill-test-{}-{nonce}", now_ms()))
    }

    async fn write_registry_skill(
        root: &Path,
        registry: &str,
        skill_name: &str,
        content: &str,
    ) -> PathBuf {
        let dir = root
            .join(SKILLS_REGISTRY_DIR_NAME)
            .join(registry)
            .join("skills")
            .join(skill_name);
        fs::create_dir_all(&dir)
            .await
            .expect("create registry skill dir");
        let path = dir.join(SKILL_MARKDOWN_FILE);
        fs::write(&path, content)
            .await
            .expect("write registry skill markdown");
        path
    }

    async fn write_registry_skill_at(
        root: &Path,
        registry: &str,
        relative_dir: &str,
        file_name: &str,
        content: &str,
    ) -> PathBuf {
        let dir = root
            .join(SKILLS_REGISTRY_DIR_NAME)
            .join(registry)
            .join(relative_dir);
        fs::create_dir_all(&dir)
            .await
            .expect("create registry nested skill dir");
        let path = dir.join(file_name);
        fs::write(&path, content)
            .await
            .expect("write registry nested skill markdown");
        path
    }

    fn init_local_git_registry_repo(root: &Path, registry: &str, skill_name: &str) -> PathBuf {
        let repo_dir = root.join(format!("repo-{registry}"));
        std::fs::create_dir_all(repo_dir.join("skills").join(skill_name))
            .expect("create repo skill dir");
        std::fs::write(
            repo_dir
                .join("skills")
                .join(skill_name)
                .join(SKILL_MARKDOWN_FILE),
            format!("# {skill_name}"),
        )
        .expect("write repo skill");

        let init = Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&repo_dir)
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed: {:?}", init);

        let add = Command::new("git")
            .args(["add", "."])
            .current_dir(&repo_dir)
            .output()
            .expect("git add");
        assert!(add.status.success(), "git add failed: {:?}", add);

        let commit = Command::new("git")
            .args([
                "-c",
                "user.name=Klaw Test",
                "-c",
                "user.email=klaw-test@example.com",
                "commit",
                "-m",
                "init",
            ])
            .current_dir(&repo_dir)
            .output()
            .expect("git commit");
        assert!(commit.status.success(), "git commit failed: {:?}", commit);

        repo_dir
    }

    #[tokio::test]
    async fn validate_skill_name_rejects_invalid_values() {
        assert!(matches!(
            FileSystemSkillStore::<MockSkillFetcher>::validate_skill_name("../abc"),
            Err(SkillError::InvalidSkillName(_))
        ));
        assert!(matches!(
            FileSystemSkillStore::<MockSkillFetcher>::validate_skill_name(""),
            Err(SkillError::InvalidSkillName(_))
        ));
        assert!(matches!(
            FileSystemSkillStore::<MockSkillFetcher>::validate_skill_name("hello world"),
            Err(SkillError::InvalidSkillName(_))
        ));
        assert_eq!(
            FileSystemSkillStore::<MockSkillFetcher>::validate_skill_name("skill_name-1").unwrap(),
            "skill_name-1"
        );
    }

    #[tokio::test]
    async fn list_and_get_installed_support_empty_and_filled_states() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        let empty = store.list_installed().await.expect("list should work");
        assert!(empty.is_empty());

        let skill_dir = root.join(SKILLS_DIR_NAME).join("demo");
        fs::create_dir_all(&skill_dir)
            .await
            .expect("create skill dir");
        fs::write(skill_dir.join(SKILL_MARKDOWN_FILE), "# demo")
            .await
            .expect("write skill");

        let list = store
            .list_installed()
            .await
            .expect("list should return item");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "demo");

        let record = store.get_installed("demo").await.expect("get should work");
        assert_eq!(record.name, "demo");
        assert_eq!(record.content, "# demo");
    }

    #[tokio::test]
    async fn load_all_installed_skill_markdowns_aggregates_all_items() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());

        for (name, content) in [("b", "# b"), ("a", "# a")] {
            let dir = root.join(SKILLS_DIR_NAME).join(name);
            fs::create_dir_all(&dir).await.expect("create dir");
            fs::write(dir.join(SKILL_MARKDOWN_FILE), content)
                .await
                .expect("write file");
        }

        let records = store
            .load_all_installed_skill_markdowns()
            .await
            .expect("load all should succeed");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "a");
        assert_eq!(records[1].name, "b");
    }

    #[tokio::test]
    async fn install_from_registry_indexes_manifest_without_copying_to_local_skills() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        let registry_path =
            write_registry_skill(&root, "vercel-labs", "find-skills", "# find-skills").await;

        let (record, already_installed) = store
            .install_from_registry("vercel-labs", "find-skills")
            .await
            .expect("install registry skill");
        assert!(!already_installed);
        assert_eq!(record.name, "find-skills");
        assert_eq!(record.source_kind, SkillSourceKind::Registry);
        assert_eq!(record.local_path, registry_path);

        let local_copied_path = root
            .join(SKILLS_DIR_NAME)
            .join("find-skills")
            .join(SKILL_MARKDOWN_FILE);
        let copied_exists = fs::try_exists(&local_copied_path)
            .await
            .expect("check copied file");
        assert!(!copied_exists);

        let manifest = store
            .load_installed_manifest()
            .await
            .expect("load manifest");
        assert_eq!(manifest.managed.len(), 1);
        assert_eq!(manifest.managed[0].registry, "vercel-labs");
        assert_eq!(manifest.managed[0].name, "find-skills");
    }

    #[tokio::test]
    async fn list_and_load_all_merge_registry_and_local_and_registry_wins_on_conflict() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        write_registry_skill(&root, "vercel-labs", "demo", "# demo").await;
        let local_demo = root.join(SKILLS_DIR_NAME).join("demo");
        let local_only = root.join(SKILLS_DIR_NAME).join("local-only");
        fs::create_dir_all(&local_demo)
            .await
            .expect("create local demo");
        fs::create_dir_all(&local_only)
            .await
            .expect("create local-only");
        fs::write(local_demo.join(SKILL_MARKDOWN_FILE), "# local-placeholder")
            .await
            .expect("write local demo");
        fs::write(local_only.join(SKILL_MARKDOWN_FILE), "# local-only")
            .await
            .expect("write local-only");
        store
            .install_from_registry("vercel-labs", "demo")
            .await
            .expect("install managed");

        let list = store.list_installed().await.expect("list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "demo");
        assert_eq!(list[0].source_kind, SkillSourceKind::Registry);
        assert_eq!(list[1].name, "local-only");
        assert_eq!(list[1].source_kind, SkillSourceKind::Local);

        let demo = store.get_installed("demo").await.expect("get managed demo");
        assert_eq!(demo.content, "# demo");
        assert_eq!(demo.source_kind, SkillSourceKind::Registry);

        let all = store
            .load_all_installed_skill_markdowns()
            .await
            .expect("load_all");
        assert_eq!(all.len(), 2);
        let managed = all
            .iter()
            .find(|item| item.name == "demo")
            .expect("managed present");
        assert_eq!(managed.content, "# demo");
        let local = all
            .iter()
            .find(|item| item.name == "local-only")
            .expect("local present");
        assert_eq!(local.content, "# local-only");
    }

    #[tokio::test]
    async fn stale_registry_skills_are_marked_stale_in_records() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        write_registry_skill(&root, "vercel-labs", "demo", "# demo").await;
        let mut manifest = InstalledSkillsManifest::default();
        manifest.managed.push(InstalledSkill {
            registry: "vercel-labs".to_string(),
            name: "demo".to_string(),
        });
        manifest.stale_registries.insert("vercel-labs".to_string());
        store
            .write_installed_manifest(&manifest)
            .await
            .expect("write manifest");

        let record = store
            .get_installed("demo")
            .await
            .expect("get stale managed");
        assert_eq!(record.source_kind, SkillSourceKind::Registry);
        assert_eq!(record.stale, Some(true));
    }

    #[tokio::test]
    async fn uninstall_removes_both_managed_index_and_local_directory() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        write_registry_skill(&root, "vercel-labs", "demo", "# demo").await;
        store
            .install_from_registry("vercel-labs", "demo")
            .await
            .expect("install managed");

        let local_demo = root.join(SKILLS_DIR_NAME).join("demo");
        fs::create_dir_all(&local_demo)
            .await
            .expect("create local demo");
        fs::write(local_demo.join(SKILL_MARKDOWN_FILE), "# local")
            .await
            .expect("write local demo");

        let removed = store.uninstall("demo").await.expect("uninstall");
        assert!(removed.removed_managed);
        assert!(removed.removed_local);

        let manifest = store
            .load_installed_manifest()
            .await
            .expect("load manifest");
        assert!(manifest.managed.is_empty());
        let exists = fs::try_exists(&local_demo)
            .await
            .expect("check local removed");
        assert!(!exists);
    }

    #[tokio::test]
    async fn list_source_skills_reads_registry_catalog() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        let _ = write_registry_skill(&root, "vercel-labs", "folder-one", "# alpha").await;
        let _ = write_registry_skill(&root, "vercel-labs", "folder-two", "# beta").await;

        let skills = store
            .list_source_skills("vercel-labs")
            .await
            .expect("list registry skills");

        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].id, "folder-one");
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].id, "folder-two");
        assert_eq!(skills[1].name, "beta");
    }

    #[tokio::test]
    async fn list_source_skills_recursively_discovers_skill_markdown_anywhere_in_repo() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        let _ = write_registry_skill_at(
            &root,
            "vercel-labs",
            "packages/alpha",
            "SKILL.md",
            "# alpha\nAlpha description",
        )
        .await;
        let _ = write_registry_skill_at(
            &root,
            "vercel-labs",
            "tools/beta",
            "skill.md",
            "name: beta\nBeta description",
        )
        .await;

        let skills = store
            .list_source_skills("vercel-labs")
            .await
            .expect("list registry skills recursively");

        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].id, "packages/alpha");
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].id, "tools/beta");
        assert_eq!(skills[1].name, "beta");
    }

    #[tokio::test]
    async fn install_and_search_registry_skills_support_nested_paths_and_descriptions() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        write_registry_skill_at(
            &root,
            "vercel-labs",
            "agents/finders",
            "SKILL.md",
            "# find-skills\nFind useful skills in repositories",
        )
        .await;

        let (record, already_installed) = store
            .install_from_registry("vercel-labs", "agents/finders")
            .await
            .expect("install nested registry skill by id");
        assert!(!already_installed);
        assert_eq!(record.name, "find-skills");

        let record_by_name = store
            .get_source_skill("vercel-labs", "find-skills")
            .await
            .expect("get nested registry skill by parsed name");
        assert_eq!(record_by_name.name, "find-skills");

        let matches = store
            .search_source_skills("vercel-labs", "useful")
            .await
            .expect("search nested registry skills");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].skill_name, "find-skills");
        assert_eq!(matches[0].description, "Find useful skills in repositories");
    }

    #[tokio::test]
    async fn uninstall_from_registry_removes_only_target_manifest_entry() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        write_registry_skill(&root, "vercel-labs", "demo", "# demo").await;
        write_registry_skill(&root, "openai", "helper", "# helper").await;
        store
            .install_from_registry("vercel-labs", "demo")
            .await
            .expect("install demo");
        store
            .install_from_registry("openai", "helper")
            .await
            .expect("install helper");

        store
            .uninstall_from_registry("vercel-labs", "demo")
            .await
            .expect("uninstall demo");

        let manifest = store
            .load_installed_manifest()
            .await
            .expect("load manifest");
        assert_eq!(manifest.managed.len(), 1);
        assert_eq!(manifest.managed[0].registry, "openai");
        assert_eq!(manifest.managed[0].name, "helper");
    }

    #[tokio::test]
    async fn partial_registry_sync_preserves_other_manifest_entries() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        let openai_repo = init_local_git_registry_repo(&root, "openai", "demo");

        let mut manifest = InstalledSkillsManifest::default();
        manifest.managed.push(InstalledSkill {
            registry: "openclaw".to_string(),
            name: "legacy".to_string(),
        });
        manifest
            .registry_commits
            .insert("openclaw".to_string(), "old-openclaw-commit".to_string());
        manifest.stale_registries.insert("openclaw".to_string());
        store
            .write_installed_manifest(&manifest)
            .await
            .expect("write initial manifest");

        let report = store
            .sync_registry_installed_skills(
                &[RegistrySource {
                    name: "openai".to_string(),
                    address: openai_repo.display().to_string(),
                }],
                &[InstalledSkill {
                    registry: "openai".to_string(),
                    name: "demo".to_string(),
                }],
                5,
            )
            .await
            .expect("partial sync should succeed");

        assert_eq!(report.installed_skills, vec!["demo"]);

        let manifest = store
            .load_installed_manifest()
            .await
            .expect("load final manifest");
        assert!(
            manifest
                .managed
                .iter()
                .any(|item| item.registry == "openclaw" && item.name == "legacy")
        );
        assert!(
            manifest
                .managed
                .iter()
                .any(|item| item.registry == "openai" && item.name == "demo")
        );
        assert_eq!(
            manifest
                .registry_commits
                .get("openclaw")
                .map(String::as_str),
            Some("old-openclaw-commit")
        );
        assert!(manifest.registry_commits.contains_key("openai"));
        assert!(manifest.stale_registries.contains("openclaw"));
    }

    #[test]
    fn parse_git_lock_path_extracts_lock_file_from_stderr() {
        let stderr = "fatal: Unable to create '/tmp/demo/.git/shallow.lock': File exists.\n\nAnother git process seems to be running in this repository, e.g.\nan editor opened by 'git commit'.";

        let path = parse_git_lock_path(stderr).expect("lock path should parse");

        assert_eq!(path, PathBuf::from("/tmp/demo/.git/shallow.lock"));
    }

    #[test]
    fn parse_git_lock_path_ignores_non_lock_git_failures() {
        let stderr = "fatal: couldn't find remote ref main";

        assert!(parse_git_lock_path(stderr).is_none());
    }

    #[tokio::test]
    async fn install_from_registry_rejects_same_name_from_different_registry() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        write_registry_skill(&root, "vercel-labs", "demo", "# demo").await;
        write_registry_skill(&root, "openai", "demo", "# demo").await;

        store
            .install_from_registry("vercel-labs", "demo")
            .await
            .expect("install first registry");
        let err = store
            .install_from_registry("openai", "demo")
            .await
            .expect_err("duplicate managed name should fail");
        assert!(matches!(err, SkillError::InvalidSkillName(_)));
    }
}
