use async_trait::async_trait;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::{ReqwestSkillFetcher, SkillError, SkillFetcher, SkillRecord, SkillSource, SkillStore, SkillSummary};

const DEFAULT_KLAW_DIR: &str = ".klaw";
const SKILLS_DIR_NAME: &str = "skills";
const SKILL_MARKDOWN_FILE: &str = "SKILL.md";

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
        fs::create_dir_all(&self.skills_dir).await.map_err(|source| SkillError::Io {
            op: "create_dir_all",
            path: self.skills_dir.clone(),
            source,
        })
    }

    async fn read_skill_record(&self, skill_name: &str) -> Result<SkillRecord, SkillError> {
        let path = self.skill_markdown_path(skill_name);
        let exists = fs::try_exists(&path).await.map_err(|source| SkillError::Io {
            op: "try_exists",
            path: path.clone(),
            source,
        })?;
        if !exists {
            return Err(SkillError::SkillNotFound(skill_name.to_string()));
        }

        let content = fs::read_to_string(&path).await.map_err(|source| SkillError::Io {
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
        let mut entries = fs::read_dir(&self.skills_dir)
            .await
            .map_err(|source| SkillError::Io {
                op: "read_dir",
                path: self.skills_dir.clone(),
                source,
            })?;

        while let Some(entry) = entries.next_entry().await.map_err(|source| SkillError::Io {
            op: "next_entry",
            path: self.skills_dir.clone(),
            source,
        })? {
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
        fs::create_dir_all(&skill_dir).await.expect("create skill dir");
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
