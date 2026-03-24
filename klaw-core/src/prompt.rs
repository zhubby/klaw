use std::{
    io,
    path::{Path, PathBuf},
};

use klaw_util::{default_data_dir, workspace_dir};
use thiserror::Error;
use tokio::fs;

const SKILLS_LAZY_LOAD_INSTRUCTIONS: &str = "When a task may require a skill, consult the available skills list first.\nBefore using a skill, read the SKILL.md file at the listed path.\nOnly load skill files when needed.";
const WORKSPACE_PROMPT_DOC_FILES: [&str; 7] = [
    "AGENTS.md",
    "BOOTSTRAP.md",
    "HEARTBEAT.md",
    "IDENTITY.md",
    "SOUL.md",
    "TOOLS.md",
    "USER.md",
];
const RUNTIME_PROMPT_RULES: &str = "Prefer lazy-loading context from files and skills instead of relying on embedded long prompt bodies. Read only what is needed for the current user request.";
const RUNTIME_PROMPT_EXTRA_INSTRUCTIONS: &str = "When local workspace docs are relevant, read them from disk on demand before acting. Do not assume their content without reading the files.\nWhen a task requires remembering or recalling prior context, use the memory tool. Do not rely on ad-hoc markdown memory files.\nFiles under archives/ are read-only source material. Never edit, move, or delete them in place. If you need to transform or modify an archived file, use the archive tool to copy it into workspace first, then operate on the copied file.";

const PROMPT_TEMPLATE_FILES: [(&str, &str); 7] = [
    ("AGENTS.md", include_str!("../templates/prompt/AGENTS.md")),
    (
        "BOOTSTRAP.md",
        include_str!("../templates/prompt/BOOTSTRAP.md"),
    ),
    (
        "HEARTBEAT.md",
        include_str!("../templates/prompt/HEARTBEAT.md"),
    ),
    (
        "IDENTITY.md",
        include_str!("../templates/prompt/IDENTITY.md"),
    ),
    ("SOUL.md", include_str!("../templates/prompt/SOUL.md")),
    ("TOOLS.md", include_str!("../templates/prompt/TOOLS.md")),
    ("USER.md", include_str!("../templates/prompt/USER.md")),
];
const AUTO_CREATE_PROMPT_TEMPLATE_FILES: [(&str, &str); 6] = [
    ("AGENTS.md", include_str!("../templates/prompt/AGENTS.md")),
    (
        "HEARTBEAT.md",
        include_str!("../templates/prompt/HEARTBEAT.md"),
    ),
    (
        "IDENTITY.md",
        include_str!("../templates/prompt/IDENTITY.md"),
    ),
    ("SOUL.md", include_str!("../templates/prompt/SOUL.md")),
    ("TOOLS.md", include_str!("../templates/prompt/TOOLS.md")),
    ("USER.md", include_str!("../templates/prompt/USER.md")),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTemplateWriteReport {
    pub created_files: Vec<String>,
    pub skipped_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPromptEntry {
    pub name: String,
    pub path: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimePromptInput {
    pub runtime_metadata: Option<String>,
    pub rules: Option<String>,
    pub local_docs: Option<String>,
    pub additional_instructions: Option<String>,
    pub skills: Vec<SkillPromptEntry>,
}

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("home directory is unavailable")]
    HomeDirUnavailable,
    #[error("failed to create directory `{path}`: {source}")]
    CreateDir { path: String, source: io::Error },
    #[error("failed to write template file `{path}`: {source}")]
    WriteTemplateFile { path: String, source: io::Error },
    #[error("failed to check path `{path}` existence: {source}")]
    CheckPathExists { path: String, source: io::Error },
}

pub async fn ensure_workspace_prompt_templates() -> Result<PromptTemplateWriteReport, PromptError> {
    let data_dir = resolve_default_data_dir()?;
    ensure_workspace_prompt_templates_in_dir(data_dir).await
}

pub async fn ensure_workspace_prompt_templates_in_dir(
    data_dir: PathBuf,
) -> Result<PromptTemplateWriteReport, PromptError> {
    fs::create_dir_all(&data_dir)
        .await
        .map_err(|source| PromptError::CreateDir {
            path: data_dir.display().to_string(),
            source,
        })?;

    let workspace_dir = workspace_dir(&data_dir);
    let workspace_previously_existed = path_exists(&workspace_dir).await?;
    fs::create_dir_all(&workspace_dir)
        .await
        .map_err(|source| PromptError::CreateDir {
            path: workspace_dir.display().to_string(),
            source,
        })?;

    let mut created_files = Vec::new();
    let mut skipped_files = Vec::new();

    if !workspace_previously_existed {
        let bootstrap_path = workspace_dir.join("BOOTSTRAP.md");
        let bootstrap_content = get_default_template_content("BOOTSTRAP.md")
            .expect("BOOTSTRAP.md template content should exist");
        fs::write(&bootstrap_path, bootstrap_content)
            .await
            .map_err(|source| PromptError::WriteTemplateFile {
                path: bootstrap_path.display().to_string(),
                source,
            })?;
        created_files.push("BOOTSTRAP.md".to_string());
    }

    for (file_name, content) in AUTO_CREATE_PROMPT_TEMPLATE_FILES {
        let target = workspace_dir.join(file_name);
        if path_exists(&target).await? {
            skipped_files.push(file_name.to_string());
            continue;
        }

        fs::write(&target, content)
            .await
            .map_err(|source| PromptError::WriteTemplateFile {
                path: target.display().to_string(),
                source,
            })?;
        created_files.push(file_name.to_string());
    }

    Ok(PromptTemplateWriteReport {
        created_files,
        skipped_files,
    })
}

pub fn format_skills_for_prompt(skills: &[SkillPromptEntry]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let mut out = String::from("## Available Skills\n\n");
    for (idx, skill) in skills.iter().enumerate() {
        out.push_str(&(idx + 1).to_string());
        out.push_str(". ");
        out.push_str(skill.name.trim());
        out.push('\n');
        out.push_str("   path: ");
        out.push_str(skill.path.trim());
        out.push('\n');
        out.push_str("   source: ");
        out.push_str(skill.source.trim());
        out.push('\n');
        out.push_str("   description: ");
        out.push_str(skill.description.trim());
        out.push_str("\n\n");
    }

    Some(out.trim_end().to_string())
}

pub fn skills_lazy_load_instructions() -> &'static str {
    SKILLS_LAZY_LOAD_INSTRUCTIONS
}

pub fn get_default_template_content(file_name: &str) -> Option<&'static str> {
    PROMPT_TEMPLATE_FILES
        .iter()
        .find(|(name, _)| *name == file_name)
        .map(|(_, content)| *content)
}

pub fn format_workspace_docs_for_prompt() -> String {
    let base = std::env::var("HOME")
        .map(|home| format!("{home}/.klaw/workspace"))
        .unwrap_or_else(|_| "~/.klaw/workspace".to_string());
    let docs = WORKSPACE_PROMPT_DOC_FILES
        .iter()
        .map(|name| format!("- {base}/{name}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Read these workspace docs on demand when relevant:\n{docs}\n\
\n\
Recommended usage:\n\
- Start with AGENTS.md and USER.md for baseline behavior and user preferences.\n\
- Read TOOLS.md before tool-heavy tasks or environment-specific operations.\n\
- Read HEARTBEAT.md only for heartbeat/autonomous polling turns.\n\
- Read BOOTSTRAP.md only during first-run initialization or cold-start setup.\n\
- Use the memory tool for durable memory; do not use markdown files as memory storage."
    )
}

pub fn compose_runtime_prompt(input: RuntimePromptInput) -> Option<String> {
    let mut sections = Vec::new();

    push_section(&mut sections, "Runtime Metadata", input.runtime_metadata);
    push_section(&mut sections, "Rules", input.rules);

    if let Some(skills_block) = format_skills_for_prompt(&input.skills) {
        sections.push(skills_block);
    }

    push_section(
        &mut sections,
        "Instructions",
        Some(skills_lazy_load_instructions().to_string()),
    );
    push_section(&mut sections, "Local Docs", input.local_docs);
    push_section(
        &mut sections,
        "Additional Instructions",
        input.additional_instructions,
    );

    if sections.is_empty() {
        return None;
    }

    Some(sections.join("\n\n--------------------------------\n\n"))
}

pub fn build_runtime_system_prompt(skills: Vec<SkillPromptEntry>) -> Option<String> {
    compose_runtime_prompt(RuntimePromptInput {
        runtime_metadata: None,
        rules: Some(RUNTIME_PROMPT_RULES.to_string()),
        local_docs: Some(format_workspace_docs_for_prompt()),
        additional_instructions: Some(RUNTIME_PROMPT_EXTRA_INSTRUCTIONS.to_string()),
        skills,
    })
}

fn push_section(sections: &mut Vec<String>, title: &str, content: Option<String>) {
    let Some(content) = content.map(|value| value.trim().to_string()) else {
        return;
    };
    if content.is_empty() {
        return;
    }
    sections.push(format!("## {title}\n\n{content}"));
}

async fn path_exists(path: &Path) -> Result<bool, PromptError> {
    fs::try_exists(path)
        .await
        .map_err(|source| PromptError::CheckPathExists {
            path: path.display().to_string(),
            source,
        })
}

fn resolve_default_data_dir() -> Result<PathBuf, PromptError> {
    default_data_dir().ok_or(PromptError::HomeDirUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs;
    use uuid::Uuid;

    #[tokio::test(flavor = "current_thread")]
    async fn ensure_templates_creates_workspace_and_writes_all_when_missing() {
        let data_dir = std::env::temp_dir().join(format!("klaw-prompt-test-{}", Uuid::new_v4()));

        let report = ensure_workspace_prompt_templates_in_dir(data_dir.clone())
            .await
            .expect("should initialize workspace templates");

        assert_eq!(report.created_files.len(), 7);
        assert!(report.skipped_files.is_empty());

        for (file_name, _) in PROMPT_TEMPLATE_FILES {
            let content = fs::read_to_string(workspace_dir(&data_dir).join(file_name))
                .await
                .expect("template file should exist");
            assert!(!content.trim().is_empty());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ensure_templates_does_not_overwrite_existing_files() {
        let data_dir = std::env::temp_dir().join(format!("klaw-prompt-test-{}", Uuid::new_v4()));
        let workspace = workspace_dir(&data_dir);
        fs::create_dir_all(&workspace)
            .await
            .expect("workspace dir should be created");
        let agents_path = workspace.join("AGENTS.md");
        fs::write(&agents_path, "custom agents")
            .await
            .expect("custom agents should be written");

        let report = ensure_workspace_prompt_templates_in_dir(data_dir.clone())
            .await
            .expect("should initialize missing templates only");

        assert!(report.skipped_files.contains(&"AGENTS.md".to_string()));
        let agents = fs::read_to_string(agents_path)
            .await
            .expect("agents should still exist");
        assert_eq!(agents, "custom agents");
        assert!(
            !workspace.join("BOOTSTRAP.md").exists(),
            "bootstrap should not be auto-created during backfill"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ensure_templates_does_not_recreate_bootstrap_after_first_init() {
        let data_dir = std::env::temp_dir().join(format!("klaw-prompt-test-{}", Uuid::new_v4()));
        let workspace = workspace_dir(&data_dir);

        let first_report = ensure_workspace_prompt_templates_in_dir(data_dir.clone())
            .await
            .expect("first init should succeed");
        assert!(first_report.created_files.contains(&"BOOTSTRAP.md".to_string()));
        assert!(workspace.join("BOOTSTRAP.md").exists());

        fs::remove_file(workspace.join("BOOTSTRAP.md"))
            .await
            .expect("bootstrap should be removable");

        let second_report = ensure_workspace_prompt_templates_in_dir(data_dir.clone())
            .await
            .expect("second init should succeed");
        assert!(
            !second_report.created_files.contains(&"BOOTSTRAP.md".to_string()),
            "bootstrap should not be recreated after first init"
        );
        assert!(
            !workspace.join("BOOTSTRAP.md").exists(),
            "bootstrap should stay deleted after later backfill"
        );
    }

    #[test]
    fn format_skills_returns_none_for_empty_list() {
        assert_eq!(format_skills_for_prompt(&[]), None);
    }

    #[test]
    fn format_skills_shortlist_has_expected_fields() {
        let out = format_skills_for_prompt(&[SkillPromptEntry {
            name: "github".to_string(),
            path: "workspace/skills/github/SKILL.md".to_string(),
            description: "interact with GitHub repositories".to_string(),
            source: "workspace".to_string(),
        }])
        .expect("skills block expected");

        assert!(out.contains("## Available Skills"));
        assert!(out.contains("1. github"));
        assert!(out.contains("path: workspace/skills/github/SKILL.md"));
        assert!(out.contains("source: workspace"));
        assert!(out.contains("description: interact with GitHub repositories"));
        assert!(!out.contains("Use this skill when:"));
    }

    #[test]
    fn compose_runtime_prompt_skips_empty_sections() {
        let prompt = compose_runtime_prompt(RuntimePromptInput {
            runtime_metadata: Some("  ".to_string()),
            rules: Some("rule-a".to_string()),
            local_docs: None,
            additional_instructions: Some(String::new()),
            skills: vec![],
        })
        .expect("composed prompt expected");

        assert!(prompt.contains("## Rules\n\nrule-a"));
        assert!(prompt.contains("## Instructions"));
        assert!(!prompt.contains("## Runtime Metadata"));
        assert!(!prompt.contains("## Available Skills"));
    }

    #[test]
    fn workspace_docs_prompt_contains_routing_and_memory_tool_rule() {
        let docs_prompt = format_workspace_docs_for_prompt();

        assert!(docs_prompt.contains("Read these workspace docs on demand when relevant:"));
        assert!(docs_prompt.contains("Recommended usage:"));
        assert!(docs_prompt.contains("AGENTS.md"));
        assert!(docs_prompt.contains("USER.md"));
        assert!(docs_prompt.contains("TOOLS.md"));
        assert!(docs_prompt.contains("Use the memory tool for durable memory"));
    }

    #[test]
    fn build_runtime_system_prompt_includes_runtime_sections() {
        let prompt = build_runtime_system_prompt(vec![SkillPromptEntry {
            name: "github".to_string(),
            path: "workspace/skills/github/SKILL.md".to_string(),
            description: "interact with GitHub repositories".to_string(),
            source: "workspace".to_string(),
        }])
        .expect("runtime prompt expected");

        assert!(prompt.contains("## Rules"));
        assert!(prompt.contains("## Available Skills"));
        assert!(prompt.contains("## Instructions"));
        assert!(prompt.contains("## Local Docs"));
        assert!(prompt.contains("## Additional Instructions"));
    }
}
