use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_skill::{
    FileSystemSkillStore, ReqwestSkillFetcher, SkillsRegistry, open_default_skill_registry,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

#[derive(Debug, Clone)]
struct SkillSourceDef {
    name: String,
}

pub struct SkillsRegistryTool {
    store: FileSystemSkillStore<ReqwestSkillFetcher>,
    sources: BTreeMap<String, SkillSourceDef>,
}

impl SkillsRegistryTool {
    pub fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let store = open_default_skill_registry().map_err(|err| {
            ToolError::ExecutionFailed(format!("open skills registry failed: {err}"))
        })?;
        let mut sources = BTreeMap::new();
        for source_name in config.skills.registries.keys() {
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

    fn resolve_source<'a>(&'a self, args: &'a Value) -> Result<&'a SkillSourceDef, ToolError> {
        let source_name = args
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs("missing `source`".to_string()))?;
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

    async fn do_list(&self, args: &Value) -> Result<Value, ToolError> {
        let source = self.resolve_source(args)?;
        let items = self
            .store
            .list_source_skills(&source.name)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "list",
            "source": source.name,
            "items": items
        }))
    }

    async fn do_show(&self, args: &Value) -> Result<Value, ToolError> {
        let source = self.resolve_source(args)?;
        let skill_name = Self::require_skill_name(args)?;
        let record = self
            .store
            .get_source_skill(&source.name, skill_name)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "show",
            "source": source.name,
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
            let mut source_matches = self
                .store
                .search_source_skills(&source_def.name, query)
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
            "source": source.map(|it| it.name.clone()),
            "scanned_sources": scanned_sources,
            "items": matches
        }))
    }
}

#[async_trait]
impl Tool for SkillsRegistryTool {
    fn name(&self) -> &str {
        "skills_registry"
    }

    fn description(&self) -> &str {
        "Browse and search local registry mirrors under ~/.klaw/skills-registry. This tool is read-only and supports list/show/search actions for registry skills."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Operate on configured registry mirrors only. Use this tool to browse or inspect installable registry skills before calling skills_manager.install_from_registry.",
            "oneOf": [
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
                { "action": "list", "source": "vercel" },
                { "action": "show", "source": "vercel", "skill_name": "find-skills" },
                { "action": "search", "query": "rust cli", "limit": 10 }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemRead
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let result = match action {
            "list" => self.do_list(&args).await?,
            "show" => self.do_show(&args).await?,
            "search" => self.do_search(&args).await?,
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of list/show/search".to_string(),
                ));
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
