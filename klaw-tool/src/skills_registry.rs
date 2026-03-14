use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_skill::{open_default_skill_store, FileSystemSkillStore, ReqwestSkillFetcher, SkillStore};
use serde_json::{json, Value};
use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf};
use tokio::fs;
use tokio::time::{timeout, Duration};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

#[derive(Debug, Clone)]
struct SkillSourceDef {
    name: String,
}

pub struct SkillsRegistryTool {
    store: FileSystemSkillStore<ReqwestSkillFetcher>,
    sources: BTreeMap<String, SkillSourceDef>,
}

#[derive(Debug, Clone)]
struct RegistrySearchItem {
    source: String,
    skill_name: String,
    description: String,
    local_path: PathBuf,
    score: i32,
    matched_fields: Vec<String>,
}

const INSTALL_TIMEOUT_SECS: u64 = 15;

impl SkillsRegistryTool {
    pub fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let store = open_default_skill_store()
            .map_err(|err| ToolError::ExecutionFailed(format!("open skill store failed: {err}")))?;
        let mut sources = BTreeMap::new();
        for (source_name, registry) in &config.skills.registries {
            let _download_url_template = resolve_download_template(&registry.address)?;
            sources.insert(
                source_name.clone(),
                SkillSourceDef {
                    name: source_name.clone(),
                },
            );
        }
        if sources.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "skills registries are empty; please configure [skills.<registry>] in config.toml"
                    .to_string(),
            ));
        }
        Ok(Self { store, sources })
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

    fn resolve_source_optional<'a>(
        &'a self,
        args: &'a Value,
    ) -> Result<Option<&'a SkillSourceDef>, ToolError> {
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

    async fn do_install(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let source = self.resolve_source(args)?;
        let (record, already_installed) = timeout(
            Duration::from_secs(INSTALL_TIMEOUT_SECS),
            self.store.install_registry_skill(&source.name, skill_name),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "install timed out after {INSTALL_TIMEOUT_SECS}s for skill `{skill_name}` from source `{}`",
                source.name
            ))
        })?
        .map_err(map_skill_err)?;
        Ok(json!({
            "action": "install",
            "source": source.name,
            "already_installed": already_installed,
            "skill": record
        }))
    }

    async fn do_uninstall(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let removed = self
            .store
            .uninstall_skill(skill_name)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "uninstall",
            "skill_name": skill_name,
            "deleted": removed.removed_managed || removed.removed_local,
            "removed_managed": removed.removed_managed,
            "removed_local": removed.removed_local
        }))
    }

    async fn do_list_installed(&self) -> Result<Value, ToolError> {
        let items = self.store.list().await.map_err(map_skill_err)?;
        Ok(json!({
            "action": "list_installed",
            "items": items
        }))
    }

    async fn do_show(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let record = self.store.get(skill_name).await.map_err(map_skill_err)?;
        let source = self.resolve_source_optional(args)?;
        Ok(json!({
            "action": "show",
            "source": source.map(|it| it.name.clone()),
            "skill": record
        }))
    }

    async fn do_search(&self, args: &Value) -> Result<Value, ToolError> {
        let query = Self::require_query(args)?;
        let source = self.resolve_source_optional(args)?;
        let limit = Self::parse_limit(args)?;
        let source_defs = match source {
            Some(item) => vec![item.clone()],
            None => self.sources.values().cloned().collect::<Vec<_>>(),
        };

        let mut matches = Vec::new();
        let mut scanned_sources = Vec::with_capacity(source_defs.len());
        for source_def in source_defs {
            scanned_sources.push(source_def.name.clone());
            let mut source_matches = self.search_source_registry(&source_def.name, query).await?;
            matches.append(&mut source_matches);
        }

        matches.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.source.cmp(&b.source))
                .then_with(|| a.skill_name.cmp(&b.skill_name))
        });
        if matches.len() > limit {
            matches.truncate(limit);
        }

        let items = matches
            .into_iter()
            .map(|item| {
                json!({
                    "source": item.source,
                    "skill_name": item.skill_name,
                    "description": item.description,
                    "local_path": item.local_path,
                    "matched_fields": item.matched_fields,
                })
            })
            .collect::<Vec<_>>();

        Ok(json!({
            "action": "search",
            "query": query,
            "source": source.map(|it| it.name.clone()),
            "scanned_sources": scanned_sources,
            "items": items
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

    async fn search_source_registry(
        &self,
        source_name: &str,
        query: &str,
    ) -> Result<Vec<RegistrySearchItem>, ToolError> {
        let mut matches = Vec::new();
        let query_terms = tokenize_query(query);
        if query_terms.is_empty() {
            return Ok(matches);
        }

        let skills_dir = self
            .store
            .skills_registry_dir()
            .join(source_name)
            .join("skills");
        let exists = fs::try_exists(&skills_dir)
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("search failed: {err}")))?;
        if !exists {
            return Ok(matches);
        }

        let mut entries = fs::read_dir(&skills_dir)
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("search failed: {err}")))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("search failed: {err}")))?
        {
            let path = entry.path();
            let ty = entry
                .file_type()
                .await
                .map_err(|err| ToolError::ExecutionFailed(format!("search failed: {err}")))?;
            if !ty.is_dir() {
                continue;
            }
            let Some(skill_name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            let skill_md_path = path.join("SKILL.md");
            let has_skill_md = fs::try_exists(&skill_md_path)
                .await
                .map_err(|err| ToolError::ExecutionFailed(format!("search failed: {err}")))?;
            if !has_skill_md {
                continue;
            }
            let content = fs::read_to_string(&skill_md_path)
                .await
                .map_err(|err| ToolError::ExecutionFailed(format!("search failed: {err}")))?;
            let description = extract_skill_description(&content);
            let Some((score, matched_fields)) =
                score_registry_match(&query_terms, skill_name, &description)
            else {
                continue;
            };
            matches.push(RegistrySearchItem {
                source: source_name.to_string(),
                skill_name: skill_name.to_string(),
                description,
                local_path: skill_md_path,
                score,
                matched_fields,
            });
        }

        Ok(matches)
    }
}

#[async_trait]
impl Tool for SkillsRegistryTool {
    fn name(&self) -> &str {
        "skills_registry"
    }

    fn description(&self) -> &str {
        "Install, uninstall, inspect, and hydrate skills from a merged view of local manual skills (~/.klaw/skills) and managed registry-indexed skills (~/.klaw/skills-registry + skills-registry-manifest.json). Supports install/uninstall/list_installed/search/show/load_all actions."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Operate on local installed skills and searchable local registry mirrors synced from configured sources.",
            "oneOf": [
                {
                    "description": "Install a skill from one configured source.",
                    "properties": {
                        "action": { "const": "install" },
                        "source": {
                            "type": "string",
                            "description": "Configured registry key (`[skills.<registry>]`)."
                        },
                        "skill_name": {
                            "type": "string",
                            "description": "Skill folder name under the selected source registry."
                        }
                    },
                    "required": ["action", "source", "skill_name"],
                    "additionalProperties": false
                },
                {
                    "description": "Uninstall by skill name. Removes managed registry index entries and/or local skill directories.",
                    "properties": {
                        "action": { "const": "uninstall" },
                        "skill_name": {
                            "type": "string",
                            "description": "Installed skill folder name."
                        }
                    },
                    "required": ["action", "skill_name"],
                    "additionalProperties": false
                },
                {
                    "description": "List all installed local skills.",
                    "properties": {
                        "action": { "const": "list_installed" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "Show details of one installed local skill.",
                    "properties": {
                        "action": { "const": "show" },
                        "skill_name": {
                            "type": "string",
                            "description": "Installed skill folder name."
                        },
                        "source": {
                            "type": "string",
                            "description": "Optional source context for response output."
                        }
                    },
                    "required": ["action", "skill_name"],
                    "additionalProperties": false
                },
                {
                    "description": "Search local registry mirrors by keyword.",
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
                },
                {
                    "description": "Load all local installed SKILL.md documents for prompt hydration.",
                    "properties": {
                        "action": { "const": "load_all" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                }
            ],
            "examples": [
                { "action": "install", "source": "vercel", "skill_name": "find-skills" },
                { "action": "search", "query": "rust cli", "limit": 10 }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemWrite
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let result =
            match action {
                "install" => self.do_install(&args).await?,
                "uninstall" => self.do_uninstall(&args).await?,
                "list_installed" => self.do_list_installed().await?,
                "search" => self.do_search(&args).await?,
                "show" => self.do_show(&args).await?,
                "load_all" => self.do_load_all().await?,
                _ => return Err(ToolError::InvalidArgs(
                    "`action` must be one of install/uninstall/list_installed/search/show/load_all"
                        .to_string(),
                )),
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

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
}

fn extract_skill_description(markdown: &str) -> String {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed.to_string();
    }
    String::new()
}

fn score_registry_match(
    query_terms: &[String],
    skill_name: &str,
    description: &str,
) -> Option<(i32, Vec<String>)> {
    if query_terms.is_empty() {
        return None;
    }

    let name_lc = skill_name.to_lowercase();
    let description_lc = description.to_lowercase();
    let full_query = query_terms.join(" ");

    let mut score = 0;
    let mut matched_name = false;
    let mut matched_description = false;
    for term in query_terms {
        let in_name = name_lc.contains(term);
        let in_description = description_lc.contains(term);
        if !in_name && !in_description {
            return None;
        }
        if in_name {
            matched_name = true;
            score += 40;
        }
        if in_description {
            matched_description = true;
            score += 15;
        }
    }

    if name_lc == full_query {
        score += 100;
    } else if name_lc.contains(&full_query) {
        score += 60;
    }
    if !description_lc.is_empty() && description_lc.contains(&full_query) {
        score += 20;
    }

    let mut fields = Vec::new();
    if matched_name {
        fields.push("name".to_string());
    }
    if matched_description {
        fields.push("description".to_string());
    }

    Some((score, fields))
}

#[cfg(test)]
mod tests {
    use super::{extract_skill_description, score_registry_match, tokenize_query};

    #[test]
    fn description_uses_first_non_heading_line() {
        let markdown = "# Title\n\n## Subtitle\nFirst useful sentence.\nSecond sentence.";
        let description = extract_skill_description(markdown);
        assert_eq!(description, "First useful sentence.");
    }

    #[test]
    fn search_requires_all_terms() {
        let terms = tokenize_query("translator chinese");
        let matched = score_registry_match(
            &terms,
            "translator",
            "Translate text between Chinese and English.",
        );
        assert!(matched.is_some());

        let not_matched = score_registry_match(
            &terms,
            "translator",
            "Translate text between Japanese and English.",
        );
        assert!(not_matched.is_none());
    }
}
