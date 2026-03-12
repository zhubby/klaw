use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_skill::{open_default_skill_store, FileSystemSkillStore, ReqwestSkillFetcher, SkillStore};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

#[derive(Debug, Clone)]
struct SkillSourceDef {
    name: String,
    download_url_template: String,
}

pub struct SkillsRegistryTool {
    store: FileSystemSkillStore<ReqwestSkillFetcher>,
    sources: BTreeMap<String, SkillSourceDef>,
}

impl SkillsRegistryTool {
    pub fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let store = open_default_skill_store()
            .map_err(|err| ToolError::ExecutionFailed(format!("open skill store failed: {err}")))?;
        let mut sources = BTreeMap::new();
        for source in &config.skills.sources {
            let download_url_template = resolve_download_template(&source.address)?;
            sources.insert(
                source.name.clone(),
                SkillSourceDef {
                    name: source.name.clone(),
                    download_url_template,
                },
            );
        }
        if sources.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "skills sources are empty; please configure [skills] sources in config.toml"
                    .to_string(),
            ));
        }
        Ok(Self {
            store,
            sources,
        })
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

    fn require_source(args: &Value) -> Result<&str, ToolError> {
        args.get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `source`".to_string()))
    }

    fn resolve_source<'a>(&'a self, args: &'a Value) -> Result<&'a SkillSourceDef, ToolError> {
        let source_name = Self::require_source(args)?;
        self.sources.get(source_name).ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "unknown `source` `{source_name}`; available sources: {}",
                self.sources.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })
    }

    fn resolve_source_optional<'a>(&'a self, args: &'a Value) -> Result<Option<&'a SkillSourceDef>, ToolError> {
        let source_name = args
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let Some(source_name) = source_name else {
            return Ok(None);
        };
        let resolved = self.sources.get(source_name).ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "unknown `source` `{source_name}`; available sources: {}",
                self.sources.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })?;
        Ok(Some(resolved))
    }

    async fn do_download(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let source = self.resolve_source(args)?;
        let record = self
            .store
            .download_with_source(skill_name, &source.name, &source.download_url_template)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "download",
            "source": source.name,
            "skill": record
        }))
    }

    async fn do_delete(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        self.store
            .delete(skill_name)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "delete",
            "skill_name": skill_name,
            "deleted": true
        }))
    }

    async fn do_list(&self) -> Result<Value, ToolError> {
        let items = self.store.list().await.map_err(map_skill_err)?;
        Ok(json!({
            "action": "list",
            "items": items
        }))
    }

    async fn do_get(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let record = self.store.get(skill_name).await.map_err(map_skill_err)?;
        let source = self.resolve_source_optional(args)?;
        Ok(json!({
            "action": "get",
            "source": source.map(|it| it.name.clone()),
            "skill": record
        }))
    }

    async fn do_update(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let source = self.resolve_source(args)?;
        let record = self
            .store
            .update_with_source(skill_name, &source.name, &source.download_url_template)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "update",
            "source": source.name,
            "skill": record
        }))
    }

    async fn do_load_all(&self) -> Result<Value, ToolError> {
        let items = self
            .store
            .load_all_skill_markdowns()
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "load_all",
            "sources": self.sources.keys().cloned().collect::<Vec<_>>(),
            "items": items
        }))
    }
}

#[async_trait]
impl Tool for SkillsRegistryTool {
    fn name(&self) -> &str {
        "skills_registry"
    }

    fn description(&self) -> &str {
        "Manage local skills in ~/.klaw/skills and sync SKILL.md from configurable repositories. Supports download/update/delete/list/get/load_all actions."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Manage local skill files and load all skill markdown for prompt injection.",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["download", "delete", "list", "get", "update", "load_all"]
                },
                "skill_name": {
                    "type": "string",
                    "description": "Required for download/delete/get/update. Skill folder name under source repository."
                },
                "source": {
                    "type": "string",
                    "description": "Required for download/update. Optional for get. Source key from top-level config `skills.sources[*].name`."
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemWrite
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let result = match action {
            "download" => self.do_download(&args).await?,
            "delete" => self.do_delete(&args).await?,
            "list" => self.do_list().await?,
            "get" => self.do_get(&args).await?,
            "update" => self.do_update(&args).await?,
            "load_all" => self.do_load_all().await?,
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of download/delete/list/get/update/load_all".to_string(),
                ))
            }
        };

        let rendered = serde_json::to_string_pretty(&result).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize skills_registry output failed: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }
}

fn map_skill_err(err: klaw_skill::SkillError) -> ToolError {
    ToolError::ExecutionFailed(err.to_string())
}

fn resolve_download_template(raw_value: &str) -> Result<String, ToolError> {
    let value = raw_value.trim();
    if value.is_empty() {
        return Err(ToolError::InvalidArgs(
            "skills source cannot be empty".to_string(),
        ));
    }
    if value.contains("{skill_name}") {
        return Ok(value.to_string());
    }
    if let Some((owner, repo)) = parse_github_owner_repo(value) {
        return Ok(format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/main/skills/{{skill_name}}/SKILL.md"
        ));
    }
    Err(ToolError::InvalidArgs(format!(
        "invalid skills source `{value}`: expected GitHub URL/shorthand or template containing {{skill_name}}"
    )))
}

fn parse_github_owner_repo(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim().trim_end_matches('/');

    if let Some((owner, repo)) = parse_owner_repo_pair(trimmed) {
        return Some((owner, repo));
    }

    let without_protocol = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let without_prefix = without_protocol.strip_prefix("github.com/")?;
    parse_owner_repo_pair(without_prefix)
}

fn parse_owner_repo_pair(value: &str) -> Option<(String, String)> {
    let mut parts = value.split('/').filter(|segment| !segment.is_empty());
    let owner = parts.next()?;
    let mut repo = parts.next()?.to_string();
    if repo.ends_with(".git") {
        repo.truncate(repo.len().saturating_sub(4));
    }
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo))
}
