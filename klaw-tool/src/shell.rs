use async_trait::async_trait;
use klaw_config::AppConfig;
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::Duration;
use tokio::{process::Command, time::timeout};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

/// Shell 工具，执行本地 shell 命令并返回输出。
pub struct ShellTool {
    blocked_patterns: Vec<String>,
}

impl ShellTool {
    /// 从应用配置读取 shell 拦截规则。
    pub fn new(config: &AppConfig) -> Self {
        Self {
            blocked_patterns: config.tools.shell.blocked_patterns.clone(),
        }
    }

    /// 自定义模式：由配置注入拦截规则。
    pub fn with_blocked_patterns(blocked_patterns: Vec<String>) -> Self {
        Self { blocked_patterns }
    }

    /// 宽松模式：不拦截命令内容。
    pub fn permissive() -> Self {
        Self {
            blocked_patterns: Vec::new(),
        }
    }

    fn validate_command(&self, command: &str) -> Result<(), ToolError> {
        let normalized = command.to_ascii_lowercase();
        if let Some(pattern) = self
            .blocked_patterns
            .iter()
            .find(|pattern| normalized.contains(&pattern.to_ascii_lowercase()))
        {
            return Err(ToolError::ExecutionFailed(format!(
                "security violation: blocked pattern `{pattern}`"
            )));
        }
        Ok(())
    }

    fn format_output(stdout: &[u8], stderr: &[u8], exit_code: Option<i32>) -> String {
        let stdout_text = String::from_utf8_lossy(stdout).trim().to_string();
        let stderr_text = String::from_utf8_lossy(stderr).trim().to_string();

        let mut parts = Vec::new();
        if !stdout_text.is_empty() {
            parts.push(stdout_text);
        }
        if !stderr_text.is_empty() {
            parts.push(format!("--- stderr ---\n{stderr_text}"));
        }

        let code = exit_code.unwrap_or(-1);
        parts.push(format!("[Exit code: {code}]"));
        parts.join("\n")
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout/stderr."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 60)"
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing `command`".to_string()))?;

        self.validate_command(command)?;

        let timeout_secs = match args.get("timeout") {
            Some(v) => v.as_u64().ok_or_else(|| {
                ToolError::InvalidArgs("`timeout` must be an integer".to_string())
            })?,
            None => 60,
        };

        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        process.stdout(Stdio::piped());
        process.stderr(Stdio::piped());

        if let Some(workspace) = ctx.metadata.get("workspace").and_then(Value::as_str) {
            process.current_dir(workspace);
        }

        let output = timeout(Duration::from_secs(timeout_secs), process.output())
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!("command timed out after {timeout_secs}s"))
            })?
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        let content = Self::format_output(&output.stdout, &output.stderr, output.status.code());
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{ModelProviderConfig, ShellConfig, ToolsConfig};
    use serde_json::json;
    use std::{collections::BTreeMap, fs};

    fn test_config() -> AppConfig {
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
            tools: ToolsConfig {
                shell: ShellConfig {
                    blocked_patterns: vec!["rm -rf /".to_string()],
                },
            },
        }
    }

    fn permissive_test_config() -> AppConfig {
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
            tools: ToolsConfig::default(),
        }
    }

    #[tokio::test]
    async fn test_shell_echo() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };

        let result = tool.execute(json!({"command": "echo hello"}), &ctx).await;
        assert!(result.is_ok());
        let output = result.unwrap().content_for_model;
        assert!(output.contains("hello"));
        assert!(output.contains("[Exit code: 0]"));
    }

    #[tokio::test]
    async fn test_shell_stderr() {
        let config = permissive_test_config();
        let tool = ShellTool::new(&config);
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };

        let result = tool
            .execute(json!({"command": "echo error >&2"}), &ctx)
            .await
            .unwrap();
        assert!(result.content_for_model.contains("--- stderr ---"));
        assert!(result.content_for_model.contains("error"));
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
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };

        let result = tool
            .execute(json!({"command": "sleep 2", "timeout": 1}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_dangerous_command_blocked() {
        let config = test_config();
        let tool = ShellTool::new(&config);
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };

        let result = tool.execute(json!({"command": "rm -rf /"}), &ctx).await;
        assert!(result.is_err());
    }
}
