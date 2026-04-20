use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_skill::{
    FileSystemSkillStore, ReqwestSkillFetcher, SkillsManager, open_default_skills_manager,
};
use serde_json::{Value, json};
use tokio::time::{Duration, timeout};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

pub struct SkillsManagerTool {
    store: FileSystemSkillStore<ReqwestSkillFetcher>,
}

const INSTALL_TIMEOUT_SECS: u64 = 15;

impl SkillsManagerTool {
    pub fn open_default(_config: &AppConfig) -> Result<Self, ToolError> {
        let store = open_default_skills_manager().map_err(|err| {
            ToolError::ExecutionFailed(format!("open skills manager failed: {err}"))
        })?;
        Ok(Self { store })
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

    async fn do_install_from_registry(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let source = Self::require_source(args)?;
        let (record, already_installed) = timeout(
            Duration::from_secs(INSTALL_TIMEOUT_SECS),
            self.store.install_from_registry(source, skill_name),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "install timed out after {INSTALL_TIMEOUT_SECS}s for skill `{skill_name}` from source `{source}`"
            ))
        })?
        .map_err(map_skill_err)?;
        Ok(json!({
            "action": "install_from_registry",
            "source": source,
            "already_installed": already_installed,
            "skill": record
        }))
    }

    async fn do_uninstall(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let removed = self
            .store
            .uninstall(skill_name)
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
        let items = self.store.list_installed().await.map_err(map_skill_err)?;
        Ok(json!({
            "action": "list_installed",
            "items": items
        }))
    }

    async fn do_show_installed(&self, args: &Value) -> Result<Value, ToolError> {
        let skill_name = Self::require_skill_name(args)?;
        let record = self
            .store
            .get_installed(skill_name)
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "show_installed",
            "skill": record
        }))
    }

    async fn do_load_all(&self) -> Result<Value, ToolError> {
        let items = self
            .store
            .load_all_installed_skill_markdowns()
            .await
            .map_err(map_skill_err)?;
        Ok(json!({
            "action": "load_all",
            "items": items
        }))
    }
}

#[async_trait]
impl Tool for SkillsManagerTool {
    fn name(&self) -> &str {
        "skills_manager"
    }

    fn description(&self) -> &str {
        "Manage installed skills under ~/.klaw/skills and managed registry-indexed installs in skills-registry-manifest.json. Supports install_from_registry/uninstall/list_installed/show_installed/load_all."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Operate on installed skills. Use this tool for installation, uninstall, and loading installed SKILL.md content.",
            "oneOf": [
                {
                    "description": "Install a skill from a configured registry mirror into the managed installed set.",
                    "properties": {
                        "action": { "const": "install_from_registry" },
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
                    "description": "Uninstall an installed skill by name. Removes managed manifest entries and/or local manual files.",
                    "properties": {
                        "action": { "const": "uninstall" },
                        "skill_name": {
                            "type": "string",
                            "description": "Installed skill name."
                        }
                    },
                    "required": ["action", "skill_name"],
                    "additionalProperties": false
                },
                {
                    "description": "List installed skills from the merged local + managed view.",
                    "properties": {
                        "action": { "const": "list_installed" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "Show one installed skill from the merged local + managed view.",
                    "properties": {
                        "action": { "const": "show_installed" },
                        "skill_name": {
                            "type": "string",
                            "description": "Installed skill name."
                        }
                    },
                    "required": ["action", "skill_name"],
                    "additionalProperties": false
                },
                {
                    "description": "Load all installed SKILL.md documents for runtime prompt hydration.",
                    "properties": {
                        "action": { "const": "load_all" }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                }
            ],
            "examples": [
                { "action": "install_from_registry", "source": "vercel", "skill_name": "find-skills" },
                { "action": "list_installed" }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FilesystemWrite
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_action(&args)?;
        let result = match action {
            "install_from_registry" => self.do_install_from_registry(&args).await?,
            "uninstall" => self.do_uninstall(&args).await?,
            "list_installed" => self.do_list_installed().await?,
            "show_installed" => self.do_show_installed(&args).await?,
            "load_all" => self.do_load_all().await?,
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of install_from_registry/uninstall/list_installed/show_installed/load_all".to_string(),
                ))
            }
        };

        let rendered = serde_json::to_string_pretty(&result).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize skills_manager output failed: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
            media: Vec::new(),
            signals: Vec::new(),
        })
    }
}

fn map_skill_err(err: klaw_skill::SkillError) -> ToolError {
    ToolError::ExecutionFailed(err.to_string())
}
