use async_trait::async_trait;
use klaw_util::command_search_path;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::fs;
use tokio::process::Command;
use tokio::time::{Instant, sleep, timeout};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolSignal};

const DEFAULT_TIMEOUT_SECS: u64 = 20;
const DEFAULT_HISTORY_LINES: usize = 200;
const DEFAULT_WAIT_HISTORY_LINES: usize = 2000;
const DEFAULT_POLL_INTERVAL_MS: u64 = 500;
const DEFAULT_MAX_AUTO_OBSERVE_STEPS: u32 = 5;
const SOCKET_DIR_ENV: &str = "KLAW_TMUX_SOCKET_DIR";
const SOCKET_FILE_NAME: &str = "k.sock";

/// 基于 tmux 私有 socket 的终端多路复用工具。
///
/// 设计目标是稳定驱动交互式 TTY 程序（如 codex、REPL、调试器），
/// 因此工具会统一使用独立 socket、显式 pane target、结构化输出与轮询同步。
pub struct TerminalMultiplexerTool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SessionMetadata {
    socket_path: String,
    session_name: String,
    default_target: String,
    created_at: String,
    last_used_at: String,
    #[serde(default)]
    last_turn_id: Option<String>,
    #[serde(default)]
    observation_steps_used: u32,
    purpose: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationStateUpdate {
    Reset,
    Increment,
}

#[derive(Debug, Clone)]
struct SocketLayout {
    socket_dir: PathBuf,
    socket_path: PathBuf,
    metadata_dir: PathBuf,
}

impl TerminalMultiplexerTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse_timeout_secs(args: &Value) -> Result<u64, ToolError> {
        match args.get("timeout") {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| ToolError::InvalidArgs("`timeout` must be an integer".to_string())),
            None => Ok(DEFAULT_TIMEOUT_SECS),
        }
    }

    fn parse_history_lines(
        args: &Value,
        key: &str,
        default_value: usize,
    ) -> Result<usize, ToolError> {
        match args.get(key) {
            Some(v) => {
                let value = v
                    .as_u64()
                    .ok_or_else(|| ToolError::InvalidArgs(format!("`{key}` must be an integer")))?;
                usize::try_from(value)
                    .map_err(|_| ToolError::InvalidArgs(format!("`{key}` is too large")))
            }
            None => Ok(default_value),
        }
    }

    fn parse_poll_interval_ms(args: &Value) -> Result<u64, ToolError> {
        match args.get("poll_interval_ms") {
            Some(v) => v.as_u64().ok_or_else(|| {
                ToolError::InvalidArgs("`poll_interval_ms` must be an integer".to_string())
            }),
            None => Ok(DEFAULT_POLL_INTERVAL_MS),
        }
    }

    fn current_turn_id(ctx: &ToolContext) -> Option<String> {
        ctx.metadata
            .get("agent.message_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn max_auto_observe_steps(ctx: &ToolContext) -> Result<u32, ToolError> {
        match ctx
            .metadata
            .get("terminal_multiplexer.max_auto_observe_steps")
        {
            Some(value) => {
                let value = value.as_u64().ok_or_else(|| {
                    ToolError::InvalidArgs(
                        "`terminal_multiplexer.max_auto_observe_steps` must be an integer"
                            .to_string(),
                    )
                })?;
                u32::try_from(value).map_err(|_| {
                    ToolError::InvalidArgs(
                        "`terminal_multiplexer.max_auto_observe_steps` is too large".to_string(),
                    )
                })
            }
            None => Ok(DEFAULT_MAX_AUTO_OBSERVE_STEPS),
        }
    }

    fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
        args.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::InvalidArgs(format!("missing `{key}`")))
    }

    fn optional_str(args: &Value, key: &str) -> Result<Option<String>, ToolError> {
        match args.get(key) {
            Some(Value::String(value)) => {
                let value = value.trim();
                if value.is_empty() {
                    return Err(ToolError::InvalidArgs(format!("`{key}` cannot be empty")));
                }
                Ok(Some(value.to_string()))
            }
            Some(_) => Err(ToolError::InvalidArgs(format!("`{key}` must be a string"))),
            None => Ok(None),
        }
    }

    fn validate_session_name(session: &str) -> Result<(), ToolError> {
        if session.chars().any(char::is_whitespace) {
            return Err(ToolError::InvalidArgs(
                "`session` cannot contain whitespace".to_string(),
            ));
        }
        if session.contains(':') || session.contains('.') {
            return Err(ToolError::InvalidArgs(
                "`session` cannot contain `:` or `.`".to_string(),
            ));
        }
        Ok(())
    }

    fn parse_keys(args: &Value) -> Result<Option<Vec<String>>, ToolError> {
        let Some(value) = args.get("keys") else {
            return Ok(None);
        };
        match value {
            Value::String(key) => {
                let key = key.trim();
                if key.is_empty() {
                    return Err(ToolError::InvalidArgs("`keys` cannot be empty".to_string()));
                }
                Ok(Some(vec![key.to_string()]))
            }
            Value::Array(items) => {
                if items.is_empty() {
                    return Err(ToolError::InvalidArgs("`keys` cannot be empty".to_string()));
                }
                let mut parsed = Vec::with_capacity(items.len());
                for item in items {
                    let key = item.as_str().ok_or_else(|| {
                        ToolError::InvalidArgs("`keys` array entries must be strings".to_string())
                    })?;
                    let key = key.trim();
                    if key.is_empty() {
                        return Err(ToolError::InvalidArgs(
                            "`keys` array entries cannot be empty".to_string(),
                        ));
                    }
                    parsed.push(key.to_string());
                }
                Ok(Some(parsed))
            }
            _ => Err(ToolError::InvalidArgs(
                "`keys` must be a string or array of strings".to_string(),
            )),
        }
    }

    fn session_target(session: &str, target: Option<&str>) -> String {
        target
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{session}:0.0"))
    }

    fn safe_session_slug(session: &str) -> String {
        session
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                    ch
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn format_timestamp(now: OffsetDateTime) -> Result<String, ToolError> {
        now.format(&Rfc3339)
            .map_err(|err| ToolError::ExecutionFailed(format!("format timestamp failed: {err}")))
    }

    fn now_timestamp() -> Result<String, ToolError> {
        Self::format_timestamp(OffsetDateTime::now_utc())
    }

    fn socket_layout(&self, ctx: &ToolContext) -> SocketLayout {
        let socket_dir = ctx
            .metadata
            .get("tmux_socket_dir")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os(SOCKET_DIR_ENV).map(PathBuf::from))
            .unwrap_or_else(|| std::env::temp_dir().join("klaw-tmux-sockets"));
        let socket_path = socket_dir.join(SOCKET_FILE_NAME);
        let metadata_dir = socket_dir.join("metadata");
        SocketLayout {
            socket_dir,
            socket_path,
            metadata_dir,
        }
    }

    async fn ensure_layout(&self, layout: &SocketLayout) -> Result<(), ToolError> {
        fs::create_dir_all(&layout.socket_dir)
            .await
            .map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to create tmux socket directory `{}`: {err}",
                    layout.socket_dir.display()
                ))
            })?;
        fs::create_dir_all(&layout.metadata_dir)
            .await
            .map_err(|err| {
                ToolError::ExecutionFailed(format!(
                    "failed to create tmux metadata directory `{}`: {err}",
                    layout.metadata_dir.display()
                ))
            })?;
        Ok(())
    }

    fn metadata_path(layout: &SocketLayout, session: &str) -> PathBuf {
        layout
            .metadata_dir
            .join(format!("{}.json", Self::safe_session_slug(session)))
    }

    async fn write_metadata(
        &self,
        layout: &SocketLayout,
        session: &str,
        purpose: Option<String>,
        ctx: &ToolContext,
        observation_update: ObservationStateUpdate,
    ) -> Result<SessionMetadata, ToolError> {
        self.ensure_layout(layout).await?;
        let metadata_path = Self::metadata_path(layout, session);
        let now = Self::now_timestamp()?;
        let current_turn_id = Self::current_turn_id(ctx);
        let metadata = match self.read_metadata(layout, session).await? {
            Some(mut existing) => {
                Self::apply_observation_update(
                    &mut existing,
                    current_turn_id.as_deref(),
                    observation_update,
                );
                existing.last_used_at = now.clone();
                if purpose.is_some() {
                    existing.purpose = purpose;
                }
                existing
            }
            None => SessionMetadata {
                socket_path: layout.socket_path.to_string_lossy().to_string(),
                session_name: session.to_string(),
                default_target: Self::session_target(session, None),
                created_at: now.clone(),
                last_used_at: now,
                last_turn_id: current_turn_id,
                observation_steps_used: match observation_update {
                    ObservationStateUpdate::Increment => 1,
                    ObservationStateUpdate::Reset => 0,
                },
                purpose,
            },
        };
        let raw = serde_json::to_vec_pretty(&metadata).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize session metadata failed: {err}"))
        })?;
        fs::write(&metadata_path, raw).await.map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to write session metadata `{}`: {err}",
                metadata_path.display()
            ))
        })?;
        Ok(metadata)
    }

    fn apply_observation_update(
        metadata: &mut SessionMetadata,
        current_turn_id: Option<&str>,
        observation_update: ObservationStateUpdate,
    ) {
        let turn_changed = current_turn_id != metadata.last_turn_id.as_deref();
        if turn_changed {
            metadata.observation_steps_used = 0;
        }
        metadata.last_turn_id = current_turn_id.map(ToString::to_string);
        match observation_update {
            ObservationStateUpdate::Reset => metadata.observation_steps_used = 0,
            ObservationStateUpdate::Increment => {
                metadata.observation_steps_used = metadata.observation_steps_used.saturating_add(1);
            }
        }
    }

    async fn read_metadata(
        &self,
        layout: &SocketLayout,
        session: &str,
    ) -> Result<Option<SessionMetadata>, ToolError> {
        let metadata_path = Self::metadata_path(layout, session);
        let raw = match fs::read(&metadata_path).await {
            Ok(raw) => raw,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(ToolError::ExecutionFailed(format!(
                    "failed to read session metadata `{}`: {err}",
                    metadata_path.display()
                )));
            }
        };
        let metadata = serde_json::from_slice(&raw).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to parse session metadata `{}`: {err}",
                metadata_path.display()
            ))
        })?;
        Ok(Some(metadata))
    }

    async fn delete_metadata(&self, layout: &SocketLayout, session: &str) -> Result<(), ToolError> {
        let metadata_path = Self::metadata_path(layout, session);
        match fs::remove_file(&metadata_path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(ToolError::ExecutionFailed(format!(
                "failed to delete session metadata `{}`: {err}",
                metadata_path.display()
            ))),
        }
    }

    async fn remove_all_metadata(&self, layout: &SocketLayout) -> Result<(), ToolError> {
        match fs::read_dir(&layout.metadata_dir).await {
            Ok(mut dir) => {
                while let Some(entry) = dir.next_entry().await.map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "failed to read tmux metadata directory `{}`: {err}",
                        layout.metadata_dir.display()
                    ))
                })? {
                    let path = entry.path();
                    if path.extension() == Some(OsStr::new("json")) {
                        let _ = fs::remove_file(path).await;
                    }
                }
                Ok(())
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(ToolError::ExecutionFailed(format!(
                "failed to inspect tmux metadata directory `{}`: {err}",
                layout.metadata_dir.display()
            ))),
        }
    }

    fn monitor_attach_command(socket_path: &Path, session: &str) -> String {
        format!(
            "tmux -S '{}' attach -t '{}'",
            socket_path.display(),
            session
        )
    }

    fn monitor_capture_command(socket_path: &Path, target: &str) -> String {
        format!(
            "tmux -S '{}' capture-pane -p -J -t '{}' -S -200",
            socket_path.display(),
            target
        )
    }

    fn tmux_missing_error() -> ToolError {
        ToolError::structured_execution_failed(
            "tmux is not available on this system",
            "tmux_not_installed",
            None,
            false,
            Vec::new(),
        )
    }

    fn tmux_state_error(
        message: impl Into<String>,
        code: &'static str,
        layout: Option<&SocketLayout>,
        session: Option<&str>,
        target: Option<&str>,
        retryable: bool,
    ) -> ToolError {
        let mut details = serde_json::Map::new();
        if let Some(layout) = layout {
            details.insert(
                "socket".to_string(),
                Value::String(layout.socket_path.to_string_lossy().to_string()),
            );
        }
        if let Some(session) = session {
            details.insert("session".to_string(), Value::String(session.to_string()));
        }
        if let Some(target) = target {
            details.insert("target".to_string(), Value::String(target.to_string()));
        }
        ToolError::structured_execution_failed(
            message,
            code,
            (!details.is_empty()).then_some(Value::Object(details)),
            retryable,
            Vec::new(),
        )
    }

    async fn ensure_tmux_available(
        &self,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<(), ToolError> {
        let mut cmd = tmux_command();
        cmd.arg("-V");
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        if let Some(workspace) = ctx.metadata.get("workspace").and_then(Value::as_str) {
            cmd.current_dir(workspace);
        }

        match timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
            Ok(Ok(output)) if output.status.success() => Ok(()),
            Ok(Err(err)) if err.kind() == ErrorKind::NotFound => Err(Self::tmux_missing_error()),
            Ok(Ok(_)) | Ok(Err(_)) | Err(_) => Err(Self::tmux_missing_error()),
        }
    }

    async fn run_tmux(
        &self,
        layout: &SocketLayout,
        args: &[String],
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<std::process::Output, ToolError> {
        let mut cmd = tmux_command();
        cmd.arg("-S").arg(&layout.socket_path);
        cmd.args(args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        if let Some(workspace) = ctx.metadata.get("workspace").and_then(Value::as_str) {
            cmd.current_dir(workspace);
        }

        match timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(err)) if err.kind() == ErrorKind::NotFound => Err(Self::tmux_missing_error()),
            Ok(Err(err)) => Err(ToolError::ExecutionFailed(format!(
                "failed to run tmux: {err}"
            ))),
            Err(_) => Err(ToolError::ExecutionFailed(format!(
                "tmux timed out after {timeout_secs}s"
            ))),
        }
    }

    fn combined_output_text(output: &std::process::Output) -> String {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stdout.trim().is_empty() {
            stderr.into_owned()
        } else if stderr.trim().is_empty() {
            stdout.into_owned()
        } else {
            format!("{stdout}\n{stderr}")
        }
    }

    fn is_no_server_output(output: &std::process::Output) -> bool {
        let lower = Self::combined_output_text(output).to_ascii_lowercase();
        lower.contains("no server running on")
            || (lower.contains("error connecting to")
                && lower.contains("no such file or directory"))
    }

    fn classify_tmux_failure(
        output: &std::process::Output,
        layout: &SocketLayout,
        session: Option<&str>,
        target: Option<&str>,
        step: &str,
    ) -> ToolError {
        let text = Self::combined_output_text(output);
        let lower = text.to_ascii_lowercase();
        if lower.contains("no server running on")
            || (lower.contains("error connecting to")
                && lower.contains("no such file or directory"))
        {
            return Self::tmux_state_error(
                format!("{step} failed because the klaw tmux socket is not running"),
                "socket_not_found",
                Some(layout),
                session,
                target,
                true,
            );
        }
        if lower.contains("can't find session") || lower.contains("failed to connect to server") {
            return Self::tmux_state_error(
                format!("{step} failed because tmux session was not found"),
                "session_not_found",
                Some(layout),
                session,
                target,
                true,
            );
        }
        if lower.contains("can't find pane")
            || lower.contains("can't find window")
            || lower.contains("can't find client")
        {
            return Self::tmux_state_error(
                format!("{step} failed because tmux target was not found"),
                "target_not_found",
                Some(layout),
                session,
                target,
                true,
            );
        }

        ToolError::structured_execution_failed(
            format!("{step} failed: {}", text.trim()),
            "tmux_command_failed",
            Some(json!({
                "socket": layout.socket_path.to_string_lossy().to_string(),
                "session": session,
                "target": target,
                "stdout": String::from_utf8_lossy(&output.stdout).trim(),
                "stderr": String::from_utf8_lossy(&output.stderr).trim(),
                "exit_code": output.status.code(),
            })),
            true,
            Vec::new(),
        )
    }

    fn ensure_success(
        output: &std::process::Output,
        layout: &SocketLayout,
        session: Option<&str>,
        target: Option<&str>,
        step: &str,
    ) -> Result<(), ToolError> {
        if output.status.success() {
            Ok(())
        } else {
            Err(Self::classify_tmux_failure(
                output, layout, session, target, step,
            ))
        }
    }

    fn parse_session_names(raw: &str) -> Vec<String> {
        raw.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !line.contains("no server running on"))
            .map(ToOwned::to_owned)
            .collect()
    }

    async fn list_session_names(
        &self,
        layout: &SocketLayout,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<Vec<String>, ToolError> {
        let output = self
            .run_tmux(
                layout,
                &[
                    "list-sessions".to_string(),
                    "-F".to_string(),
                    "#{session_name}".to_string(),
                ],
                timeout_secs,
                ctx,
            )
            .await?;
        if !output.status.success() {
            if Self::is_no_server_output(&output) {
                return Ok(Vec::new());
            }
            return Err(Self::classify_tmux_failure(
                &output,
                layout,
                None,
                None,
                "list sessions",
            ));
        }
        Ok(Self::parse_session_names(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    async fn session_exists(
        &self,
        layout: &SocketLayout,
        session: &str,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<bool, ToolError> {
        Ok(self
            .list_session_names(layout, timeout_secs, ctx)
            .await?
            .iter()
            .any(|name| name == session))
    }

    async fn ensure_target_exists(
        &self,
        layout: &SocketLayout,
        target: &str,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<(), ToolError> {
        let output = self
            .run_tmux(
                layout,
                &[
                    "list-panes".to_string(),
                    "-t".to_string(),
                    target.to_string(),
                    "-F".to_string(),
                    "#{pane_id}".to_string(),
                ],
                timeout_secs,
                ctx,
            )
            .await?;
        Self::ensure_success(&output, layout, None, Some(target), "inspect pane target")
    }

    async fn capture_pane(
        &self,
        layout: &SocketLayout,
        target: &str,
        history_lines: usize,
        full: bool,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let mut cmd = vec![
            "capture-pane".to_string(),
            "-p".to_string(),
            "-J".to_string(),
            "-t".to_string(),
            target.to_string(),
        ];
        if full {
            cmd.push("-S".to_string());
            cmd.push("-".to_string());
        } else {
            cmd.push("-S".to_string());
            cmd.push(format!("-{}", history_lines.max(1)));
        }
        let output = self.run_tmux(layout, &cmd, timeout_secs, ctx).await?;
        Self::ensure_success(&output, layout, None, Some(target), "capture pane output")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn build_session_response(
        &self,
        layout: &SocketLayout,
        session: &str,
        target: &str,
        summary: String,
        metadata: SessionMetadata,
        extra: Value,
    ) -> Result<ToolOutput, ToolError> {
        let mut value = serde_json::Map::from_iter([
            ("backend".to_string(), Value::String("tmux".to_string())),
            (
                "socket".to_string(),
                Value::String(layout.socket_path.to_string_lossy().to_string()),
            ),
            ("session".to_string(), Value::String(session.to_string())),
            ("target".to_string(), Value::String(target.to_string())),
            (
                "monitor_attach_command".to_string(),
                Value::String(Self::monitor_attach_command(&layout.socket_path, session)),
            ),
            (
                "monitor_capture_command".to_string(),
                Value::String(Self::monitor_capture_command(&layout.socket_path, target)),
            ),
            ("summary".to_string(), Value::String(summary)),
            (
                "metadata".to_string(),
                serde_json::to_value(metadata).map_err(|err| {
                    ToolError::ExecutionFailed(format!(
                        "serialize session metadata response failed: {err}"
                    ))
                })?,
            ),
        ]);
        if let Value::Object(extra_map) = extra {
            value.extend(extra_map);
        }
        let rendered = serde_json::to_string_pretty(&Value::Object(value)).map_err(|err| {
            ToolError::ExecutionFailed(format!("serialize response failed: {err}"))
        })?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }

    fn observation_budget_error(
        layout: &SocketLayout,
        session: &str,
        target: &str,
        metadata: &SessionMetadata,
        captured: String,
        max_auto_observe_steps: u32,
        reason: &str,
    ) -> ToolError {
        ToolError::structured_execution_failed(
            format!(
                "automatic tmux observation budget exhausted after {} steps; summarize the current tmux state for the user and wait for their decision before continuing",
                metadata.observation_steps_used
            ),
            "observation_budget_exhausted",
            Some(json!({
                "reason": reason,
                "backend": "tmux",
                "socket": layout.socket_path.to_string_lossy().to_string(),
                "session": session,
                "target": target,
                "observation_steps_used": metadata.observation_steps_used,
                "max_auto_observe_steps": max_auto_observe_steps,
                "monitor_attach_command": Self::monitor_attach_command(&layout.socket_path, session),
                "monitor_capture_command": Self::monitor_capture_command(&layout.socket_path, target),
                "captured": captured,
            })),
            false,
            vec![ToolSignal::stop_current_turn(
                Some("terminal_multiplexer_observation_budget_exhausted"),
                Some("terminal_multiplexer"),
            )],
        )
    }

    async fn start_or_resume(
        &self,
        args: &Value,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let session = Self::require_str(args, "session")?;
        Self::validate_session_name(session)?;
        let initial_command = Self::optional_str(args, "initial_command")?;
        let purpose = Self::optional_str(args, "purpose")?;
        let layout = self.socket_layout(ctx);
        self.ensure_layout(&layout).await?;

        let existed = self
            .session_exists(&layout, session, timeout_secs, ctx)
            .await?;
        if !existed {
            let output = self
                .run_tmux(
                    &layout,
                    &[
                        "new-session".to_string(),
                        "-d".to_string(),
                        "-s".to_string(),
                        session.to_string(),
                        "-n".to_string(),
                        "shell".to_string(),
                    ],
                    timeout_secs,
                    ctx,
                )
                .await?;
            Self::ensure_success(&output, &layout, Some(session), None, "start tmux session")?;
        }

        let target = Self::session_target(session, None);
        self.ensure_target_exists(&layout, &target, timeout_secs, ctx)
            .await?;

        if let Some(command) = initial_command.as_deref() {
            self.send_to_target(
                &layout,
                session,
                &target,
                Some(command),
                None,
                true,
                timeout_secs,
                ctx,
            )
            .await?;
        }

        let metadata = self
            .write_metadata(
                &layout,
                session,
                purpose,
                ctx,
                ObservationStateUpdate::Reset,
            )
            .await?;
        let summary = if existed {
            format!("session `{session}` resumed on klaw tmux socket")
        } else {
            format!("session `{session}` started on klaw tmux socket")
        };
        self.build_session_response(
            &layout,
            session,
            &target,
            summary,
            metadata,
            json!({
                "created": !existed,
                "initial_command_sent": initial_command.is_some(),
            }),
        )
        .await
    }

    async fn send_to_target(
        &self,
        layout: &SocketLayout,
        session: &str,
        target: &str,
        text: Option<&str>,
        keys: Option<&[String]>,
        press_enter: bool,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<(), ToolError> {
        self.ensure_target_exists(layout, target, timeout_secs, ctx)
            .await?;
        if let Some(text) = text {
            let output = self
                .run_tmux(
                    layout,
                    &[
                        "send-keys".to_string(),
                        "-t".to_string(),
                        target.to_string(),
                        "-l".to_string(),
                        text.to_string(),
                    ],
                    timeout_secs,
                    ctx,
                )
                .await?;
            Self::ensure_success(
                &output,
                layout,
                Some(session),
                Some(target),
                "send text to pane",
            )?;
            if press_enter {
                let enter_output = self
                    .run_tmux(
                        layout,
                        &[
                            "send-keys".to_string(),
                            "-t".to_string(),
                            target.to_string(),
                            "Enter".to_string(),
                        ],
                        timeout_secs,
                        ctx,
                    )
                    .await?;
                Self::ensure_success(
                    &enter_output,
                    layout,
                    Some(session),
                    Some(target),
                    "send enter to pane",
                )?;
            }
        }
        if let Some(keys) = keys {
            let mut cmd = vec![
                "send-keys".to_string(),
                "-t".to_string(),
                target.to_string(),
            ];
            cmd.extend(keys.iter().cloned());
            let output = self.run_tmux(layout, &cmd, timeout_secs, ctx).await?;
            Self::ensure_success(
                &output,
                layout,
                Some(session),
                Some(target),
                "send key sequence to pane",
            )?;
        }
        Ok(())
    }

    async fn send(
        &self,
        args: &Value,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let session = Self::require_str(args, "session")?;
        Self::validate_session_name(session)?;
        let content = Self::optional_str(args, "content")?;
        let keys = Self::parse_keys(args)?;
        if content.is_none() == keys.is_none() {
            return Err(ToolError::InvalidArgs(
                "exactly one of `content` or `keys` is required".to_string(),
            ));
        }
        let press_enter = args
            .get("press_enter")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if keys.is_some() && args.get("press_enter").is_some() {
            return Err(ToolError::InvalidArgs(
                "`press_enter` can only be used with `content`".to_string(),
            ));
        }

        let layout = self.socket_layout(ctx);
        let target = Self::session_target(session, args.get("target").and_then(Value::as_str));
        self.send_to_target(
            &layout,
            session,
            &target,
            content.as_deref(),
            keys.as_deref(),
            press_enter,
            timeout_secs,
            ctx,
        )
        .await?;

        let metadata = self
            .write_metadata(&layout, session, None, ctx, ObservationStateUpdate::Reset)
            .await?;
        let summary = if let Some(content) = content.as_deref() {
            format!("sent {} chars to `{target}`", content.chars().count())
        } else {
            format!(
                "sent {} key events to `{target}`",
                keys.as_ref().map_or(0, Vec::len)
            )
        };
        self.build_session_response(
            &layout,
            session,
            &target,
            summary,
            metadata,
            json!({
                "sent_text": content.is_some(),
                "keys": keys,
                "press_enter": content.is_some() && press_enter,
            }),
        )
        .await
    }

    async fn capture(
        &self,
        args: &Value,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let session = Self::require_str(args, "session")?;
        Self::validate_session_name(session)?;
        let full = args.get("full").and_then(Value::as_bool).unwrap_or(false);
        let history_lines =
            Self::parse_history_lines(args, "history_lines", DEFAULT_HISTORY_LINES)?;
        let max_auto_observe_steps = Self::max_auto_observe_steps(ctx)?;
        let layout = self.socket_layout(ctx);
        let target = Self::session_target(session, args.get("target").and_then(Value::as_str));
        let captured = self
            .capture_pane(&layout, &target, history_lines, full, timeout_secs, ctx)
            .await?;
        let metadata = self
            .write_metadata(
                &layout,
                session,
                None,
                ctx,
                ObservationStateUpdate::Increment,
            )
            .await?;
        if metadata.observation_steps_used >= max_auto_observe_steps {
            return Err(Self::observation_budget_error(
                &layout,
                session,
                &target,
                &metadata,
                captured,
                max_auto_observe_steps,
                "capture",
            ));
        }
        self.build_session_response(
            &layout,
            session,
            &target,
            format!("captured output from `{target}`"),
            metadata,
            json!({
                "full": full,
                "history_lines": history_lines,
                "captured": captured,
            }),
        )
        .await
    }

    async fn wait_for_text(
        &self,
        args: &Value,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let session = Self::require_str(args, "session")?;
        Self::validate_session_name(session)?;
        let pattern = Self::require_str(args, "pattern")?;
        let use_regex = args.get("regex").and_then(Value::as_bool).unwrap_or(false);
        let history_lines =
            Self::parse_history_lines(args, "history_lines", DEFAULT_WAIT_HISTORY_LINES)?;
        let poll_interval_ms = Self::parse_poll_interval_ms(args)?;
        let max_auto_observe_steps = Self::max_auto_observe_steps(ctx)?;
        let layout = self.socket_layout(ctx);
        let target = Self::session_target(session, args.get("target").and_then(Value::as_str));
        let compiled_regex = if use_regex {
            Some(Regex::new(pattern).map_err(|err| {
                ToolError::InvalidArgs(format!("invalid regex in `pattern`: {err}"))
            })?)
        } else {
            None
        };

        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        loop {
            let captured = self
                .capture_pane(&layout, &target, history_lines, false, timeout_secs, ctx)
                .await?;
            let matched = if let Some(regex) = &compiled_regex {
                regex.is_match(&captured)
            } else {
                captured.contains(pattern)
            };
            if matched {
                let metadata = self
                    .write_metadata(
                        &layout,
                        session,
                        None,
                        ctx,
                        ObservationStateUpdate::Increment,
                    )
                    .await?;
                if metadata.observation_steps_used >= max_auto_observe_steps {
                    return Err(Self::observation_budget_error(
                        &layout,
                        session,
                        &target,
                        &metadata,
                        captured,
                        max_auto_observe_steps,
                        "wait_for_text",
                    ));
                }
                return self
                    .build_session_response(
                        &layout,
                        session,
                        &target,
                        format!("matched pattern in `{target}`"),
                        metadata,
                        json!({
                            "pattern": pattern,
                            "regex": use_regex,
                            "history_lines": history_lines,
                            "matched": true,
                            "captured": captured,
                        }),
                    )
                    .await;
            }
            if Instant::now() >= deadline {
                return Err(Self::tmux_state_error(
                    format!("timed out waiting for pattern `{pattern}` in tmux pane"),
                    "wait_timeout",
                    Some(&layout),
                    Some(session),
                    Some(&target),
                    true,
                ));
            }
            sleep(Duration::from_millis(poll_interval_ms)).await;
        }
    }

    async fn terminate(
        &self,
        args: &Value,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let session = Self::require_str(args, "session")?;
        Self::validate_session_name(session)?;
        let layout = self.socket_layout(ctx);
        let target = Self::session_target(session, None);
        let output = self
            .run_tmux(
                &layout,
                &[
                    "kill-session".to_string(),
                    "-t".to_string(),
                    session.to_string(),
                ],
                timeout_secs,
                ctx,
            )
            .await?;
        Self::ensure_success(
            &output,
            &layout,
            Some(session),
            Some(&target),
            "terminate session",
        )?;
        self.delete_metadata(&layout, session).await?;
        let rendered = serde_json::to_string_pretty(&json!({
            "backend": "tmux",
            "socket": layout.socket_path.to_string_lossy().to_string(),
            "session": session,
            "target": target,
            "summary": format!("session `{session}` terminated"),
            "monitor_attach_command": Self::monitor_attach_command(&layout.socket_path, session),
            "monitor_capture_command": Self::monitor_capture_command(&layout.socket_path, &Self::session_target(session, None)),
        }))
        .map_err(|err| ToolError::ExecutionFailed(format!("serialize response failed: {err}")))?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }

    async fn list_sessions(
        &self,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let layout = self.socket_layout(ctx);
        let sessions = self.list_session_names(&layout, timeout_secs, ctx).await?;
        let mut items = Vec::with_capacity(sessions.len());
        for session in sessions {
            let metadata = self.read_metadata(&layout, &session).await?;
            let target = Self::session_target(&session, None);
            items.push(json!({
                "session": session,
                "target": target,
                "monitor_attach_command": Self::monitor_attach_command(&layout.socket_path, &session),
                "monitor_capture_command": Self::monitor_capture_command(&layout.socket_path, &Self::session_target(&session, None)),
                "metadata": metadata,
            }));
        }
        let rendered = serde_json::to_string_pretty(&json!({
            "backend": "tmux",
            "socket": layout.socket_path.to_string_lossy().to_string(),
            "session_count": items.len(),
            "sessions": items,
        }))
        .map_err(|err| ToolError::ExecutionFailed(format!("serialize response failed: {err}")))?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }

    async fn kill_all(
        &self,
        timeout_secs: u64,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let layout = self.socket_layout(ctx);
        let output = self
            .run_tmux(&layout, &["kill-server".to_string()], timeout_secs, ctx)
            .await?;
        if !output.status.success() && !Self::is_no_server_output(&output) {
            return Err(Self::classify_tmux_failure(
                &output,
                &layout,
                None,
                None,
                "kill tmux server",
            ));
        }
        self.remove_all_metadata(&layout).await?;
        let rendered = serde_json::to_string_pretty(&json!({
            "backend": "tmux",
            "socket": layout.socket_path.to_string_lossy().to_string(),
            "summary": "all klaw tmux sessions terminated",
        }))
        .map_err(|err| ToolError::ExecutionFailed(format!("serialize response failed: {err}")))?;
        Ok(ToolOutput {
            content_for_model: rendered.clone(),
            content_for_user: Some(rendered),
        })
    }
}

fn tmux_command() -> Command {
    let mut cmd = Command::new("tmux");
    if let Some(path) = command_search_path() {
        cmd.env("PATH", path);
    }
    cmd
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
        "Operate interactive tmux sessions on a private socket: start or resume a session, send literal text or control keys to a pane, capture scrollback, wait for prompt text, list isolated sessions, or terminate them without touching the user's own tmux server."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Operate one tmux session action per request. All actions run against klaw's isolated tmux socket.",
            "oneOf": [
                {
                    "description": "Start a new background tmux session on the private socket, or reuse an existing one if it already exists.",
                    "properties": {
                        "action": { "const": "start_or_resume" },
                        "session": { "type": "string", "description": "Stable tmux session name. Use short slug-like names without spaces, `:`, or `.`." },
                        "initial_command": { "type": "string", "description": "Optional first command to type into the pane after the session is ready. Sent literally and followed by Enter." },
                        "purpose": { "type": "string", "description": "Optional metadata label describing the session, for example `codex`, `python`, or `shell`." },
                        "timeout": { "type": "integer", "description": "tmux command timeout in seconds. Default: 20." }
                    },
                    "required": ["action", "session"],
                    "additionalProperties": false
                },
                {
                    "description": "Send input to a tmux pane. Use `content` for literal text or `keys` for control keys such as `C-c`, `C-d`, `Escape`, or `Enter`.",
                    "properties": {
                        "action": { "const": "send" },
                        "session": { "type": "string", "description": "tmux session name." },
                        "target": { "type": "string", "description": "Optional explicit pane target like `session:0.0`. Defaults to the session's main pane." },
                        "content": { "type": "string", "description": "Literal text to send with `tmux send-keys -l`. Use this for shell commands, REPL code, or prompts." },
                        "keys": {
                            "oneOf": [
                                { "type": "string" },
                                { "type": "array", "items": { "type": "string" }, "minItems": 1 }
                            ],
                            "description": "One key or a sequence of tmux key names to send. Mutually exclusive with `content`."
                        },
                        "press_enter": { "type": "boolean", "description": "Only valid with `content`. When true, send Enter after the text. Default: true." },
                        "timeout": { "type": "integer", "description": "tmux command timeout in seconds. Default: 20." }
                    },
                    "required": ["action", "session"],
                    "additionalProperties": false
                },
                {
                    "description": "Capture recent output from a tmux pane with joined wrapped lines for reliable prompt scraping.",
                    "properties": {
                        "action": { "const": "capture" },
                        "session": { "type": "string", "description": "tmux session name." },
                        "target": { "type": "string", "description": "Optional explicit pane target like `session:0.0`." },
                        "history_lines": { "type": "integer", "description": "How many recent history lines to include. Default: 200." },
                        "full": { "type": "boolean", "description": "When true, capture the full tmux scrollback using `-S -`." },
                        "timeout": { "type": "integer", "description": "tmux command timeout in seconds. Default: 20." }
                    },
                    "required": ["action", "session"],
                    "additionalProperties": false
                },
                {
                    "description": "Poll tmux pane output until text appears. Use this to wait for prompts before sending the next command.",
                    "properties": {
                        "action": { "const": "wait_for_text" },
                        "session": { "type": "string", "description": "tmux session name." },
                        "target": { "type": "string", "description": "Optional explicit pane target like `session:0.0`." },
                        "pattern": { "type": "string", "description": "Text or regex to wait for in captured pane output." },
                        "regex": { "type": "boolean", "description": "Interpret `pattern` as a Rust regex. Default: false." },
                        "history_lines": { "type": "integer", "description": "How many recent lines to search on each poll. Default: 2000." },
                        "poll_interval_ms": { "type": "integer", "description": "Polling interval in milliseconds. Default: 500." },
                        "timeout": { "type": "integer", "description": "Maximum wait time in seconds. Default: 20." }
                    },
                    "required": ["action", "session", "pattern"],
                    "additionalProperties": false
                },
                {
                    "description": "Terminate one tmux session on the private socket.",
                    "properties": {
                        "action": { "const": "terminate" },
                        "session": { "type": "string", "description": "tmux session name." },
                        "timeout": { "type": "integer", "description": "tmux command timeout in seconds. Default: 20." }
                    },
                    "required": ["action", "session"],
                    "additionalProperties": false
                },
                {
                    "description": "List active tmux sessions on klaw's private socket only.",
                    "properties": {
                        "action": { "const": "list_sessions" },
                        "timeout": { "type": "integer", "description": "tmux command timeout in seconds. Default: 20." }
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "description": "Terminate every tmux session on klaw's private socket without touching the user's personal tmux server.",
                    "properties": {
                        "action": { "const": "kill_all" },
                        "timeout": { "type": "integer", "description": "tmux command timeout in seconds. Default: 20." }
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

        match action {
            "start_or_resume" => {
                self.ensure_tmux_available(timeout_secs, ctx).await?;
                self.start_or_resume(&args, timeout_secs, ctx).await
            }
            "send" => {
                self.ensure_tmux_available(timeout_secs, ctx).await?;
                self.send(&args, timeout_secs, ctx).await
            }
            "capture" => {
                self.ensure_tmux_available(timeout_secs, ctx).await?;
                self.capture(&args, timeout_secs, ctx).await
            }
            "wait_for_text" => {
                self.ensure_tmux_available(timeout_secs, ctx).await?;
                self.wait_for_text(&args, timeout_secs, ctx).await
            }
            "terminate" => {
                self.ensure_tmux_available(timeout_secs, ctx).await?;
                self.terminate(&args, timeout_secs, ctx).await
            }
            "list_sessions" => {
                self.ensure_tmux_available(timeout_secs, ctx).await?;
                self.list_sessions(timeout_secs, ctx).await
            }
            "kill_all" => {
                self.ensure_tmux_available(timeout_secs, ctx).await?;
                self.kill_all(timeout_secs, ctx).await
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

    fn test_ctx(socket_dir: &Path) -> ToolContext {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "tmux_socket_dir".to_string(),
            Value::String(socket_dir.to_string_lossy().to_string()),
        );
        metadata.insert(
            "agent.message_id".to_string(),
            Value::String("msg-1".to_string()),
        );
        ToolContext {
            session_key: "test-session".to_string(),
            metadata,
        }
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let now = OffsetDateTime::now_utc().unix_timestamp_nanos();
        PathBuf::from("/tmp").join(format!("ktt-{label}-{now}"))
    }

    fn tmux_available() -> bool {
        std::process::Command::new("tmux")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    #[test]
    fn parse_session_names_works_for_short_output() {
        let raw = "alpha\nbeta\n";
        assert_eq!(
            TerminalMultiplexerTool::parse_session_names(raw),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn parse_session_names_handles_tmux_no_server() {
        let raw = "no server running on /tmp/tmux-1000/default\n";
        assert!(TerminalMultiplexerTool::parse_session_names(raw).is_empty());
    }

    #[test]
    fn session_target_defaults_to_main_pane() {
        assert_eq!(
            TerminalMultiplexerTool::session_target("demo", None),
            "demo:0.0".to_string()
        );
    }

    #[test]
    fn observation_update_resets_on_new_turn() {
        let mut metadata = SessionMetadata {
            socket_path: "/tmp/k.sock".to_string(),
            session_name: "demo".to_string(),
            default_target: "demo:0.0".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            last_used_at: "2026-01-01T00:00:00Z".to_string(),
            last_turn_id: Some("msg-1".to_string()),
            observation_steps_used: 3,
            purpose: None,
        };
        TerminalMultiplexerTool::apply_observation_update(
            &mut metadata,
            Some("msg-2"),
            ObservationStateUpdate::Increment,
        );
        assert_eq!(metadata.last_turn_id.as_deref(), Some("msg-2"));
        assert_eq!(metadata.observation_steps_used, 1);
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

    #[tokio::test]
    async fn send_rejects_content_and_keys_together() {
        let tool = TerminalMultiplexerTool::new();
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };
        let err = tool
            .execute(
                json!({
                    "action": "send",
                    "session": "demo",
                    "content": "echo hi",
                    "keys": "Enter"
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "invalid_args");
        assert!(err.message().contains("exactly one of `content` or `keys`"));
    }

    #[tokio::test]
    async fn wait_for_text_requires_valid_regex() {
        let tool = TerminalMultiplexerTool::new();
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };
        let err = tool
            .execute(
                json!({
                    "action": "wait_for_text",
                    "session": "demo",
                    "pattern": "(",
                    "regex": true
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        if tmux_available() {
            assert_eq!(err.code(), "invalid_args");
        } else {
            assert_eq!(err.code(), "tmux_not_installed");
        }
    }

    #[tokio::test]
    async fn isolated_socket_start_send_capture_wait_and_kill_all() {
        if !tmux_available() {
            return;
        }

        let socket_dir = unique_test_dir("integration");
        let ctx = test_ctx(&socket_dir);
        let tool = TerminalMultiplexerTool::new();
        let session = "itest";

        let start = tool
            .execute(
                json!({
                    "action": "start_or_resume",
                    "session": session,
                    "purpose": "shell",
                    "initial_command": "printf 'session-ready\\n'"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(start.content_for_model.contains("\"created\": true"));

        let wait = tool
            .execute(
                json!({
                    "action": "wait_for_text",
                    "session": session,
                    "pattern": "session-ready",
                    "history_lines": 200,
                    "timeout": 5
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(wait.content_for_model.contains("\"matched\": true"));

        tool.execute(
            json!({
                "action": "send",
                "session": session,
                "content": "printf 'klaw-itest\\n'"
            }),
            &ctx,
        )
        .await
        .unwrap();

        let waited_for_output = tool
            .execute(
                json!({
                    "action": "wait_for_text",
                    "session": session,
                    "pattern": "klaw-itest",
                    "timeout": 5
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(waited_for_output.content_for_model.contains("klaw-itest"));

        let capture = tool
            .execute(
                json!({
                    "action": "capture",
                    "session": session,
                    "history_lines": 200
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(capture.content_for_model.contains("klaw-itest"));

        let list = tool
            .execute(json!({ "action": "list_sessions" }), &ctx)
            .await
            .unwrap();
        assert!(list.content_for_model.contains("\"session\": \"itest\""));

        let kill_all = tool
            .execute(json!({ "action": "kill_all" }), &ctx)
            .await
            .unwrap();
        assert!(
            kill_all
                .content_for_model
                .contains("all klaw tmux sessions terminated")
        );

        let _ = fs::remove_dir_all(socket_dir).await;
    }

    #[tokio::test]
    async fn wait_for_text_times_out_cleanly() {
        if !tmux_available() {
            return;
        }

        let socket_dir = unique_test_dir("timeout");
        let ctx = test_ctx(&socket_dir);
        let tool = TerminalMultiplexerTool::new();

        tool.execute(
            json!({
                "action": "start_or_resume",
                "session": "timeoutdemo"
            }),
            &ctx,
        )
        .await
        .unwrap();

        let err = tool
            .execute(
                json!({
                    "action": "wait_for_text",
                    "session": "timeoutdemo",
                    "pattern": "__definitely_not_present__",
                    "timeout": 1,
                    "poll_interval_ms": 100
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "wait_timeout");

        tool.execute(json!({ "action": "kill_all" }), &ctx)
            .await
            .unwrap();
        let _ = fs::remove_dir_all(socket_dir).await;
    }

    #[tokio::test]
    async fn observation_budget_exhaustion_returns_stop_signal_error() {
        if !tmux_available() {
            return;
        }

        let socket_dir = unique_test_dir("observe-budget");
        let mut ctx = test_ctx(&socket_dir);
        ctx.metadata.insert(
            "terminal_multiplexer.max_auto_observe_steps".to_string(),
            Value::Number(serde_json::Number::from(1)),
        );
        let tool = TerminalMultiplexerTool::new();

        tool.execute(
            json!({
                "action": "start_or_resume",
                "session": "budgetdemo",
                "initial_command": "printf 'budget-ready\\n'"
            }),
            &ctx,
        )
        .await
        .unwrap();

        let err = tool
            .execute(
                json!({
                    "action": "wait_for_text",
                    "session": "budgetdemo",
                    "pattern": "budget-ready",
                    "timeout": 5
                }),
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code(), "observation_budget_exhausted");
        assert!(err.signals().iter().any(|signal| signal.kind == "stop"));

        tool.execute(json!({ "action": "kill_all" }), &ctx)
            .await
            .unwrap();
        let _ = fs::remove_dir_all(socket_dir).await;
    }
}
