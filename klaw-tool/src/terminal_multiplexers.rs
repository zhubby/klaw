use async_trait::async_trait;
use serde_json::{Value, json};
use std::io::ErrorKind;
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{fs, process::Command, time::timeout};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

/// 基于 zellij 的终端多路复用工具。
///
/// 支持 action:
/// - start_or_resume: 开启或恢复会话（后台）
/// - send: 向会话焦点 pane 发送内容
/// - capture: 捕获当前可见输出（可选 full scrollback）
/// - terminate: 终止指定会话
/// - list_sessions: 列出会话
/// - kill_all: 终止全部会话
///
/// 后端优先级：zellij -> tmux（当系统未安装 zellij 时自动回退到 tmux）。
pub struct TerminalMultiplexerTool;

#[derive(Debug, Clone, Copy)]
enum MultiplexerBackend {
    Zellij,
    Tmux,
}

impl TerminalMultiplexerTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_timeout_secs(args: &Value) -> Result<u64, ToolError> {
        match args.get("timeout") {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| ToolError::InvalidArgs("`timeout` must be an integer".to_string())),
            None => Ok(20),
        }
    }

    fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
        args.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ToolError::InvalidArgs(format!("missing `{key}`")))
    }

    fn backend_label(backend: MultiplexerBackend) -> &'static str {
        match backend {
            MultiplexerBackend::Zellij => "zellij",
            MultiplexerBackend::Tmux => "tmux",
        }
    }

    fn backend_bin(backend: MultiplexerBackend) -> &'static str {
        match backend {
            MultiplexerBackend::Zellij => "zellij",
            MultiplexerBackend::Tmux => "tmux",
        }
    }

    async fn command_available(
        &self,
        bin: &str,
        version_args: &[&str],
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> bool {
        let mut cmd = Command::new(bin);
        cmd.args(version_args);
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        if let Some(workspace) = ctx.metadata.get("workspace").and_then(Value::as_str) {
            cmd.current_dir(workspace);
        }

        match timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
            Ok(Ok(output)) => output.status.success(),
            Ok(Err(err)) if err.kind() == ErrorKind::NotFound => false,
            _ => false,
        }
    }

    async fn detect_backend(
        &self,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<MultiplexerBackend, ToolError> {
        if self
            .command_available("zellij", &["--version"], timeout_secs, ctx)
            .await
        {
            return Ok(MultiplexerBackend::Zellij);
        }
        if self
            .command_available("tmux", &["-V"], timeout_secs, ctx)
            .await
        {
            return Ok(MultiplexerBackend::Tmux);
        }
        Err(ToolError::ExecutionFailed(
            "neither zellij nor tmux is available on this system".to_string(),
        ))
    }

    async fn run_multiplexer(
        &self,
        backend: MultiplexerBackend,
        args: &[String],
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<std::process::Output, ToolError> {
        let mut cmd = Command::new(Self::backend_bin(backend));
        cmd.args(args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        if let Some(workspace) = ctx.metadata.get("workspace").and_then(Value::as_str) {
            cmd.current_dir(workspace);
        }

        timeout(Duration::from_secs(timeout_secs), cmd.output())
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!(
                    "{} timed out after {timeout_secs}s",
                    Self::backend_label(backend)
                ))
            })?
            .map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to run {}: {err}",
                    Self::backend_label(backend)
                ))
            })
    }

    fn ensure_success(
        output: &std::process::Output,
        step: &str,
        backend: MultiplexerBackend,
    ) -> Result<(), ToolError> {
        if output.status.success() {
            return Ok(());
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(ToolError::ExecutionFailed(format!(
            "{step} failed via {} (exit={:?})\nstdout:\n{}\nstderr:\n{}",
            Self::backend_label(backend),
            output.status.code(),
            if stdout.is_empty() {
                "<empty>"
            } else {
                &stdout
            },
            if stderr.is_empty() {
                "<empty>"
            } else {
                &stderr
            },
        )))
    }

    fn parse_session_names(raw: &str) -> Vec<String> {
        raw.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !line.contains("No active zellij sessions found"))
            .filter(|line| !line.contains("no server running on"))
            .map(|line| {
                line.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .filter(|line| !line.is_empty())
            .collect()
    }

    fn is_tmux_no_server_output(output: &std::process::Output) -> bool {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        stderr.contains("no server running on")
    }

    fn default_tmux_target(session: &str) -> String {
        format!("{session}:0.0")
    }

    async fn list_sessions(
        &self,
        backend: MultiplexerBackend,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<Vec<String>, ToolError> {
        let output = match backend {
            MultiplexerBackend::Zellij => {
                self.run_multiplexer(
                    backend,
                    &["list-sessions".to_string(), "--short".to_string()],
                    timeout_secs,
                    ctx,
                )
                .await?
            }
            MultiplexerBackend::Tmux => {
                self.run_multiplexer(
                    backend,
                    &[
                        "list-sessions".to_string(),
                        "-F".to_string(),
                        "#{session_name}".to_string(),
                    ],
                    timeout_secs,
                    ctx,
                )
                .await?
            }
        };

        if !output.status.success() {
            if matches!(backend, MultiplexerBackend::Tmux)
                && Self::is_tmux_no_server_output(&output)
            {
                return Ok(Vec::new());
            }
            Self::ensure_success(&output, "list sessions", backend)?;
        }

        Ok(Self::parse_session_names(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    async fn start_or_resume(
        &self,
        backend: MultiplexerBackend,
        session: &str,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let sessions = self.list_sessions(backend, timeout_secs, ctx).await?;
        if sessions.iter().any(|s| s == session) {
            let content = format!(
                "session `{session}` already exists and is available (backend: {}).",
                Self::backend_label(backend)
            );
            return Ok(ToolOutput {
                content_for_model: content.clone(),
                content_for_user: Some(content),
            });
        }

        let output = match backend {
            MultiplexerBackend::Zellij => {
                self.run_multiplexer(
                    backend,
                    &[
                        "attach".to_string(),
                        "--create-background".to_string(),
                        session.to_string(),
                    ],
                    timeout_secs,
                    ctx,
                )
                .await?
            }
            MultiplexerBackend::Tmux => {
                self.run_multiplexer(
                    backend,
                    &[
                        "new-session".to_string(),
                        "-d".to_string(),
                        "-s".to_string(),
                        session.to_string(),
                    ],
                    timeout_secs,
                    ctx,
                )
                .await?
            }
        };
        Self::ensure_success(&output, "start or resume session", backend)?;

        let content = format!(
            "session `{session}` started in background (backend: {}).",
            Self::backend_label(backend)
        );
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }

    async fn send(
        &self,
        backend: MultiplexerBackend,
        session: &str,
        content: &str,
        press_enter: bool,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        match backend {
            MultiplexerBackend::Zellij => {
                let write_out = self
                    .run_multiplexer(
                        backend,
                        &[
                            "--session".to_string(),
                            session.to_string(),
                            "action".to_string(),
                            "write-chars".to_string(),
                            content.to_string(),
                        ],
                        timeout_secs,
                        ctx,
                    )
                    .await?;
                Self::ensure_success(&write_out, "send chars", backend)?;

                if press_enter {
                    let enter_out = self
                        .run_multiplexer(
                            backend,
                            &[
                                "--session".to_string(),
                                session.to_string(),
                                "action".to_string(),
                                "write".to_string(),
                                "10".to_string(), // LF
                            ],
                            timeout_secs,
                            ctx,
                        )
                        .await?;
                    Self::ensure_success(&enter_out, "send enter", backend)?;
                }
            }
            MultiplexerBackend::Tmux => {
                let target = Self::default_tmux_target(session);
                let write_out = self
                    .run_multiplexer(
                        backend,
                        &[
                            "send-keys".to_string(),
                            "-t".to_string(),
                            target.clone(),
                            "-l".to_string(),
                            content.to_string(),
                        ],
                        timeout_secs,
                        ctx,
                    )
                    .await?;
                Self::ensure_success(&write_out, "send chars", backend)?;

                if press_enter {
                    let enter_out = self
                        .run_multiplexer(
                            backend,
                            &[
                                "send-keys".to_string(),
                                "-t".to_string(),
                                target,
                                "Enter".to_string(),
                            ],
                            timeout_secs,
                            ctx,
                        )
                        .await?;
                    Self::ensure_success(&enter_out, "send enter", backend)?;
                }
            }
        }

        let content = format!(
            "sent {} chars to session `{session}` (backend: {}).",
            content.chars().count(),
            Self::backend_label(backend)
        );
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }

    async fn capture(
        &self,
        backend: MultiplexerBackend,
        session: &str,
        full: bool,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        match backend {
            MultiplexerBackend::Zellij => {
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| ToolError::ExecutionFailed(format!("clock error: {err}")))?
                    .as_nanos();
                let path = std::env::temp_dir().join(format!("klaw-zellij-capture-{nonce}.txt"));
                let mut cmd = vec![
                    "--session".to_string(),
                    session.to_string(),
                    "action".to_string(),
                    "dump-screen".to_string(),
                ];
                if full {
                    cmd.push("--full".to_string());
                }
                cmd.push(path.to_string_lossy().to_string());

                let output = self
                    .run_multiplexer(backend, &cmd, timeout_secs, ctx)
                    .await?;
                Self::ensure_success(&output, "capture screen", backend)?;

                let captured = fs::read_to_string(&path).await.map_err(|err| {
                    ToolError::ExecutionFailed(format!("failed to read capture file: {err}"))
                })?;
                let _ = fs::remove_file(&path).await;

                Ok(ToolOutput {
                    content_for_model: captured.clone(),
                    content_for_user: Some(captured),
                })
            }
            MultiplexerBackend::Tmux => {
                let mut cmd = vec![
                    "capture-pane".to_string(),
                    "-p".to_string(),
                    "-t".to_string(),
                    Self::default_tmux_target(session),
                ];
                if full {
                    cmd.push("-S".to_string());
                    cmd.push("-".to_string());
                }
                let output = self
                    .run_multiplexer(backend, &cmd, timeout_secs, ctx)
                    .await?;
                Self::ensure_success(&output, "capture screen", backend)?;

                let captured = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(ToolOutput {
                    content_for_model: captured.clone(),
                    content_for_user: Some(captured),
                })
            }
        }
    }

    async fn terminate(
        &self,
        backend: MultiplexerBackend,
        session: &str,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let output = match backend {
            MultiplexerBackend::Zellij => {
                self.run_multiplexer(
                    backend,
                    &["kill-session".to_string(), session.to_string()],
                    timeout_secs,
                    ctx,
                )
                .await?
            }
            MultiplexerBackend::Tmux => {
                self.run_multiplexer(
                    backend,
                    &[
                        "kill-session".to_string(),
                        "-t".to_string(),
                        session.to_string(),
                    ],
                    timeout_secs,
                    ctx,
                )
                .await?
            }
        };
        Self::ensure_success(&output, "terminate session", backend)?;
        let content = format!(
            "session `{session}` terminated (backend: {}).",
            Self::backend_label(backend)
        );
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}

impl Default for TerminalMultiplexerTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TerminalMultiplexerTool {
    fn name(&self) -> &str {
        "terminal_multiplexer"
    }

    fn description(&self) -> &str {
        "Manage terminal multiplexing sessions with zellij (preferred) or tmux fallback: start/resume, send input, capture output, terminate, and session listing."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Operate one terminal multiplexer action per request.",
            "oneOf": [
                {
                    "description": "Start a new session or resume an existing one.",
                    "properties": {
                        "action": { "const": "start_or_resume" },
                        "session": { "type": "string", "description": "Session name." },
                        "timeout": { "type": "integer", "description": "Command timeout in seconds (default 20)." }
                    },
                    "required": ["action", "session"],
                    "additionalProperties": false
                },
                {
                    "description": "Send text input to a session.",
                    "properties": {
                        "action": { "const": "send" },
                        "session": { "type": "string", "description": "Session name." },
                        "content": { "type": "string", "description": "Text to send." },
                        "press_enter": { "type": "boolean", "description": "Append Enter after sending text (default true)." },
                        "timeout": { "type": "integer", "description": "Command timeout in seconds (default 20)." }
                    },
                    "required": ["action", "session", "content"],
                    "additionalProperties": false
                },
                {
                    "description": "Capture session output.",
                    "properties": {
                        "action": { "const": "capture" },
                        "session": { "type": "string", "description": "Session name." },
                        "full": { "type": "boolean", "description": "Capture full scrollback (default false)." },
                        "timeout": { "type": "integer", "description": "Command timeout in seconds (default 20)." }
                    },
                    "required": ["action", "session"],
                    "additionalProperties": false
                },
                {
                    "description": "Terminate one session.",
                    "properties": {
                        "action": { "const": "terminate" },
                        "session": { "type": "string", "description": "Session name." },
                        "timeout": { "type": "integer", "description": "Command timeout in seconds (default 20)." }
                    },
                    "required": ["action", "session"],
                    "additionalProperties": false
                },
                {
                    "description": "List all active sessions.",
                    "properties": {
                        "action": { "const": "list_sessions" },
                        "timeout": { "type": "integer", "description": "Command timeout in seconds (default 20)." }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "Terminate all sessions.",
                    "properties": {
                        "action": { "const": "kill_all" },
                        "timeout": { "type": "integer", "description": "Command timeout in seconds (default 20)." }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = Self::require_str(&args, "action")?;
        let timeout_secs = Self::parse_timeout_secs(&args)?;
        let backend = self.detect_backend(timeout_secs, ctx).await?;

        match action {
            "start_or_resume" => {
                let session = Self::require_str(&args, "session")?;
                self.start_or_resume(backend, session, timeout_secs, ctx)
                    .await
            }
            "send" => {
                let session = Self::require_str(&args, "session")?;
                let content = Self::require_str(&args, "content")?;
                let press_enter = args
                    .get("press_enter")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.send(backend, session, content, press_enter, timeout_secs, ctx)
                    .await
            }
            "capture" => {
                let session = Self::require_str(&args, "session")?;
                let full = args.get("full").and_then(Value::as_bool).unwrap_or(false);
                self.capture(backend, session, full, timeout_secs, ctx)
                    .await
            }
            "terminate" => {
                let session = Self::require_str(&args, "session")?;
                self.terminate(backend, session, timeout_secs, ctx).await
            }
            "list_sessions" => {
                let sessions = self.list_sessions(backend, timeout_secs, ctx).await?;
                let content = serde_json::to_string_pretty(&json!({
                    "backend": Self::backend_label(backend),
                    "sessions": sessions
                }))
                .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
                Ok(ToolOutput {
                    content_for_model: content.clone(),
                    content_for_user: Some(content),
                })
            }
            "kill_all" => {
                let output = match backend {
                    MultiplexerBackend::Zellij => {
                        self.run_multiplexer(
                            backend,
                            &["kill-all-sessions".to_string()],
                            timeout_secs,
                            ctx,
                        )
                        .await?
                    }
                    MultiplexerBackend::Tmux => {
                        self.run_multiplexer(
                            backend,
                            &["kill-server".to_string()],
                            timeout_secs,
                            ctx,
                        )
                        .await?
                    }
                };

                if !output.status.success() {
                    if !(matches!(backend, MultiplexerBackend::Tmux)
                        && Self::is_tmux_no_server_output(&output))
                    {
                        Self::ensure_success(&output, "kill all sessions", backend)?;
                    }
                }

                let content = format!(
                    "all sessions terminated (backend: {}).",
                    Self::backend_label(backend)
                );
                Ok(ToolOutput {
                    content_for_model: content.clone(),
                    content_for_user: Some(content),
                })
            }
            other => Err(ToolError::InvalidArgs(format!(
                "unsupported action `{other}`"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn parse_session_names_works_for_short_output() {
        let raw = "alpha\nbeta\n";
        assert_eq!(
            TerminalMultiplexerTool::parse_session_names(raw),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn parse_session_names_handles_empty_state() {
        let raw = "No active zellij sessions found.\n";
        assert!(TerminalMultiplexerTool::parse_session_names(raw).is_empty());
    }

    #[test]
    fn parse_session_names_handles_tmux_no_server() {
        let raw = "no server running on /tmp/tmux-1000/default\n";
        assert!(TerminalMultiplexerTool::parse_session_names(raw).is_empty());
    }

    #[tokio::test]
    async fn execute_requires_action() {
        let tool = TerminalMultiplexerTool::new();
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };
        let err = tool.execute(json!({}), &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("missing `action`"));
    }
}
