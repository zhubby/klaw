use crate::approval::{ApprovalCreateInput, ApprovalStoreService};
use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};
use async_trait::async_trait;
use klaw_config::{AppConfig, ShellApprovalPolicy};
use klaw_storage::{DefaultSessionStore, SessionStorage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use tokio::{process::Command, time::timeout};
use tracing::info;

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const META_WORKSPACE: &str = "workspace";
const META_SHELL_PATH: &str = "shell.path";
const META_SHELL_APPROVED: &str = "shell.approved";
const META_SHELL_APPROVAL_ID: &str = "shell.approval_id";
const APPROVAL_TTL_MINUTES: i64 = 10;

/// Shell 工具，执行本地 shell 命令并返回结构化输出。
pub struct ShellTool {
    config: klaw_config::ShellConfig,
    store: Option<DefaultSessionStore>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShellRequest {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandRisk {
    Safe,
    Mutating,
    Destructive,
}

#[derive(Debug, Serialize)]
struct ShellExecutionResult {
    success: bool,
    command: String,
    cwd: String,
    risk: &'static str,
    approval_required: bool,
    approved: bool,
    timed_out: bool,
    exit_code: Option<i32>,
    duration_ms: u64,
    stdout: String,
    stderr: String,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

impl ShellTool {
    /// 从应用配置读取 shell 规则。
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.tools.shell.clone(),
            store: None,
        }
    }

    /// 从应用配置读取 shell 规则，并注入会话存储用于审批单持久化。
    pub fn with_store(config: &AppConfig, store: DefaultSessionStore) -> Self {
        Self {
            config: config.tools.shell.clone(),
            store: Some(store),
        }
    }

    /// 自定义模式：由配置注入拦截规则。
    pub fn with_config(config: klaw_config::ShellConfig) -> Self {
        Self {
            config,
            store: None,
        }
    }

    /// 自定义模式并注入审批存储。
    pub fn with_config_and_store(
        config: klaw_config::ShellConfig,
        store: DefaultSessionStore,
    ) -> Self {
        Self {
            config,
            store: Some(store),
        }
    }

    /// 宽松模式：不拦截命令内容，不要求审批。
    pub fn permissive() -> Self {
        let mut config = klaw_config::ShellConfig::default();
        config.blocked_patterns.clear();
        config.approval_policy = ShellApprovalPolicy::Never;
        Self {
            config,
            store: None,
        }
    }

    fn parse_request(args: Value) -> Result<ShellRequest, ToolError> {
        let mut request: ShellRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;

        request.command = request.command.trim().to_string();
        if request.command.is_empty() {
            return Err(ToolError::InvalidArgs(
                "`command` cannot be empty".to_string(),
            ));
        }

        if let Some(workdir) = request.workdir.as_mut() {
            *workdir = workdir.trim().to_string();
            if workdir.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "`workdir` cannot be empty".to_string(),
                ));
            }
        }

        Ok(request)
    }

    fn classify_risk(&self, command: &str) -> CommandRisk {
        let normalized = command.to_ascii_lowercase();
        if self
            .config
            .blocked_patterns
            .iter()
            .any(|pattern| normalized.contains(&pattern.to_ascii_lowercase()))
        {
            return CommandRisk::Destructive;
        }

        if Self::contains_shell_operators(command) {
            return CommandRisk::Mutating;
        }

        let Some(first_token) = command.split_whitespace().next() else {
            return CommandRisk::Mutating;
        };
        let first_token = first_token.to_ascii_lowercase();
        if self
            .config
            .safe_commands
            .iter()
            .any(|item| item.eq_ignore_ascii_case(&first_token))
        {
            CommandRisk::Safe
        } else {
            CommandRisk::Mutating
        }
    }

    fn contains_shell_operators(command: &str) -> bool {
        ["&&", "||", "|", ";", ">", "<", "$(", "`"]
            .iter()
            .any(|op| command.contains(op))
    }

    async fn check_approval(
        &self,
        risk: CommandRisk,
        command: &str,
        session_key: &str,
        metadata: &std::collections::BTreeMap<String, Value>,
    ) -> Result<(bool, bool), ToolError> {
        let approval_required = !matches!(risk, CommandRisk::Safe);

        if !approval_required {
            return Ok((false, true));
        }

        if matches!(self.config.approval_policy, ShellApprovalPolicy::Never) {
            return Err(ToolError::ExecutionFailed(
                "command requires approval but shell approval policy is `never`".to_string(),
            ));
        }

        let approved_via_flag = metadata
            .get(META_SHELL_APPROVED)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if approved_via_flag {
            return Ok((true, true));
        }

        let command_hash = Self::command_hash(command);
        if let (Some(store), Some(approval_id)) = (
            self.store.as_ref(),
            metadata.get(META_SHELL_APPROVAL_ID).and_then(Value::as_str),
        ) {
            let consumed = store
                .consume_approved_shell_command(
                    approval_id,
                    session_key,
                    &command_hash,
                    Self::now_ms(),
                )
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to validate approval `{approval_id}`: {err}"
                    ))
                })?;
            if consumed {
                return Ok((true, true));
            }
        }

        if let Some(store) = self.store.as_ref() {
            let consumed = store
                .consume_latest_approved_shell_command(session_key, &command_hash, Self::now_ms())
                .await
                .map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to consume approved command for session `{session_key}`: {err}"
                    ))
                })?;
            if consumed {
                return Ok((true, true));
            }
        }

        if let Some(store) = self.store.as_ref() {
            let approval = ApprovalStoreService::new(store.clone())
                .create(ApprovalCreateInput {
                    session_key: session_key.to_string(),
                    tool_name: "shell".to_string(),
                    command_text: command.trim().to_string(),
                    command_preview: Some(Self::command_preview(command)),
                    command_hash: Some(command_hash),
                    risk_level: Some(
                        match risk {
                            CommandRisk::Safe => "safe",
                            CommandRisk::Mutating => "mutating",
                            CommandRisk::Destructive => "destructive",
                        }
                        .to_string(),
                    ),
                    requested_by: Some("agent".to_string()),
                    justification: None,
                    expires_in_minutes: Some(APPROVAL_TTL_MINUTES),
                })
                .await?;
            return Err(ToolError::ExecutionFailed(format!(
                "approval required: approval_id={}; approve it then retry with metadata `shell.approval_id`",
                approval.id
            )));
        }

        Err(ToolError::ExecutionFailed(
            "approval required: set metadata `shell.approved=true`".to_string(),
        ))
    }

    fn resolve_workspace_base(&self, ctx: &ToolContext) -> Result<PathBuf, ToolError> {
        if let Some(workspace) = ctx.metadata.get(META_WORKSPACE).and_then(Value::as_str) {
            return std::fs::canonicalize(workspace).map_err(|err| {
                ToolError::ExecutionFailed(format!("invalid workspace path: {err}"))
            });
        }
        if let Some(workspace) = self.config.workspace.as_deref() {
            return std::fs::canonicalize(workspace).map_err(|err| {
                ToolError::ExecutionFailed(format!("invalid shell workspace path: {err}"))
            });
        }
        std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to resolve home dir: {err}")))
    }

    fn resolve_cwd(request: &ShellRequest, base: &Path) -> Result<PathBuf, ToolError> {
        let target = match request.workdir.as_deref() {
            Some(workdir) => {
                let path = PathBuf::from(workdir);
                if path.is_absolute() {
                    path
                } else {
                    base.join(path)
                }
            }
            None => base.to_path_buf(),
        };

        let canonical = std::fs::canonicalize(&target).map_err(|err| {
            ToolError::ExecutionFailed(format!("invalid workdir `{}`: {err}", target.display()))
        })?;
        if !canonical.is_dir() {
            return Err(ToolError::ExecutionFailed(format!(
                "workdir `{}` is not a directory",
                canonical.display()
            )));
        }

        if canonical.starts_with(base) {
            return Ok(canonical);
        }

        Err(ToolError::ExecutionFailed(format!(
            "workdir `{}` is outside workspace `{}`",
            canonical.display(),
            base.display()
        )))
    }

    fn resolve_timeout(&self, request: &ShellRequest) -> Result<u64, ToolError> {
        let timeout_ms = request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
        if timeout_ms == 0 {
            return Err(ToolError::InvalidArgs(
                "`timeout_ms` must be greater than 0".to_string(),
            ));
        }
        Ok(timeout_ms.min(self.config.max_timeout_ms))
    }

    fn resolve_shell_bin(ctx: &ToolContext) -> String {
        if let Some(shell) = ctx.metadata.get(META_SHELL_PATH).and_then(Value::as_str) {
            let shell = shell.trim();
            if !shell.is_empty() {
                return shell.to_string();
            }
        }
        std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
    }

    fn shell_args(shell_bin: &str, command: &str) -> Vec<String> {
        let normalized = shell_bin.to_ascii_lowercase();
        if normalized.contains("pwsh") || normalized.contains("powershell") {
            return vec![
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                command.to_string(),
            ];
        }
        vec!["-c".to_string(), command.to_string()]
    }

    fn truncate_stream(bytes: &[u8], max: usize) -> (String, bool) {
        if bytes.len() <= max {
            return (String::from_utf8_lossy(bytes).trim().to_string(), false);
        }
        let truncated = &bytes[..max];
        (String::from_utf8_lossy(truncated).trim().to_string(), true)
    }

    fn format_output(result: &ShellExecutionResult) -> Result<String, ToolError> {
        serde_json::to_string_pretty(result)
            .map_err(|err| ToolError::ExecutionFailed(format!("failed to serialize output: {err}")))
    }

    fn command_hash(command: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(command.trim().as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn command_preview(command: &str) -> String {
        let trimmed = command.trim();
        let max = 160;
        if trimmed.chars().count() <= max {
            return trimmed.to_string();
        }
        let mut preview = trimmed.chars().take(max).collect::<String>();
        preview.push_str("...");
        preview
    }

    fn now_ms() -> i64 {
        (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a local shell command with workspace-aware path controls, approval checks, timeout/output limits, and structured result output."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Arguments for shell command execution with approval checks.",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command string passed to the selected shell.",
                    "minLength": 1,
                    "examples": [
                        "ls -la",
                        "cargo check --workspace",
                        "rg -n \"ToolRegistry\" klaw-core/src"
                    ]
                },
                "workdir": {
                    "type": "string",
                    "description": "Optional working directory. Relative paths are resolved from workspace metadata.",
                    "examples": [".", "klaw-tool", "/tmp"]
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Execution timeout in milliseconds. Defaults to 60000 and is clamped by tools.shell.max_timeout_ms.",
                    "minimum": 1,
                    "default": 60000
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;

        let risk = self.classify_risk(&request.command);
        if matches!(risk, CommandRisk::Destructive) {
            return Err(ToolError::ExecutionFailed(
                "security violation: command matched blocked patterns".to_string(),
            ));
        }

        let (approval_required, approved) = self
            .check_approval(risk, &request.command, &ctx.session_key, &ctx.metadata)
            .await?;
        let base = self.resolve_workspace_base(ctx)?;
        let cwd = Self::resolve_cwd(&request, &base)?;
        let timeout_ms = self.resolve_timeout(&request)?;

        let shell_bin = Self::resolve_shell_bin(ctx);
        let args = Self::shell_args(&shell_bin, &request.command);

        info!(
            session_key = %ctx.session_key,
            tool = "shell",
            command = %request.command,
            cwd = %cwd.display(),
            risk = ?risk,
            approval_required,
            approved,
            "shell command begin"
        );

        let started = Instant::now();
        let mut command = Command::new(&shell_bin);
        command.args(args);
        command.current_dir(&cwd);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.kill_on_drop(true);
        command.env("KLAW_SESSION_KEY", &ctx.session_key);

        let output = timeout(Duration::from_millis(timeout_ms), command.output())
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!("command timed out after {timeout_ms}ms"))
            })?
            .map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to execute command: {err}"))
            })?;

        let per_stream_limit = (self.config.max_output_bytes / 2).max(1);
        let (stdout, stdout_truncated) = Self::truncate_stream(&output.stdout, per_stream_limit);
        let (stderr, stderr_truncated) = Self::truncate_stream(&output.stderr, per_stream_limit);
        let duration_ms = started.elapsed().as_millis() as u64;

        let result = ShellExecutionResult {
            success: output.status.success(),
            command: request.command,
            cwd: cwd.display().to_string(),
            risk: match risk {
                CommandRisk::Safe => "safe",
                CommandRisk::Mutating => "mutating",
                CommandRisk::Destructive => "destructive",
            },
            approval_required,
            approved,
            timed_out: false,
            exit_code: output.status.code(),
            duration_ms,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
        };

        info!(
            session_key = %ctx.session_key,
            tool = "shell",
            success = result.success,
            exit_code = result.exit_code,
            duration_ms = result.duration_ms,
            stdout_truncated = result.stdout_truncated,
            stderr_truncated = result.stderr_truncated,
            "shell command finish"
        );

        let content = Self::format_output(&result)?;
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{ModelProviderConfig, ShellApprovalPolicy, ShellConfig};
    use klaw_storage::{ApprovalStatus, SessionStorage, StoragePaths};
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::{collections::BTreeMap, fs};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn base_config() -> AppConfig {
        let mut providers = BTreeMap::new();
        providers.insert(
            "openai".to_string(),
            ModelProviderConfig {
                name: None,
                base_url: "https://api.openai.com/v1".to_string(),
                wire_api: "chat_completions".to_string(),
                default_model: "gpt-4o-mini".to_string(),
                api_key: None,
                env_key: Some("OPENAI_API_KEY".to_string()),
            },
        );
        AppConfig {
            model_provider: "openai".to_string(),
            model_providers: providers,
            ..Default::default()
        }
    }

    fn test_config() -> AppConfig {
        let mut cfg = base_config();
        cfg.tools.shell = ShellConfig {
            enabled: true,
            workspace: None,
            blocked_patterns: vec!["rm -rf /".to_string()],
            safe_commands: vec![
                "echo".to_string(),
                "cat".to_string(),
                "ls".to_string(),
                "pwd".to_string(),
                "sleep".to_string(),
                "printf".to_string(),
            ],
            approval_policy: ShellApprovalPolicy::OnRequest,
            allow_login_shell: true,
            max_timeout_ms: 5_000,
            max_output_bytes: 4 * 1024,
        };
        cfg
    }

    fn permissive_test_config() -> AppConfig {
        let mut cfg = base_config();
        cfg.tools.shell = ShellConfig {
            enabled: true,
            workspace: None,
            blocked_patterns: vec![],
            safe_commands: vec![
                "echo".to_string(),
                "cat".to_string(),
                "ls".to_string(),
                "pwd".to_string(),
                "sleep".to_string(),
                "printf".to_string(),
            ],
            approval_policy: ShellApprovalPolicy::Never,
            allow_login_shell: true,
            max_timeout_ms: 5_000,
            max_output_bytes: 4 * 1024,
        };
        cfg
    }

    fn base_ctx() -> ToolContext {
        ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    async fn create_store() -> DefaultSessionStore {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "klaw-shell-approval-test-{}-{suffix}",
            ShellTool::now_ms()
        ));
        DefaultSessionStore::open(StoragePaths::from_root(base))
            .await
            .expect("store should open")
    }

    #[tokio::test]
    async fn test_shell_echo() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);

        let result = tool
            .execute(json!({"command": "echo hello"}), &base_ctx())
            .await;
        assert!(result.is_ok());
        let output = result.unwrap().content_for_model;
        assert!(output.contains("\"success\": true"));
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn test_shell_structured_failure_on_non_zero_exit() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);

        let result = tool
            .execute(json!({"command": "cat does-not-exist.txt"}), &base_ctx())
            .await
            .unwrap();

        assert!(result.content_for_model.contains("\"success\": false"));
        assert!(result.content_for_model.contains("\"exit_code\": 1"));
    }

    #[tokio::test]
    async fn test_shell_with_workspace_from_metadata() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);
        let dir = std::env::temp_dir().join(format!(
            "klaw-shell-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("sample.txt"), "workspace-ok").unwrap();

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "workspace".to_string(),
            json!(dir.to_string_lossy().to_string()),
        );
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata,
        };

        let result = tool
            .execute(json!({"command": "cat sample.txt"}), &ctx)
            .await
            .unwrap();

        assert!(result.content_for_model.contains("workspace-ok"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn test_shell_timeout() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);

        let result = tool
            .execute(json!({"command": "sleep 2", "timeout_ms": 10}), &base_ctx())
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("timed out after 10ms"));
    }

    #[tokio::test]
    async fn test_dangerous_command_blocked() {
        let config = test_config();
        let tool = ShellTool::new(&config);

        let result = tool
            .execute(json!({"command": "rm -rf /"}), &base_ctx())
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("security violation"));
    }

    #[tokio::test]
    async fn test_mutating_command_requires_approval() {
        let config = test_config();
        let tool = ShellTool::new(&config);

        let result = tool
            .execute(json!({"command": "touch file.txt"}), &base_ctx())
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("approval required"));
    }

    #[tokio::test]
    async fn test_mutating_command_succeeds_when_approved() {
        let config = test_config();
        let tool = ShellTool::new(&config);

        let mut metadata = BTreeMap::new();
        metadata.insert("shell.approved".to_string(), json!(true));
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata,
        };

        let result = tool
            .execute(
                json!({
                    "command": "touch file.txt"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.content_for_model.contains("\"approved\": true"));
        let _ = fs::remove_file("file.txt");
    }

    #[tokio::test]
    async fn test_workdir_outside_workspace_is_rejected() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);

        let base = std::env::temp_dir().join(format!(
            "klaw-shell-workspace-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let outside = std::env::temp_dir();
        fs::create_dir_all(&base).unwrap();

        let mut metadata = BTreeMap::new();
        metadata.insert(
            "workspace".to_string(),
            json!(base.to_string_lossy().to_string()),
        );
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata,
        };

        let result = tool
            .execute(
                json!({
                    "command": "pwd",
                    "workdir": outside.to_string_lossy().to_string()
                }),
                &ctx,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("outside workspace"), "unexpected error: {err}");
        let _ = fs::remove_dir_all(base);
    }

    #[tokio::test]
    async fn test_timeout_is_clamped_by_config() {
        let mut cfg = permissive_test_config();
        cfg.tools.shell.max_timeout_ms = 100;
        let tool = ShellTool::new(&cfg);

        let result = tool
            .execute(
                json!({"command": "sleep 2", "timeout_ms": 10_000}),
                &base_ctx(),
            )
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("timed out after 100ms"));
    }

    #[tokio::test]
    async fn test_unknown_fields_rejected() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);

        let result = tool
            .execute(json!({"command": "echo hi", "timeout": 1}), &base_ctx())
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[tokio::test]
    async fn test_removed_fields_rejected_as_unknown() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);

        let result = tool
            .execute(
                json!({
                    "command": "echo hi",
                    "sandbox_permissions": "require_escalated"
                }),
                &base_ctx(),
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[tokio::test]
    async fn test_output_truncation() {
        let mut cfg = permissive_test_config();
        cfg.tools.shell.max_output_bytes = 64;
        let tool = ShellTool::new(&cfg);

        let result = tool
            .execute(
                json!({"command": "printf '1234567890abcdefghijklmnopqrstuvwxyz1234567890abcdefghijklmnopqrstuvwxyz'"}),
                &base_ctx(),
            )
            .await
            .unwrap();

        assert!(result
            .content_for_model
            .contains("\"stdout_truncated\": true"));
    }

    #[tokio::test]
    async fn test_mutating_command_creates_pending_approval() {
        let config = test_config();
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "stdio")
            .await
            .expect("session should exist");
        let tool = ShellTool::with_store(&config, store.clone());

        let result = tool
            .execute(json!({"command": "touch file.txt"}), &base_ctx())
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("approval_id="), "unexpected error: {err}");
        let approval_id = err
            .split("approval_id=")
            .nth(1)
            .and_then(|tail| tail.split(';').next())
            .expect("approval id should be present");
        let approval = store
            .get_approval(approval_id)
            .await
            .expect("approval should be persisted");
        assert_eq!(approval.session_key, "s1");
        assert_eq!(approval.tool_name, "shell");
        assert_eq!(approval.status.as_str(), "pending");
    }

    #[tokio::test]
    async fn test_mutating_command_executes_with_approved_approval_id_once() {
        let config = test_config();
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "stdio")
            .await
            .expect("session should exist");
        let tool = ShellTool::with_store(&config, store.clone());

        let first = tool
            .execute(json!({"command": "touch file.txt"}), &base_ctx())
            .await;
        assert!(first.is_err());
        let first_err = first.unwrap_err().to_string();
        let approval_id = first_err
            .split("approval_id=")
            .nth(1)
            .and_then(|tail| tail.split(';').next())
            .expect("approval id should be present")
            .to_string();

        store
            .update_approval_status(&approval_id, ApprovalStatus::Approved, Some("user"))
            .await
            .expect("approval should transition to approved");

        let mut metadata = BTreeMap::new();
        metadata.insert("shell.approval_id".to_string(), json!(approval_id.clone()));
        let approved_ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata,
        };
        let approved_exec = tool
            .execute(json!({"command": "touch file.txt"}), &approved_ctx)
            .await
            .expect("approved command should execute");
        assert!(approved_exec
            .content_for_model
            .contains("\"approved\": true"));

        let consumed = store
            .get_approval(&approval_id)
            .await
            .expect("approval should exist");
        assert_eq!(consumed.status, ApprovalStatus::Consumed);
    }

    #[tokio::test]
    async fn test_consumed_approval_id_cannot_be_replayed() {
        let config = test_config();
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "stdio")
            .await
            .expect("session should exist");
        let tool = ShellTool::with_store(&config, store.clone());

        let first = tool
            .execute(json!({"command": "touch file.txt"}), &base_ctx())
            .await;
        let first_err = first.expect_err("should require approval").to_string();
        let approval_id = first_err
            .split("approval_id=")
            .nth(1)
            .and_then(|tail| tail.split(';').next())
            .expect("approval id should be present")
            .to_string();

        store
            .update_approval_status(&approval_id, ApprovalStatus::Approved, Some("user"))
            .await
            .expect("approval should transition to approved");

        let mut metadata = BTreeMap::new();
        metadata.insert("shell.approval_id".to_string(), json!(approval_id.clone()));
        let approved_ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: metadata.clone(),
        };
        let _ = tool
            .execute(json!({"command": "touch file.txt"}), &approved_ctx)
            .await
            .expect("first approved execution should pass");

        let replay = tool
            .execute(
                json!({"command": "touch file.txt"}),
                &ToolContext {
                    session_key: "s1".to_string(),
                    metadata,
                },
            )
            .await;
        let replay_err = replay.expect_err("replay should fail").to_string();
        assert!(replay_err.contains("approval required"));

        let _ = fs::remove_file("file.txt");
    }

    #[tokio::test]
    async fn test_mutating_command_executes_after_approve_without_metadata_id() {
        let config = test_config();
        let store = create_store().await;
        store
            .touch_session("s1", "chat-1", "stdio")
            .await
            .expect("session should exist");
        let tool = ShellTool::with_store(&config, store.clone());

        let first = tool
            .execute(json!({"command": "touch file.txt"}), &base_ctx())
            .await;
        let first_err = first.expect_err("should require approval").to_string();
        let approval_id = first_err
            .split("approval_id=")
            .nth(1)
            .and_then(|tail| tail.split(';').next())
            .expect("approval id should be present")
            .to_string();

        store
            .update_approval_status(&approval_id, ApprovalStatus::Approved, Some("user"))
            .await
            .expect("approval should transition to approved");

        let approved_exec = tool
            .execute(json!({"command": "touch file.txt"}), &base_ctx())
            .await
            .expect("approved command should execute by hash match");
        assert!(approved_exec
            .content_for_model
            .contains("\"approved\": true"));

        let consumed = store
            .get_approval(&approval_id)
            .await
            .expect("approval should exist");
        assert_eq!(consumed.status, ApprovalStatus::Consumed);

        let _ = fs::remove_file("file.txt");
    }
}
