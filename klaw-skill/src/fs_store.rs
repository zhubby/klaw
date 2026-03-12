use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::{
    ReqwestSkillFetcher, SkillError, SkillFetcher, SkillRecord, SkillSource, SkillStore,
    SkillSummary,
};

const DEFAULT_KLAW_DIR: &str = ".klaw";
const SKILLS_DIR_NAME: &str = "skills";
const SKILLS_REGISTRY_DIR_NAME: &str = "skills-registry";
const SKILL_MARKDOWN_FILE: &str = "SKILL.md";
const INSTALLED_SKILLS_MANIFEST_FILE: &str = "skills-registry-manifest.json";

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
}

#[derive(Clone, Debug)]
pub struct FileSystemSkillStore<F = ReqwestSkillFetcher> {
    root_dir: PathBuf,
    skills_dir: PathBuf,
    fetcher: F,
}

impl FileSystemSkillStore<ReqwestSkillFetcher> {
    pub fn from_home_dir() -> Result<Self, SkillError> {
        let home = std::env::var_os("HOME").ok_or(SkillError::HomeDirUnavailable)?;
        let root = PathBuf::from(home).join(DEFAULT_KLAW_DIR);
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
        let skills_dir = root_dir.join(SKILLS_DIR_NAME);
        Self {
            root_dir,
            skills_dir,
            fetcher,
        }
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    pub fn skills_registry_dir(&self) -> PathBuf {
        self.root_dir.join(SKILLS_REGISTRY_DIR_NAME)
    }

    fn installed_manifest_path(&self) -> PathBuf {
        self.root_dir.join(INSTALLED_SKILLS_MANIFEST_FILE)
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
            INSTALLED_SKILLS_MANIFEST_FILE,
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
                    "installed skill registry cannot be empty".to_string(),
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
        let mut report = RegistrySyncReport::default();
        for (name, address) in &source_map {
            let source = RegistrySource {
                name: name.clone(),
                address: address.clone(),
            };
            self.sync_source_repository(&registry_root, &source).await?;
            report.synced_registries.push(name.clone());
        }

        let mut previous_managed = BTreeSet::new();
        for item in current_manifest.managed {
            previous_managed.insert(item.name);
        }

        let mut desired = BTreeMap::new();
        for (registry_name, requested_name) in requested {
            let repo_dir = registry_root.join(&registry_name);
            let (source_dir, target_name) =
                resolve_registry_skill_dir(&repo_dir, &requested_name, &registry_name).await?;
            if desired
                .insert(target_name.clone(), (registry_name.clone(), source_dir))
                .is_some()
            {
                return Err(SkillError::InvalidSkillName(format!(
                    "duplicate installed skill target `{target_name}`"
                )));
            }
        }

        let mut next_manifest = InstalledSkillsManifest::default();
        let desired_names: BTreeSet<String> = desired.keys().cloned().collect();
        for (target_name, (registry_name, source_dir)) in &desired {
            let target_dir = self.skills_dir.join(target_name);
            let target_exists =
                fs::try_exists(&target_dir)
                    .await
                    .map_err(|source| SkillError::Io {
                        op: "try_exists",
                        path: target_dir.clone(),
                        source,
                    })?;
            if target_exists && !previous_managed.contains(target_name) {
                return Err(SkillError::LocalSkillConflict {
                    skill_name: target_name.clone(),
                    path: target_dir,
                });
            }
            if target_exists {
                fs::remove_dir_all(&target_dir)
                    .await
                    .map_err(|source| SkillError::Io {
                        op: "remove_dir_all",
                        path: target_dir.clone(),
                        source,
                    })?;
            }

            copy_dir_recursive(&source_dir, &target_dir).await?;
            report.installed_skills.push(target_name.clone());
            next_manifest.managed.push(InstalledSkill {
                registry: registry_name.clone(),
                name: target_name.clone(),
            });
        }

        for managed_name in &previous_managed {
            if desired_names.contains(managed_name) {
                continue;
            }
            let target_dir = self.skills_dir.join(managed_name);
            let target_exists =
                fs::try_exists(&target_dir)
                    .await
                    .map_err(|source| SkillError::Io {
                        op: "try_exists",
                        path: target_dir.clone(),
                        source,
                    })?;
            if target_exists {
                fs::remove_dir_all(&target_dir)
                    .await
                    .map_err(|source| SkillError::Io {
                        op: "remove_dir_all",
                        path: target_dir,
                        source,
                    })?;
                report.removed_skills.push(managed_name.clone());
            }
        }

        next_manifest.managed.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.registry.cmp(&b.registry))
        });
        self.write_installed_manifest(&next_manifest).await?;
        report.synced_registries.sort();
        report.installed_skills.sort();
        report.removed_skills.sort();
        Ok(report)
    }

    async fn sync_source_repository(
        &self,
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

    async fn read_skill_record(&self, skill_name: &str) -> Result<SkillRecord, SkillError> {
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
            source: SkillSource::github_anthropic(skill_name),
            local_path: path,
            content,
            updated_at_ms: modified_time_ms(&metadata).unwrap_or_default(),
        })
    }

    async fn write_skill_markdown_atomic(
        &self,
        skill_name: &str,
        content: &str,
    ) -> Result<PathBuf, SkillError> {
        let skill_dir = self.skills_dir.join(skill_name);
        fs::create_dir_all(&skill_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "create_dir_all",
                path: skill_dir.clone(),
                source,
            })?;

        let target = skill_dir.join(SKILL_MARKDOWN_FILE);
        let temp = skill_dir.join(format!("{}.tmp-{}", SKILL_MARKDOWN_FILE, now_ms()));
        fs::write(&temp, content)
            .await
            .map_err(|source| SkillError::Io {
                op: "write",
                path: temp.clone(),
                source,
            })?;
        fs::rename(&temp, &target)
            .await
            .map_err(|source| SkillError::Io {
                op: "rename",
                path: target.clone(),
                source,
            })?;
        Ok(target)
    }
}

#[async_trait]
impl<F> SkillStore for FileSystemSkillStore<F>
where
    F: SkillFetcher,
{
    async fn download(&self, skill_name: &str) -> Result<SkillRecord, SkillError> {
        self.download_with_source(
            skill_name,
            "anthropic",
            "https://raw.githubusercontent.com/anthropics/skills/main/skills/{skill_name}/SKILL.md",
        )
        .await
    }

    async fn download_with_source(
        &self,
        skill_name: &str,
        source_name: &str,
        download_url_template: &str,
    ) -> Result<SkillRecord, SkillError> {
        let name = Self::validate_skill_name(skill_name)?;
        self.ensure_skills_dir().await?;
        let source = SkillSource::configured(source_name, &name, download_url_template);
        let markdown = self.fetcher.fetch_markdown(&source).await?;
        self.write_skill_markdown_atomic(&name, &markdown).await?;
        let mut record = self.read_skill_record(&name).await?;
        record.source = source;
        Ok(record)
    }

    async fn delete(&self, skill_name: &str) -> Result<(), SkillError> {
        let name = Self::validate_skill_name(skill_name)?;
        let skill_dir = self.skills_dir.join(&name);
        let exists = fs::try_exists(&skill_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "try_exists",
                path: skill_dir.clone(),
                source,
            })?;
        if !exists {
            return Err(SkillError::SkillNotFound(name));
        }
        fs::remove_dir_all(&skill_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "remove_dir_all",
                path: skill_dir,
                source,
            })
    }

    async fn list(&self) -> Result<Vec<SkillSummary>, SkillError> {
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
            let skill_md_path = self.skill_markdown_path(name);
            let exists = fs::try_exists(&skill_md_path)
                .await
                .map_err(|source| SkillError::Io {
                    op: "try_exists",
                    path: skill_md_path.clone(),
                    source,
                })?;
            if !exists {
                continue;
            }

            let metadata = fs::metadata(&skill_md_path)
                .await
                .map_err(|source| SkillError::Io {
                    op: "metadata",
                    path: skill_md_path.clone(),
                    source,
                })?;
            items.push(SkillSummary {
                name: name.to_string(),
                local_path: skill_md_path,
                updated_at_ms: modified_time_ms(&metadata).unwrap_or_default(),
            });
        }

        items.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(items)
    }

    async fn get(&self, skill_name: &str) -> Result<SkillRecord, SkillError> {
        let name = Self::validate_skill_name(skill_name)?;
        self.read_skill_record(&name).await
    }

    async fn update(&self, skill_name: &str) -> Result<SkillRecord, SkillError> {
        self.download(skill_name).await
    }

    async fn update_with_source(
        &self,
        skill_name: &str,
        source_name: &str,
        download_url_template: &str,
    ) -> Result<SkillRecord, SkillError> {
        self.download_with_source(skill_name, source_name, download_url_template)
            .await
    }

    async fn load_all_skill_markdowns(&self) -> Result<Vec<SkillRecord>, SkillError> {
        let skills = self.list().await?;
        let mut records = Vec::with_capacity(skills.len());
        for skill in skills {
            records.push(self.get(&skill.name).await?);
        }
        records.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(records)
    }
}

pub fn open_default_skill_store() -> Result<FileSystemSkillStore<ReqwestSkillFetcher>, SkillError> {
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

async fn resolve_registry_skill_dir(
    registry_repo_dir: &Path,
    requested_name: &str,
    registry_name: &str,
) -> Result<(PathBuf, String), SkillError> {
    let direct_dir = registry_repo_dir.join("skills").join(requested_name);
    let direct_md = direct_dir.join(SKILL_MARKDOWN_FILE);
    let direct_exists = fs::try_exists(&direct_md)
        .await
        .map_err(|source| SkillError::Io {
            op: "try_exists",
            path: direct_md.clone(),
            source,
        })?;
    if direct_exists {
        return Ok((direct_dir, requested_name.to_string()));
    }

    let skills_root = registry_repo_dir.join("skills");
    let skills_root_exists =
        fs::try_exists(&skills_root)
            .await
            .map_err(|source| SkillError::Io {
                op: "try_exists",
                path: skills_root.clone(),
                source,
            })?;
    if !skills_root_exists {
        return Err(SkillError::RegistrySkillNotFound {
            registry: registry_name.to_string(),
            skill_name: requested_name.to_string(),
            path: direct_md,
        });
    }

    let mut entries = fs::read_dir(&skills_root)
        .await
        .map_err(|source| SkillError::Io {
            op: "read_dir",
            path: skills_root.clone(),
            source,
        })?;
    let mut matched: Option<(PathBuf, String)> = None;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|source| SkillError::Io {
            op: "next_entry",
            path: skills_root.clone(),
            source,
        })?
    {
        let candidate_dir = entry.path();
        if !is_directory(&candidate_dir, &entry).await? {
            continue;
        }
        let candidate_md = candidate_dir.join(SKILL_MARKDOWN_FILE);
        let candidate_exists =
            fs::try_exists(&candidate_md)
                .await
                .map_err(|source| SkillError::Io {
                    op: "try_exists",
                    path: candidate_md.clone(),
                    source,
                })?;
        if !candidate_exists {
            continue;
        }
        let content = fs::read_to_string(&candidate_md)
            .await
            .map_err(|source| SkillError::Io {
                op: "read_to_string",
                path: candidate_md.clone(),
                source,
            })?;
        let Some(parsed_name) = parse_skill_name_from_markdown(&content) else {
            continue;
        };
        if parsed_name != requested_name {
            continue;
        }
        let Some(folder_name) = candidate_dir
            .file_name()
            .and_then(OsStr::to_str)
            .map(ToString::to_string)
        else {
            continue;
        };
        if matched.is_some() {
            return Err(SkillError::InvalidSkillName(format!(
                "skill name `{requested_name}` in registry `{registry_name}` is ambiguous"
            )));
        }
        matched = Some((candidate_dir, folder_name));
    }

    matched.ok_or_else(|| SkillError::RegistrySkillNotFound {
        registry: registry_name.to_string(),
        skill_name: requested_name.to_string(),
        path: direct_md,
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

async fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), SkillError> {
    let source_path = source.to_path_buf();
    let target_path = target.to_path_buf();
    tokio::task::spawn_blocking(move || copy_dir_recursive_blocking(&source_path, &target_path))
        .await
        .map_err(|source_err| SkillError::Io {
            op: "join copy task",
            path: target.to_path_buf(),
            source: std::io::Error::other(source_err.to_string()),
        })?
}

fn copy_dir_recursive_blocking(source: &Path, target: &Path) -> Result<(), SkillError> {
    std::fs::create_dir_all(target).map_err(|source_err| SkillError::Io {
        op: "create_dir_all",
        path: target.to_path_buf(),
        source: source_err,
    })?;

    let entries = std::fs::read_dir(source).map_err(|source_err| SkillError::Io {
        op: "read_dir",
        path: source.to_path_buf(),
        source: source_err,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source_err| SkillError::Io {
            op: "read_dir_entry",
            path: source.to_path_buf(),
            source: source_err,
        })?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let metadata = entry.metadata().map_err(|source_err| SkillError::Io {
            op: "metadata",
            path: from.clone(),
            source: source_err,
        })?;
        if metadata.is_dir() {
            copy_dir_recursive_blocking(&from, &to)?;
            continue;
        }
        if metadata.is_file() {
            std::fs::copy(&from, &to).map_err(|source_err| SkillError::Io {
                op: "copy",
                path: from,
                source: source_err,
            })?;
        }
    }
    Ok(())
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
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new("git");
        command.args(&args_owned);
        if let Some(dir) = cwd_owned {
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
                command: format!("git {}", args_owned.join(" ")),
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[derive(Default)]
    struct MockSkillFetcher {
        payloads: Arc<Mutex<BTreeMap<String, String>>>,
    }

    impl MockSkillFetcher {
        fn insert(&self, skill: &str, content: &str) {
            self.payloads
                .lock()
                .expect("lock payloads")
                .insert(skill.to_string(), content.to_string());
        }
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
    async fn list_and_get_support_empty_and_filled_states() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        let empty = store.list().await.expect("list should work");
        assert!(empty.is_empty());

        let skill_dir = root.join(SKILLS_DIR_NAME).join("demo");
        fs::create_dir_all(&skill_dir)
            .await
            .expect("create skill dir");
        fs::write(skill_dir.join(SKILL_MARKDOWN_FILE), "# demo")
            .await
            .expect("write skill");

        let list = store.list().await.expect("list should return item");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "demo");

        let record = store.get("demo").await.expect("get should work");
        assert_eq!(record.name, "demo");
        assert_eq!(record.content, "# demo");
    }

    #[tokio::test]
    async fn delete_handles_exists_and_not_found() {
        let root = test_root();
        let store = FileSystemSkillStore::with_fetcher(root.clone(), MockSkillFetcher::default());
        let skill_dir = root.join(SKILLS_DIR_NAME).join("dead");
        fs::create_dir_all(&skill_dir).await.expect("create dir");
        fs::write(skill_dir.join(SKILL_MARKDOWN_FILE), "dead")
            .await
            .expect("write file");

        store.delete("dead").await.expect("delete should succeed");
        let still_exists = fs::try_exists(&skill_dir).await.expect("try_exists");
        assert!(!still_exists);

        let err = store.delete("dead").await.expect_err("must fail");
        assert!(matches!(err, SkillError::SkillNotFound(name) if name == "dead"));
    }

    #[tokio::test]
    async fn load_all_skill_markdowns_aggregates_all_items() {
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
            .load_all_skill_markdowns()
            .await
            .expect("load all should succeed");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "a");
        assert_eq!(records[1].name, "b");
    }

    #[tokio::test]
    async fn download_and_update_use_fetcher_without_real_network() {
        let root = test_root();
        let fetcher = MockSkillFetcher::default();
        fetcher.insert("planner", "# v1");
        let store = FileSystemSkillStore::with_fetcher(root.clone(), fetcher);

        let first = store.download("planner").await.expect("download");
        assert_eq!(first.content, "# v1");

        let fetcher2 = MockSkillFetcher::default();
        fetcher2.insert("planner", "# v2");
        let store2 = FileSystemSkillStore::with_fetcher(root.clone(), fetcher2);
        let updated = store2.update("planner").await.expect("update");
        assert_eq!(updated.content, "# v2");
    }
}
