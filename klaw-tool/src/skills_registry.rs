use async_trait::async_trait;
use klaw_config::{AppConfig, ConfigStore, SkillsRegistryConfig};
use klaw_skill::{
    FileSystemSkillStore, RegistrySyncStatus, ReqwestSkillFetcher, SkillsRegistry,
    open_default_skill_registry,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tokio::time::{Duration, timeout};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const SYNC_TIMEOUT_FLOOR_SECS: u64 = 1;

pub struct SkillsRegistryTool {
    store: FileSystemSkillStore<ReqwestSkillFetcher>,
    config_store: ConfigStore,
}

impl SkillsRegistryTool {
    pub fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let store = open_default_skill_registry().map_err(|err| {
            ToolError::ExecutionFailed(format!("open skills registry failed: {err}"))
        })?;
        let config_store = ConfigStore::open(None).map_err(map_config_err)?;
        let _ = config;
        Ok(Self {
            store,
            config_store,
        })
    }

    #[cfg(test)]
    fn new(store: FileSystemSkillStore<ReqwestSkillFetcher>, config_store: ConfigStore) -> Self {
        Self {
            store,
            config_store,
        }
    }

    fn require_action(args: &Value) -> Result<&str, ToolError> {
        args.get("action")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `action`".to_string()))
    }

    fn require_skill_name(args: &Value) -> Result<&str, ToolError> {
        args.get("skill_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `skill_name`".to_string()))
    }

    fn require_query(args: &Value) -> Result<&str, ToolError> {
        args.get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `query`".to_string()))
    }

    fn require_address(args: &Value) -> Result<&str, ToolError> {
        args.get("address")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `address`".to_string()))
    }

    fn parse_source(args: &Value) -> Option<&str> {
        args.get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
    }

    fn parse_limit(args: &Value) -> Result<usize, ToolError> {
        let Some(raw) = args.get("limit") else {
            return Ok(20);
        };
        let value = raw
            .as_u64()
            .ok_or_else(|| ToolError::InvalidArgs("`limit` must be an integer".to_string()))?;
        if !(1..=100).contains(&value) {
            return Err(ToolError::InvalidArgs(
                "`limit` must be in range [1, 100]".to_string(),
            ));
        }
        Ok(value as usize)
    }

    fn load_config(&self) -> AppConfig {
        self.config_store.snapshot().config
    }

    fn save_config(&self, config: &AppConfig) -> Result<AppConfig, ToolError> {
        let raw = toml::to_string_pretty(config).map_err(|err| {
            ToolError::ExecutionFailed(format!("render skills registry config failed: {err}"))
        })?;
        let saved = self
            .config_store
            .save_raw_toml(&raw)
            .map_err(map_config_err)?;
        Ok(saved.config)
    }

    fn configured_sources(config: &AppConfig) -> BTreeMap<String, SkillsRegistryConfig> {
        config.skills.registries.clone()
    }

    fn resolve_source<'a>(
        config: &'a AppConfig,
        source_name: &str,
    ) -> Result<&'a SkillsRegistryConfig, ToolError> {
        config.skills.registries.get(source_name).ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "unknown `source` `{source_name}`; available sources: {}",
                config
                    .skills
                    .registries
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
    }

    fn resolve_source_from_args<'a>(
        config: &'a AppConfig,
        args: &'a Value,
    ) -> Result<(&'a str, &'a SkillsRegistryConfig), ToolError> {
        let source_name = Self::parse_source(args)
            .ok_or_else(|| ToolError::InvalidArgs("missing `source`".to_string()))?;
        let source = Self::resolve_source(config, source_name)?;
        Ok((source_name, source))
    }

    fn resolve_source_optional<'a>(
        config: &'a AppConfig,
        args: &'a Value,
    ) -> Result<Option<(&'a str, &'a SkillsRegistryConfig)>, ToolError> {
        let Some(source_name) = Self::parse_source(args) else {
            return Ok(None);
        };
        let source = Self::resolve_source(config, source_name)?;
        Ok(Some((source_name, source)))
    }

    fn parse_source_name(input: &str) -> Result<String, ToolError> {
        let value = input.trim();
        if value.is_empty() {
            return Err(ToolError::InvalidArgs(
                "source name cannot be empty".to_string(),
            ));
        }
        let valid = value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
        if !valid {
            return Err(ToolError::InvalidArgs(format!(
                "invalid `source` `{value}`; allowed characters: letters, numbers, `-`, `_`"
            )));
        }
        Ok(value.to_string())
    }

    fn infer_source_name(address: &str) -> Option<String> {
        let trimmed = address.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return None;
        }
        let candidate = trimmed
            .rsplit('/')
            .next()
            .unwrap_or(trimmed)
            .rsplit(':')
            .next()
            .unwrap_or(trimmed)
            .trim_end_matches(".git")
            .trim();
        Self::parse_source_name(candidate).ok()
    }

    fn make_unique_source_name(base: &str, existing: &BTreeSet<String>) -> String {
        if !existing.contains(base) {
            return base.to_string();
        }
        let mut idx = 2;
        loop {
            let candidate = format!("{base}-{idx}");
            if !existing.contains(&candidate) {
                return candidate;
            }
            idx += 1;
        }
    }

    async fn sync_source_with_timeout(
        &self,
        source_name: &str,
        address: &str,
        sync_timeout_secs: u64,
    ) -> Result<RegistrySyncStatus, ToolError> {
        timeout(
            Duration::from_secs(sync_timeout_secs.max(SYNC_TIMEOUT_FLOOR_SECS)),
            self.store.sync_source(source_name, address),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "sync timed out after {}s for source `{source_name}`",
                sync_timeout_secs.max(SYNC_TIMEOUT_FLOOR_SECS)
            ))
        })?
        .map_err(map_skill_err)
    }

    async fn do_list(&self, args: &Value) -> Result<Value, ToolError> {
        let config = self.load_config();
        let (source_name, _source) = Self::resolve_source_from_args(&config, args)?;
        let items = self
            .store
            .list_source_skills(source_name)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "list",
            "source": source_name,
            "items": items
        }))
    }

    async fn do_show(&self, args: &Value) -> Result<Value, ToolError> {
        let config = self.load_config();
        let (source_name, _source) = Self::resolve_source_from_args(&config, args)?;
        let skill_name = Self::require_skill_name(args)?;
        let record = self
            .store
            .get_source_skill(source_name, skill_name)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "show",
            "source": source_name,
            "skill": record
        }))
    }

    async fn do_search(&self, args: &Value) -> Result<Value, ToolError> {
        let config = self.load_config();
        let query = Self::require_query(args)?;
        let source = Self::resolve_source_optional(&config, args)?;
        let limit = Self::parse_limit(args)?;
        let source_names = match source {
            Some((source_name, _)) => vec![source_name.to_string()],
            None => Self::configured_sources(&config)
                .into_keys()
                .collect::<Vec<_>>(),
        };

        let mut matches = Vec::new();
        let mut scanned_sources = Vec::with_capacity(source_names.len());
        for source_name in source_names {
            scanned_sources.push(source_name.clone());
            let mut source_matches = self
                .store
                .search_source_skills(&source_name, query)
                .await
                .map_err(map_skill_err)?;
            matches.append(&mut source_matches);
        }

        matches.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.skill_name.cmp(&b.skill_name))
        });
        if matches.len() > limit {
            matches.truncate(limit);
        }

        Ok(json!({
            "action": "search",
            "query": query,
            "source": source.map(|(source_name, _)| source_name.to_string()),
            "scanned_sources": scanned_sources,
            "items": matches
        }))
    }

    async fn do_add(&self, args: &Value) -> Result<Value, ToolError> {
        let address = Self::require_address(args)?;
        let current = self.load_config();
        let base_source = match Self::parse_source(args) {
            Some(value) => Self::parse_source_name(value)?,
            None => Self::infer_source_name(address).ok_or_else(|| {
                ToolError::InvalidArgs(
                    "cannot infer `source` from `address`; please provide `source` explicitly"
                        .to_string(),
                )
            })?,
        };
        let existing = current
            .skills
            .registries
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        let final_source = Self::make_unique_source_name(&base_source, &existing);
        let auto_renamed = final_source != base_source;

        let mut next = current.clone();
        next.skills.registries.insert(
            final_source.clone(),
            SkillsRegistryConfig {
                address: address.to_string(),
                installed: Vec::new(),
            },
        );

        self.save_config(&next)?;
        let sync_result = self
            .sync_source_with_timeout(&final_source, address, next.skills.sync_timeout)
            .await;
        let sync_status = match sync_result {
            Ok(status) => status,
            Err(err) => {
                let rollback = self.save_config(&current);
                return match rollback {
                    Ok(_) => Err(err),
                    Err(rollback_err) => Err(ToolError::ExecutionFailed(format!(
                        "{err}; rollback failed: {rollback_err}"
                    ))),
                };
            }
        };

        Ok(json!({
            "action": "add",
            "source": final_source,
            "address": address,
            "auto_renamed": auto_renamed,
            "commit": sync_status.commit,
            "is_stale": sync_status.is_stale,
            "local_path": self.store.skills_registry_dir().join(sync_status.registry_name),
        }))
    }

    async fn do_sync(&self, args: &Value) -> Result<Value, ToolError> {
        let config = self.load_config();
        let (source_name, source) = Self::resolve_source_from_args(&config, args)?;
        let sync_status = self
            .sync_source_with_timeout(source_name, &source.address, config.skills.sync_timeout)
            .await?;
        Ok(json!({
            "action": "sync",
            "source": source_name,
            "address": source.address,
            "commit": sync_status.commit,
            "is_stale": sync_status.is_stale,
            "local_path": self.store.skills_registry_dir().join(source_name),
        }))
    }

    async fn do_delete(&self, args: &Value) -> Result<Value, ToolError> {
        let current = self.load_config();
        let (source_name, source) = Self::resolve_source_from_args(&current, args)?;
        let source_name = source_name.to_string();
        let source_address = source.address.clone();
        let mut next = current.clone();
        next.skills.registries.remove(&source_name);
        self.save_config(&next)?;

        let deleted = self.store.delete_source(&source_name).await;
        let delete_report = match deleted {
            Ok(report) => report,
            Err(err) => {
                let rollback = self.save_config(&current);
                return match rollback {
                    Ok(_) => Err(map_skill_err(err)),
                    Err(rollback_err) => Err(ToolError::ExecutionFailed(format!(
                        "{}; rollback failed: {rollback_err}",
                        map_skill_err(err)
                    ))),
                };
            }
        };
        Ok(json!({
            "action": "delete",
            "source": source_name,
            "address": source_address,
            "removed_managed_count": delete_report.removed_managed_count,
            "removed_local_clone": delete_report.removed_local_clone,
        }))
    }
}

#[async_trait]
impl Tool for SkillsRegistryTool {
    fn name(&self) -> &str {
        "skills_registry"
    }

    fn description(&self) -> &str {
        "Manage configured skills registries under ~/.klaw/skills-registry. Supports add/sync/delete for registry sources and list/show/search for synced registry skills."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Manage configured registry mirrors and browse synced registry skills. `add` persists `[skills.<source>]` into config.toml and immediately syncs the repository.",
            "oneOf": [
                {
                    "description": "Add a new skills registry source and immediately sync it. When `source` is omitted the tool derives it from the git repository name; conflicts auto-rename to `<name>-2`, `<name>-3`, etc.",
                    "properties": {
                        "action": { "const": "add" },
                        "address": {
                            "type": "string",
                            "description": "Git repository URL or clone address for the registry source."
                        },
                        "source": {
                            "type": "string",
                            "description": "Optional source key to persist under `[skills.<source>]`. Must contain only letters, numbers, `-`, `_`."
                        }
                    },
                    "required": ["action", "address"],
                    "additionalProperties": false
                },
                {
                    "description": "Sync one configured source registry into the local mirror cache under ~/.klaw/skills-registry/<source>.",
                    "properties": {
                        "action": { "const": "sync" },
                        "source": {
                            "type": "string",
                            "description": "Configured registry key (`[skills.<source>]`)."
                        }
                    },
                    "required": ["action", "source"],
                    "additionalProperties": false
                },
                {
                    "description": "Delete one configured source registry, remove its local mirror clone, and clear its managed manifest entries.",
                    "properties": {
                        "action": { "const": "delete" },
                        "source": {
                            "type": "string",
                            "description": "Configured registry key (`[skills.<source>]`)."
                        }
                    },
                    "required": ["action", "source"],
                    "additionalProperties": false
                },
                {
                    "description": "List skills available in one configured source registry.",
                    "properties": {
                        "action": { "const": "list" },
                        "source": {
                            "type": "string",
                            "description": "Configured registry key (`[skills.<registry>]`)."
                        }
                    },
                    "required": ["action", "source"],
                    "additionalProperties": false
                },
                {
                    "description": "Show one registry skill from a configured source.",
                    "properties": {
                        "action": { "const": "show" },
                        "source": {
                            "type": "string",
                            "description": "Configured registry key (`[skills.<registry>]`)."
                        },
                        "skill_name": {
                            "type": "string",
                            "description": "Skill folder name or resolved skill name inside the selected source."
                        }
                    },
                    "required": ["action", "source", "skill_name"],
                    "additionalProperties": false
                },
                {
                    "description": "Search registry mirrors by keyword against skill name and extracted description.",
                    "properties": {
                        "action": { "const": "search" },
                        "query": {
                            "type": "string",
                            "description": "Matched against skill name and extracted SKILL.md description."
                        },
                        "source": {
                            "type": "string",
                            "description": "Optional source filter; when omitted all configured sources are scanned."
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 100,
                            "description": "Max number of returned matches. Defaults to 20."
                        }
                    },
                    "required": ["action", "query"],
                    "additionalProperties": false
                }
            ],
            "examples": [
                { "action": "add", "address": "https://github.com/example/skills.git" },
                { "action": "sync", "source": "vercel" },
                { "action": "delete", "source": "vercel" },
                { "action": "list", "source": "vercel" },
                { "action": "show", "source": "vercel", "skill_name": "find-skills" },
                { "action": "search", "query": "rust cli", "limit": 10 }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemWrite
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let result = match action {
            "add" => self.do_add(&args).await?,
            "sync" => self.do_sync(&args).await?,
            "delete" => self.do_delete(&args).await?,
            "list" => self.do_list(&args).await?,
            "show" => self.do_show(&args).await?,
            "search" => self.do_search(&args).await?,
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of add/sync/delete/list/show/search".to_string(),
                ));
            }
        };

        let rendered = serde_json::to_string_pretty(&result).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize skills_registry output failed: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
            signals: Vec::new(),
        })
    }
}

fn map_skill_err(err: klaw_skill::SkillError) -> ToolError {
    ToolError::ExecutionFailed(err.to_string())
}

fn map_config_err(err: klaw_config::ConfigError) -> ToolError {
    ToolError::ExecutionFailed(format!("config operation failed: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::AppConfig;
    use klaw_util::{SKILLS_REGISTRY_DIR_NAME, config_path};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_root() -> PathBuf {
        let nonce = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "klaw-tool-skills-registry-test-{timestamp}-{nonce}"
        ))
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_key: "session-1".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    fn init_local_git_registry_repo(
        root: &std::path::Path,
        registry: &str,
        skill_name: &str,
    ) -> PathBuf {
        let repo_dir = root.join(format!("repo-{registry}"));
        let _ = std::fs::remove_dir_all(&repo_dir);
        std::fs::create_dir_all(repo_dir.join("skills").join(skill_name))
            .expect("create repo skill dir");
        std::fs::write(
            repo_dir.join("skills").join(skill_name).join("SKILL.md"),
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

    fn tool_with_config(root: PathBuf, config: AppConfig) -> SkillsRegistryTool {
        std::fs::create_dir_all(&root).expect("create root");
        let path = config_path(&root);
        std::fs::write(
            &path,
            toml::to_string_pretty(&AppConfig::default()).expect("serialize default config"),
        )
        .expect("write initial config file");
        let config_store = ConfigStore::open(Some(&path)).expect("config store should open");
        let raw = toml::to_string_pretty(&config).expect("serialize config");
        config_store
            .save_raw_toml(&raw)
            .expect("save initial config");
        let store = FileSystemSkillStore::from_root_dir(root);
        SkillsRegistryTool::new(store, config_store)
    }

    #[tokio::test]
    async fn add_infers_source_and_syncs_repo() {
        let root = test_root();
        let repo_dir = init_local_git_registry_repo(&root, "demo-source", "demo");
        let mut config = AppConfig::default();
        config.skills.registries.clear();
        let tool = tool_with_config(root.clone(), config);

        let output = tool
            .execute(
                json!({
                    "action": "add",
                    "address": repo_dir.display().to_string()
                }),
                &test_ctx(),
            )
            .await
            .expect("add should succeed");
        let payload: Value =
            serde_json::from_str(&output.content_for_model).expect("json output should parse");
        assert_eq!(payload["source"], "repo-demo-source");
        assert_eq!(payload["auto_renamed"], false);

        let config_store =
            ConfigStore::open(Some(&config_path(&root))).expect("config store should reopen");
        assert!(
            config_store
                .snapshot()
                .config
                .skills
                .registries
                .contains_key("repo-demo-source")
        );
        let clone_dir = root.join(SKILLS_REGISTRY_DIR_NAME).join("repo-demo-source");
        assert!(clone_dir.exists());
    }

    #[tokio::test]
    async fn add_auto_renames_when_source_conflicts() {
        let root = test_root();
        let repo_dir = init_local_git_registry_repo(&root, "alpha", "demo");
        let mut config = AppConfig::default();
        config.skills.registries.clear();
        config.skills.registries.insert(
            "alpha".to_string(),
            SkillsRegistryConfig {
                address: "https://example.com/original.git".to_string(),
                installed: Vec::new(),
            },
        );
        let tool = tool_with_config(root.clone(), config);

        let output = tool
            .execute(
                json!({
                    "action": "add",
                    "address": repo_dir.display().to_string(),
                    "source": "alpha"
                }),
                &test_ctx(),
            )
            .await
            .expect("add should succeed");
        let payload: Value =
            serde_json::from_str(&output.content_for_model).expect("json output should parse");
        assert_eq!(payload["source"], "alpha-2");
        assert_eq!(payload["auto_renamed"], true);
    }

    #[tokio::test]
    async fn sync_reports_missing_source() {
        let root = test_root();
        let mut config = AppConfig::default();
        config.skills.registries.clear();
        let tool = tool_with_config(root, config);

        let err = tool
            .execute(
                json!({
                    "action": "sync",
                    "source": "missing"
                }),
                &test_ctx(),
            )
            .await
            .expect_err("sync should fail");
        assert!(err.to_string().contains("unknown `source` `missing`"));
    }

    #[tokio::test]
    async fn delete_removes_config_clone_and_manifest_entries() {
        let root = test_root();
        let repo_dir = init_local_git_registry_repo(&root, "demo-source", "demo");
        let mut config = AppConfig::default();
        config.skills.registries.clear();
        config.skills.registries.insert(
            "demo-source".to_string(),
            SkillsRegistryConfig {
                address: repo_dir.display().to_string(),
                installed: vec!["demo".to_string()],
            },
        );
        let tool = tool_with_config(root.clone(), config);
        tool.store
            .sync_source("demo-source", &repo_dir.display().to_string())
            .await
            .expect("sync source should succeed");
        tool.store
            .install_from_registry("demo-source", "demo")
            .await
            .expect("install managed skill");

        let output = tool
            .execute(
                json!({
                    "action": "delete",
                    "source": "demo-source"
                }),
                &test_ctx(),
            )
            .await
            .expect("delete should succeed");
        let payload: Value =
            serde_json::from_str(&output.content_for_model).expect("json output should parse");
        assert_eq!(payload["removed_managed_count"], 1);
        assert_eq!(payload["removed_local_clone"], true);

        let config_store =
            ConfigStore::open(Some(&config_path(&root))).expect("config store should reopen");
        assert!(
            !config_store
                .snapshot()
                .config
                .skills
                .registries
                .contains_key("demo-source")
        );
        assert!(
            !root
                .join(SKILLS_REGISTRY_DIR_NAME)
                .join("demo-source")
                .exists()
        );
    }

    #[tokio::test]
    async fn list_works_for_source_added_from_empty_config() {
        let root = test_root();
        let repo_dir = init_local_git_registry_repo(&root, "catalog", "demo");
        let mut config = AppConfig::default();
        config.skills.registries.clear();
        let tool = tool_with_config(root, config);

        tool.execute(
            json!({
                "action": "add",
                "address": repo_dir.display().to_string(),
                "source": "catalog"
            }),
            &test_ctx(),
        )
        .await
        .expect("add should succeed");

        let output = tool
            .execute(
                json!({
                    "action": "list",
                    "source": "catalog"
                }),
                &test_ctx(),
            )
            .await
            .expect("list should succeed");
        assert!(output.content_for_model.contains("\"demo\""));
    }
}
