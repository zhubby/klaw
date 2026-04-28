use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use handlebars::Handlebars;
use klaw_util::{default_data_dir, workspace_dir};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::fs;
use tracing::warn;

const SKILLS_LAZY_LOAD_INSTRUCTIONS: &str = r#"When a task may require a skill, consult the available skills list first.
Before using a skill, read the SKILL.md file at the listed path.
Only load skill files when needed."#;
const INLINED_WORKSPACE_PROMPT_DOC_FILES: [&str; 4] =
    ["AGENTS.md", "SOUL.md", "IDENTITY.md", "TOOLS.md"];
const ON_DEMAND_WORKSPACE_PROMPT_DOC_FILES: [&str; 2] = ["USER.md", "BOOTSTRAP.md"];
const RUNTIME_PROMPT_RULES: &str = r#"Treat the inlined workspace docs below as baseline instructions. Lazy-load only the remaining workspace docs and skills when they are relevant to the current user request."#;
const RUNTIME_PROMPT_EXTRA_INSTRUCTIONS: &str = r#"General:
- Do not re-read `AGENTS.md`, `SOUL.md`, `IDENTITY.md`, or `TOOLS.md` just to recover instructions already inlined into this system prompt.
- When additional workspace context is needed, read only the remaining on-demand docs from disk before acting.

Truthfulness:
- Never claim to have read, checked, searched, run, verified, confirmed, sent, or changed something unless that action actually happened through a tool result in this turn or the evidence was explicitly provided by the user in the conversation.
- Do not present intended actions, assumptions, remembered context, or likely outcomes as completed work.
- If something has not been verified yet, say that it is unverified and describe the next tool you would use to verify it.

Memory:
- When a task requires remembering or recalling prior context, use the memory tool. Do not rely on ad-hoc markdown memory files.

Archives:
- Files under archives/ are read-only source material. Never edit, move, or delete them in place.
- If you need to transform or modify an archived file, use the archive tool to copy it into workspace first, then operate on the copied file.

Channel Attachments:
- When the user wants a file or image sent back into chat, use the `channel_attachment` tool instead of only describing the file in plain text.
- Use `archive_id` when you already have a valid archived file id.
- Use `path` only for an absolute local file path that is inside the workspace or a configured channel allowlist.
- Prefer `kind=image` for screenshots or images that should render inline, and `kind=file` for generic documents or downloads.
- Never pass a list index like `1` as `archive_id`, and never use a relative path.

Ask Question:
- When you can enumerate clear options for a user decision or preference, prefer the `ask_question` tool over open-ended text — it presents a single-select card so the user picks directly instead of typing.
- Do not use `ask_question` for open-ended questions needing free-form text. If you recommend an option, place it first and append `(Recommended)` to its label.
"#;

const PROMPT_TEMPLATE_FILES: [(&str, &str); 6] = [
    ("AGENTS.md", include_str!("../templates/prompt/AGENTS.md")),
    (
        "BOOTSTRAP.md",
        include_str!("../templates/prompt/BOOTSTRAP.md"),
    ),
    (
        "IDENTITY.md",
        include_str!("../templates/prompt/IDENTITY.md"),
    ),
    ("SOUL.md", include_str!("../templates/prompt/SOUL.md")),
    ("TOOLS.md", include_str!("../templates/prompt/TOOLS.md")),
    ("USER.md", include_str!("../templates/prompt/USER.md")),
];
const AUTO_CREATE_PROMPT_TEMPLATE_FILES: [(&str, &str); 5] = [
    ("AGENTS.md", include_str!("../templates/prompt/AGENTS.md")),
    (
        "IDENTITY.md",
        include_str!("../templates/prompt/IDENTITY.md"),
    ),
    ("SOUL.md", include_str!("../templates/prompt/SOUL.md")),
    ("TOOLS.md", include_str!("../templates/prompt/TOOLS.md")),
    ("USER.md", include_str!("../templates/prompt/USER.md")),
];

// ---------------------------------------------------------------------------
// Prompt Extension trait & built-in extensions
// ---------------------------------------------------------------------------

/// A prompt extension that conditionally includes template sections based on
/// runtime environment conditions.
///
/// Template files use Handlebars `{{#if}}` blocks to denote extension sections:
///
/// ```handlebars
/// {{#if rtk}}
/// ## Extension Agent: rtk Command Proxy
/// ...content...
/// {{/if}}
/// ```
///
/// When an extension's `is_active()` returns `true`, the Handlebars context
/// includes its name as `true` and the `{{#if name}}` block is rendered.
/// When `is_active()` returns `false`, the entire block is omitted.
///
/// The condition check (`is_active()`) is implemented in code — e.g., probing
/// the system for a command, checking a config flag, or testing network
/// reachability. It is **not** left to the LLM to decide.
pub trait PromptExtension: Send + Sync {
    /// Unique identifier matching the key used in `{{#if name}}` blocks.
    fn name(&self) -> &str;

    /// Evaluate whether this extension should be active.
    /// Implemented in code — e.g., probing the system for a command, checking a
    /// config flag, or testing network reachability.
    fn is_active(&self) -> bool;

    /// Return the prompt section content when `is_active()` is true.
    /// Used as a standalone fallback for dynamic injection outside of
    /// Handlebars template files (e.g., when a template lacks the `{{#if}}`
    /// block for this extension).
    fn prompt_section(&self) -> String;
}

/// Built-in extension: checks whether the `rtk` command-proxy exists on the
/// system. When active, shell-tool commands should be prefixed with `rtk` for
/// token-optimized command output; the shell tool still applies its own
/// approval and blocked-command policy to the unwrapped command.
pub struct RtkPromptExtension;

impl PromptExtension for RtkPromptExtension {
    fn name(&self) -> &str {
        "rtk"
    }

    fn is_active(&self) -> bool {
        is_command_available("rtk")
    }

    fn prompt_section(&self) -> String {
        RTK_PROMPT_SECTION.to_string()
    }
}

const RTK_PROMPT_SECTION: &str = r#"## Extension Agent: rtk Command Proxy

The `rtk` command is available on this system and acts as a proxy/wrapper for shell operations. It provides token-optimized command output while Klaw's shell tool remains responsible for approval and blocked-command enforcement.

**Rule:** When using the shell tool to execute commands, **always prefix the command with `rtk`**. Examples:
- Instead of `git status` → run `rtk git status`
- Instead of `npm install` → run `rtk npm install`
- Instead of `cargo build` → run `rtk cargo build`
- Instead of `ls -la` → run `rtk ls -la`

This applies to **every** shell command — no exceptions. Klaw's shell tool unwraps `rtk` commands before applying blocked-command and approval policy, so `rtk` is an execution proxy and output optimizer, not an approval bypass."#;

/// Return the default set of built-in prompt extensions.
/// Callers may append additional extensions before passing the list into
/// `build_runtime_system_prompt_with_extensions`.
pub fn default_prompt_extensions() -> Vec<Arc<dyn PromptExtension>> {
    vec![Arc::new(RtkPromptExtension)]
}

/// Check whether a command is available on the system by searching PATH.
/// Uses `which` on Unix-like systems and `where` on Windows.
fn is_command_available(name: &str) -> bool {
    let checker = if cfg!(target_family = "unix") {
        "which"
    } else {
        "where"
    };
    std::process::Command::new(checker)
        .arg(name)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Render a Handlebars template string, injecting extension activity flags
/// into the template context.
///
/// Each extension contributes a boolean key (`name` → `is_active()`) to the
/// context. Template sections wrapped in `{{#if name}}` / `{{/if}}` are
/// included when the extension is active and omitted when it is not.
///
/// Built-in extension content is injected by code in
/// `format_inlined_workspace_context_for_prompt_in_dir`, not embedded in
/// template files, so that AGENTS.md stays pure markdown without visible
/// Handlebars syntax. This function is still applied to on-disk content as
/// a safety net for user-edited files that may add `{{#if}}` blocks.
///
/// Falls back to the raw content on rendering errors (e.g., malformed
/// Handlebars syntax in a user-edited file), ensuring the prompt is always
/// available even if template processing fails.
fn render_template_with_extensions(
    content: &str,
    extensions: &[Arc<dyn PromptExtension>],
) -> String {
    let mut hb = Handlebars::new();
    // Non-strict mode: unknown `{{variables}}` render as empty string instead
    // of raising an error. This is safer for user-edited markdown files that
    // may inadvertently contain `{{…}}` patterns unrelated to extensions.
    hb.set_strict_mode(false);

    let mut ctx = Map::new();
    for ext in extensions {
        ctx.insert(ext.name().to_string(), Value::Bool(ext.is_active()));
    }

    match hb.render_template(content, &Value::Object(ctx)) {
        Ok(rendered) => rendered,
        Err(e) => {
            warn!("Handlebars template rendering failed, using raw content: {e}");
            content.to_string()
        }
    }
}

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
    pub workspace_context: Option<String>,
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
    let workspace_previously_initialized =
        workspace_previously_existed && any_prompt_template_exists(&workspace_dir).await?;
    fs::create_dir_all(&workspace_dir)
        .await
        .map_err(|source| PromptError::CreateDir {
            path: workspace_dir.display().to_string(),
            source,
        })?;

    let mut created_files = Vec::new();
    let mut skipped_files = Vec::new();

    if !workspace_previously_initialized {
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
    resolve_default_data_dir()
        .ok()
        .map(|data_dir| format_workspace_docs_for_prompt_in_dir(&data_dir))
        .unwrap_or_else(|| format_workspace_docs_for_prompt_in_dir(Path::new("~/.klaw")))
}

fn format_workspace_docs_for_prompt_in_dir(data_dir: &Path) -> String {
    let base = workspace_dir(data_dir).display().to_string();
    let docs = ON_DEMAND_WORKSPACE_PROMPT_DOC_FILES
        .iter()
        .map(|name| format!("- {base}/{name}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Read these workspace docs only when relevant:\n{docs}\n\
\n\
Recommended usage:\n\
- Read `BOOTSTRAP.md` only during first-run initialization or cold-start setup.\n\
- Use the memory tool for durable memory; do not use markdown files as memory storage."
    )
}

pub fn compose_runtime_prompt(input: RuntimePromptInput) -> Option<String> {
    compose_runtime_prompt_with_descriptor(input, format_workspace_descriptor_for_prompt())
}

fn compose_runtime_prompt_with_descriptor(
    input: RuntimePromptInput,
    workspace_descriptor: Option<String>,
) -> Option<String> {
    let mut sections = Vec::new();

    if let Some(workspace_descriptor) = workspace_descriptor {
        sections.push(workspace_descriptor);
    }

    if let Some(workspace_context) = input.workspace_context {
        let workspace_context = workspace_context.trim();
        if !workspace_context.is_empty() {
            sections.push(workspace_context.to_string());
        }
    }

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
    build_runtime_system_prompt_with_extensions(skills, default_prompt_extensions())
}

/// Build the full runtime system prompt with explicit prompt extensions.
/// Active extensions inject their `prompt_section()` content after the
/// inlined workspace context. Inactive extensions are omitted entirely.
/// Template files may optionally use Handlebars `{{#if name}}` / `{{/if}}`
/// blocks for advanced conditional rendering, but built-in extension
/// sections are injected by code — not embedded in template files.
pub fn build_runtime_system_prompt_with_extensions(
    skills: Vec<SkillPromptEntry>,
    extensions: Vec<Arc<dyn PromptExtension>>,
) -> Option<String> {
    let data_dir = resolve_default_data_dir().ok()?;
    build_runtime_system_prompt_in_dir(&data_dir, skills, &extensions)
}

fn build_runtime_system_prompt_in_dir(
    data_dir: &Path,
    skills: Vec<SkillPromptEntry>,
    extensions: &[Arc<dyn PromptExtension>],
) -> Option<String> {
    compose_runtime_prompt_with_descriptor(
        RuntimePromptInput {
            workspace_context: format_inlined_workspace_context_for_prompt_in_dir(
                &workspace_dir(data_dir),
                extensions,
            ),
            runtime_metadata: None,
            rules: Some(RUNTIME_PROMPT_RULES.to_string()),
            local_docs: Some(format_workspace_docs_for_prompt_in_dir(data_dir)),
            additional_instructions: Some(RUNTIME_PROMPT_EXTRA_INSTRUCTIONS.to_string()),
            skills,
        },
        Some(format_workspace_descriptor_for_prompt_in_dir(data_dir)),
    )
}

fn format_workspace_descriptor_for_prompt() -> Option<String> {
    let data_dir = resolve_default_data_dir().ok()?;
    Some(format_workspace_descriptor_for_prompt_in_dir(&data_dir))
}

fn format_workspace_descriptor_for_prompt_in_dir(data_dir: &Path) -> String {
    let workspace = workspace_dir(data_dir);
    let workspace_path = workspace.display().to_string();

    format!(
        "## Workspace\n\nPath: `{workspace_path}`\n\nThis is the agent workspace home. It stores persistent workspace docs, identity, behavior rules, and local environment notes that define how the agent should operate across sessions."
    )
}

fn format_inlined_workspace_context_for_prompt_in_dir(
    workspace_dir: &Path,
    extensions: &[Arc<dyn PromptExtension>],
) -> Option<String> {
    let mut sections = Vec::new();

    for file_name in INLINED_WORKSPACE_PROMPT_DOC_FILES {
        let path = workspace_dir.join(file_name);
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let content = render_template_with_extensions(content.trim(), extensions);
        if content.is_empty() {
            continue;
        }
        sections.push(content);
    }

    // Inject active extension prompt sections after the inlined file content.
    // Extension content is defined in code (via `prompt_section()`), not
    // embedded in template files, so that AGENTS.md stays pure markdown
    // without visible Handlebars syntax in GUI previews.
    for ext in extensions {
        if ext.is_active() {
            let section = ext.prompt_section().trim().to_string();
            if !section.is_empty() {
                sections.push(section);
            }
        }
    }

    if sections.is_empty() {
        return None;
    }

    Some(sections.join("\n\n"))
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

async fn any_prompt_template_exists(workspace_dir: &Path) -> Result<bool, PromptError> {
    for (file_name, _) in PROMPT_TEMPLATE_FILES {
        if path_exists(&workspace_dir.join(file_name)).await? {
            return Ok(true);
        }
    }
    Ok(false)
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

        assert_eq!(report.created_files.len(), 6);
        assert!(report.skipped_files.is_empty());

        for (file_name, _) in PROMPT_TEMPLATE_FILES {
            let content = fs::read_to_string(workspace_dir(&data_dir).join(file_name))
                .await
                .expect("template file should exist");
            assert!(!content.trim().is_empty());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ensure_templates_writes_bootstrap_when_workspace_dir_is_empty() {
        let data_dir = std::env::temp_dir().join(format!("klaw-prompt-test-{}", Uuid::new_v4()));
        let workspace = workspace_dir(&data_dir);
        fs::create_dir_all(&workspace)
            .await
            .expect("empty workspace dir should be created");

        let report = ensure_workspace_prompt_templates_in_dir(data_dir.clone())
            .await
            .expect("should initialize empty workspace templates");

        assert!(
            report.created_files.contains(&"BOOTSTRAP.md".to_string()),
            "empty workspace dir should still be treated as first init"
        );
        assert!(workspace.join("BOOTSTRAP.md").exists());
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
        assert!(
            first_report
                .created_files
                .contains(&"BOOTSTRAP.md".to_string())
        );
        assert!(workspace.join("BOOTSTRAP.md").exists());

        fs::remove_file(workspace.join("BOOTSTRAP.md"))
            .await
            .expect("bootstrap should be removable");

        let second_report = ensure_workspace_prompt_templates_in_dir(data_dir.clone())
            .await
            .expect("second init should succeed");
        assert!(
            !second_report
                .created_files
                .contains(&"BOOTSTRAP.md".to_string()),
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
            workspace_context: None,
            runtime_metadata: Some("  ".to_string()),
            rules: Some("rule-a".to_string()),
            local_docs: None,
            additional_instructions: Some(String::new()),
            skills: vec![],
        })
        .expect("composed prompt expected");

        assert!(prompt.contains("## Workspace"));
        assert!(prompt.contains("## Rules\n\nrule-a"));
        assert!(prompt.contains("## Instructions"));
        assert!(!prompt.contains("## Runtime Metadata\n\n"));
        assert!(!prompt.contains("## Available Skills"));
    }

    #[test]
    fn workspace_docs_prompt_contains_routing_and_memory_tool_rule() {
        let docs_prompt = format_workspace_docs_for_prompt();

        assert!(docs_prompt.contains("Read these workspace docs only when relevant:"));
        assert!(docs_prompt.contains("Recommended usage:"));
        assert!(docs_prompt.contains("USER.md"));
        assert!(docs_prompt.contains("BOOTSTRAP.md"));
        assert!(!docs_prompt.contains("Read `USER.md` when user-specific preferences"));
        assert!(!docs_prompt.contains("HEARTBEAT.md"));
        assert!(!docs_prompt.contains("AGENTS.md"));
        assert!(!docs_prompt.contains("SOUL.md"));
        assert!(!docs_prompt.contains("IDENTITY.md"));
        assert!(!docs_prompt.contains("TOOLS.md"));
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

        assert!(prompt.contains("## Workspace"));
        assert!(prompt.contains("# AGENTS.md"));
        assert!(prompt.contains("## Rules"));
        assert!(prompt.contains("## Available Skills"));
        assert!(prompt.contains("## Instructions"));
        assert!(prompt.contains("## Local Docs"));
        assert!(prompt.contains("## Additional Instructions"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn build_runtime_system_prompt_keeps_expected_full_structure() {
        let data_dir = std::env::temp_dir().join(format!("klaw-prompt-full-{}", Uuid::new_v4()));
        let klaw_root = data_dir.join(".klaw");
        let workspace = workspace_dir(&klaw_root);
        fs::create_dir_all(&workspace)
            .await
            .expect("workspace dir should be created");
        fs::write(workspace.join("AGENTS.md"), "# AGENTS.md\n\nagents body")
            .await
            .expect("agents should be written");
        fs::write(workspace.join("SOUL.md"), "# SOUL.md\n\nsoul body")
            .await
            .expect("soul should be written");
        fs::write(
            workspace.join("IDENTITY.md"),
            "# IDENTITY.md\n\nidentity body",
        )
        .await
        .expect("identity should be written");
        fs::write(workspace.join("TOOLS.md"), "# TOOLS.md\n\ntools body")
            .await
            .expect("tools should be written");

        let prompt = build_runtime_system_prompt_in_dir(
            &klaw_root,
            vec![SkillPromptEntry {
                name: "github".to_string(),
                path: "workspace/skills/github/SKILL.md".to_string(),
                description: "interact with GitHub repositories".to_string(),
                source: "workspace".to_string(),
            }],
            &default_prompt_extensions(),
        )
        .expect("runtime prompt expected");

        let workspace_pos = prompt.find("## Workspace").expect("workspace section");
        let agents_pos = prompt.find("# AGENTS.md").expect("agents section");
        let soul_pos = prompt.find("# SOUL.md").expect("soul section");
        let identity_pos = prompt.find("# IDENTITY.md").expect("identity section");
        let tools_pos = prompt.find("# TOOLS.md").expect("tools section");
        let rules_pos = prompt.find("## Rules").expect("rules section");
        let skills_pos = prompt.find("## Available Skills").expect("skills section");
        let instructions_pos = prompt
            .find("## Instructions")
            .expect("instructions section");
        let docs_pos = prompt.find("## Local Docs").expect("local docs section");
        let extra_pos = prompt
            .find("## Additional Instructions")
            .expect("extra instructions section");

        assert!(workspace_pos < agents_pos);
        assert!(agents_pos < soul_pos);
        assert!(soul_pos < identity_pos);
        assert!(identity_pos < tools_pos);
        assert!(tools_pos < rules_pos);
        assert!(rules_pos < skills_pos);
        assert!(skills_pos < instructions_pos);
        assert!(instructions_pos < docs_pos);
        assert!(docs_pos < extra_pos);

        assert!(prompt.contains("- "));
        assert!(prompt.contains("USER.md"));
        assert!(prompt.contains("BOOTSTRAP.md"));
        assert!(!prompt.contains("HEARTBEAT.md"));
        assert!(!prompt.contains("Read `TOOLS.md`"));
        assert!(!prompt.contains("Read `SOUL.md`"));
        assert!(
            prompt.contains("Do not re-read `AGENTS.md`, `SOUL.md`, `IDENTITY.md`, or `TOOLS.md`")
        );
        assert!(prompt.contains(
            "Never claim to have read, checked, searched, run, verified, confirmed, sent, or changed something unless that action actually happened through a tool result in this turn"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inlined_workspace_context_reads_docs_in_expected_order() {
        let workspace =
            std::env::temp_dir().join(format!("klaw-prompt-workspace-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace)
            .await
            .expect("workspace dir should be created");
        fs::write(workspace.join("AGENTS.md"), "# AGENTS.md\n\nagents body")
            .await
            .expect("agents should be written");
        fs::write(workspace.join("SOUL.md"), "# SOUL.md\n\nsoul body")
            .await
            .expect("soul should be written");
        fs::write(
            workspace.join("IDENTITY.md"),
            "# IDENTITY.md\n\nidentity body",
        )
        .await
        .expect("identity should be written");
        fs::write(workspace.join("TOOLS.md"), "# TOOLS.md\n\ntools body")
            .await
            .expect("tools should be written");

        let prompt = format_inlined_workspace_context_for_prompt_in_dir(
            &workspace,
            &default_prompt_extensions(),
        )
        .expect("workspace context should be composed");

        assert!(prompt.contains("# AGENTS.md\n\nagents body"));
        assert!(prompt.contains("# SOUL.md\n\nsoul body"));
        assert!(prompt.contains("# IDENTITY.md\n\nidentity body"));
        assert!(prompt.contains("# TOOLS.md\n\ntools body"));

        let agents_pos = prompt.find("# AGENTS.md").expect("agents section");
        let soul_pos = prompt.find("# SOUL.md").expect("soul section");
        let identity_pos = prompt.find("# IDENTITY.md").expect("identity section");
        let tools_pos = prompt.find("# TOOLS.md").expect("tools section");

        assert!(agents_pos < soul_pos);
        assert!(soul_pos < identity_pos);
        assert!(identity_pos < tools_pos);
    }

    #[test]
    fn compose_runtime_prompt_places_workspace_context_before_existing_sections() {
        let prompt = compose_runtime_prompt(RuntimePromptInput {
            workspace_context: Some("## Workspace Context\n\nworkspace body".to_string()),
            runtime_metadata: None,
            rules: Some("rule-a".to_string()),
            local_docs: Some("docs-a".to_string()),
            additional_instructions: Some("extra-a".to_string()),
            skills: vec![],
        })
        .expect("composed prompt expected");

        let workspace_descriptor_pos = prompt.find("## Workspace").expect("workspace section");
        let workspace_pos = prompt
            .find("## Workspace Context")
            .expect("workspace content should exist");
        let rules_pos = prompt.find("## Rules").expect("rules should exist");
        let instructions_pos = prompt
            .find("## Instructions")
            .expect("instructions should exist");

        assert!(workspace_descriptor_pos < workspace_pos);
        assert!(workspace_pos < rules_pos);
        assert!(rules_pos < instructions_pos);
        assert!(prompt.contains("## Local Docs\n\ndocs-a"));
        assert!(prompt.contains("## Additional Instructions\n\nextra-a"));
    }

    // ---- Prompt Extension tests ----

    /// A simple mock extension for testing Handlebars rendering without
    /// depending on the actual system environment.
    struct MockExtension {
        name: &'static str,
        active: bool,
    }

    impl PromptExtension for MockExtension {
        fn name(&self) -> &str {
            self.name
        }
        fn is_active(&self) -> bool {
            self.active
        }
        fn prompt_section(&self) -> String {
            format!("## Extension: {}", self.name)
        }
    }

    #[test]
    fn render_template_keeps_active_if_block() {
        let input = "# AGENTS.md\n\n## Every Session\n\nDo stuff.\n\n{{#if rtk}}\n## Extension Agent: rtk\n\nPrefix with rtk.\n{{/if}}\n\n## Make It Yours\n";
        let extensions: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: true,
        })];
        let result = render_template_with_extensions(input, &extensions);

        assert!(result.contains("## Extension Agent: rtk"));
        assert!(result.contains("Prefix with rtk."));
        assert!(!result.contains("{{#if rtk}}"));
        assert!(!result.contains("{{/if}}"));
        assert!(result.contains("## Every Session"));
        assert!(result.contains("## Make It Yours"));
    }

    #[test]
    fn render_template_strips_inactive_if_block() {
        let input = "# AGENTS.md\n\n## Every Session\n\nDo stuff.\n\n{{#if rtk}}\n## Extension Agent: rtk\n\nPrefix with rtk.\n{{/if}}\n\n## Make It Yours\n";
        let extensions: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: false,
        })];
        let result = render_template_with_extensions(input, &extensions);

        assert!(!result.contains("## Extension Agent: rtk"));
        assert!(!result.contains("Prefix with rtk."));
        assert!(!result.contains("{{#if rtk}}"));
        assert!(!result.contains("{{/if}}"));
        assert!(result.contains("## Every Session"));
        assert!(result.contains("## Make It Yours"));
    }

    #[test]
    fn render_template_preserves_content_without_handlebars_syntax() {
        let input = "# AGENTS.md\n\n## Every Session\n\nDo stuff.\n\n## Make It Yours\n";
        let extensions: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: true,
        })];
        let result = render_template_with_extensions(input, &extensions);
        assert_eq!(result, input);
    }

    #[test]
    fn render_template_handles_multiple_extensions() {
        let input = "# AGENTS.md\n\n{{#if rtk}}\n## rtk\n\nrtk content\n{{/if}}\n\n{{#if docker}}\n## docker\n\ndocker content\n{{/if}}\n\n## End\n";
        let extensions: Vec<Arc<dyn PromptExtension>> = vec![
            Arc::new(MockExtension {
                name: "rtk",
                active: true,
            }),
            Arc::new(MockExtension {
                name: "docker",
                active: false,
            }),
        ];
        let result = render_template_with_extensions(input, &extensions);

        assert!(result.contains("## rtk"));
        assert!(result.contains("rtk content"));
        assert!(!result.contains("## docker"));
        assert!(!result.contains("docker content"));
        assert!(!result.contains("{{#if"));
        assert!(!result.contains("{{/if}}"));
        assert!(result.contains("## End"));
    }

    #[test]
    fn render_template_if_else_block_works() {
        let input = "{{#if rtk}}\nrtk is active\n{{else}}\nrtk is not available\n{{/if}}\n";
        let active_exts: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: true,
        })];
        let active_result = render_template_with_extensions(input, &active_exts);
        assert!(active_result.contains("rtk is active"));
        assert!(!active_result.contains("rtk is not available"));

        let inactive_exts: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: false,
        })];
        let inactive_result = render_template_with_extensions(input, &inactive_exts);
        assert!(!inactive_result.contains("rtk is active"));
        assert!(inactive_result.contains("rtk is not available"));
    }

    #[test]
    fn render_template_falls_back_on_invalid_syntax() {
        // Malformed Handlebars syntax should fall back to raw content.
        let input = "{{#if}}\nbroken block\n{{/if}}\n";
        let extensions: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: true,
        })];
        let result = render_template_with_extensions(input, &extensions);
        // Fallback returns raw content unchanged.
        assert_eq!(result, input);
    }

    #[test]
    fn rtk_prompt_extension_has_correct_name() {
        let ext = RtkPromptExtension;
        assert_eq!(ext.name(), "rtk");
    }

    #[test]
    fn rtk_prompt_extension_section_contains_shell_prefix_rule() {
        let ext = RtkPromptExtension;
        let section = ext.prompt_section();
        assert!(section.contains("## Extension Agent: rtk Command Proxy"));
        assert!(section.contains("always prefix the command with `rtk`"));
        assert!(section.contains("rtk git status"));
    }

    #[test]
    fn is_command_available_returns_false_for_nonexistent_command() {
        // A command that almost certainly does not exist on any system.
        assert!(!is_command_available("klaw_nonexistent_test_command_xyz"));
    }

    #[test]
    fn default_prompt_extensions_includes_rtk() {
        let exts = default_prompt_extensions();
        assert!(exts.iter().any(|e| e.name() == "rtk"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn format_inlined_workspace_context_omits_inactive_extension_section() {
        // Extension content is injected by code via prompt_section(), not
        // embedded in template files. Verify that inactive extensions do not
        // appear in the inlined workspace context.
        let workspace =
            std::env::temp_dir().join(format!("klaw-prompt-ext-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace)
            .await
            .expect("workspace dir should be created");

        let agents_content =
            get_default_template_content("AGENTS.md").expect("AGENTS.md template should exist");

        fs::write(workspace.join("AGENTS.md"), agents_content)
            .await
            .expect("agents should be written");
        fs::write(workspace.join("SOUL.md"), "# SOUL.md\n\nsoul body")
            .await
            .expect("soul should be written");
        fs::write(
            workspace.join("IDENTITY.md"),
            "# IDENTITY.md\n\nidentity body",
        )
        .await
        .expect("identity should be written");
        fs::write(workspace.join("TOOLS.md"), "# TOOLS.md\n\ntools body")
            .await
            .expect("tools should be written");

        // Use a mock rtk extension that is inactive to simulate a system without rtk.
        let inactive_exts: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: false,
        })];

        let prompt = format_inlined_workspace_context_for_prompt_in_dir(&workspace, &inactive_exts)
            .expect("workspace context should be composed");

        assert!(!prompt.contains("{{#if rtk}}"));
        assert!(!prompt.contains("{{/if}}"));
        assert!(!prompt.contains("## Extension: rtk"));
        assert!(prompt.contains("# AGENTS.md"));
        assert!(prompt.contains("## Make It Yours"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn format_inlined_workspace_context_injects_active_extension_section() {
        // Extension content is injected by code via prompt_section(), not
        // embedded in template files. Verify that active extensions appear
        // in the inlined workspace context.
        let workspace =
            std::env::temp_dir().join(format!("klaw-prompt-ext-active-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace)
            .await
            .expect("workspace dir should be created");

        let agents_content =
            get_default_template_content("AGENTS.md").expect("AGENTS.md template should exist");

        fs::write(workspace.join("AGENTS.md"), agents_content)
            .await
            .expect("agents should be written");
        fs::write(workspace.join("SOUL.md"), "# SOUL.md\n\nsoul body")
            .await
            .expect("soul should be written");
        fs::write(
            workspace.join("IDENTITY.md"),
            "# IDENTITY.md\n\nidentity body",
        )
        .await
        .expect("identity should be written");
        fs::write(workspace.join("TOOLS.md"), "# TOOLS.md\n\ntools body")
            .await
            .expect("tools should be written");

        // Use a mock rtk extension that is active to simulate a system with rtk.
        // MockExtension.prompt_section() returns "## Extension: rtk".
        let active_exts: Vec<Arc<dyn PromptExtension>> = vec![Arc::new(MockExtension {
            name: "rtk",
            active: true,
        })];

        let prompt = format_inlined_workspace_context_for_prompt_in_dir(&workspace, &active_exts)
            .expect("workspace context should be composed");

        assert!(!prompt.contains("{{#if rtk}}"));
        assert!(!prompt.contains("{{/if}}"));
        assert!(prompt.contains("## Extension: rtk"));
        assert!(prompt.contains("# AGENTS.md"));
        assert!(prompt.contains("## Make It Yours"));
    }
}
