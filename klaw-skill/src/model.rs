use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    GitHubAnthropic {
        skill_name: String,
    },
    Configured {
        source_name: String,
        skill_name: String,
        download_url_template: String,
    },
}

impl SkillSource {
    pub fn github_anthropic(skill_name: &str) -> Self {
        Self::GitHubAnthropic {
            skill_name: skill_name.to_string(),
        }
    }

    pub fn skill_name(&self) -> &str {
        match self {
            Self::GitHubAnthropic { skill_name } => skill_name,
            Self::Configured { skill_name, .. } => skill_name,
        }
    }

    pub fn remote_markdown_url(&self) -> String {
        match self {
            Self::GitHubAnthropic { skill_name } => format!(
                "https://raw.githubusercontent.com/anthropics/skills/main/skills/{skill_name}/SKILL.md"
            ),
            Self::Configured {
                skill_name,
                download_url_template,
                ..
            } => download_url_template.replace("{skill_name}", skill_name),
        }
    }

    pub fn configured(source_name: &str, skill_name: &str, download_url_template: &str) -> Self {
        Self::Configured {
            source_name: source_name.to_string(),
            skill_name: skill_name.to_string(),
            download_url_template: download_url_template.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub local_path: PathBuf,
    pub updated_at_ms: i64,
    pub source_kind: SkillSourceKind,
    pub registry: Option<String>,
    pub stale: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySkillSummary {
    pub id: String,
    pub name: String,
    pub local_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRecord {
    pub name: String,
    pub source: SkillSource,
    pub local_path: PathBuf,
    pub content: String,
    pub updated_at_ms: i64,
    pub source_kind: SkillSourceKind,
    pub registry: Option<String>,
    pub stale: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceKind {
    Local,
    Registry,
}
