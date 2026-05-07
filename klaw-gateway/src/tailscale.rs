use klaw_config::TailscaleMode;
use klaw_util::command_search_path;
use std::{
    io,
    process::{Output, Stdio},
    time::Duration,
};
use thiserror::Error;
use tokio::process::Command;
use tracing::{debug, info, warn};

const TAILSCALE_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const TAILSCALE_SETUP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Error)]
pub enum TailscaleError {
    #[error("tailscale CLI not found")]
    CliNotFound,
    #[error("tailscale not logged in")]
    NotLoggedIn,
    #[error("tailscale setup failed: {0}")]
    SetupFailed(String),
    #[error("tailscale reset failed: {0}")]
    ResetFailed(String),
    #[error("failed to get tailscale status: {0}")]
    StatusFailed(String),
    #[error("HTTPS not enabled for tailnet")]
    HttpsNotEnabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TailscaleStatus {
    #[default]
    Disconnected,
    Connected,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TailscaleHostInfo {
    pub status: TailscaleStatus,
    pub backend_state: Option<String>,
    pub dns_name: Option<String>,
    pub public_url: Option<String>,
    pub version: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleRuntimeInfo {
    pub mode: TailscaleMode,
    pub status: TailscaleStatus,
    pub public_url: Option<String>,
    pub message: Option<String>,
}

pub struct TailscaleManager {
    mode: TailscaleMode,
    port: u16,
    reset_on_exit: bool,
}

#[derive(Debug)]
enum CommandProbeError {
    Io(io::Error),
    TimedOut,
}

impl TailscaleManager {
    pub fn new(mode: TailscaleMode, port: u16, reset_on_exit: bool) -> Self {
        Self {
            mode,
            port,
            reset_on_exit,
        }
    }

    pub async fn check_prerequisites() -> Result<(), TailscaleError> {
        let output = run_tailscale_command_with_timeout(&["version"], TAILSCALE_SETUP_TIMEOUT)
            .await
            .map_err(prerequisite_version_error)?;

        if !output.status.success() {
            return Err(TailscaleError::CliNotFound);
        }

        let status_output =
            run_tailscale_command_with_timeout(&["status", "--json"], TAILSCALE_SETUP_TIMEOUT)
                .await
                .map_err(status_command_error)?;

        if !status_output.status.success() {
            return Err(TailscaleError::NotLoggedIn);
        }

        let status: serde_json::Value = serde_json::from_slice(&status_output.stdout)
            .map_err(|e| TailscaleError::StatusFailed(e.to_string()))?;

        if status["BackendState"].as_str() != Some("Running") {
            return Err(TailscaleError::NotLoggedIn);
        }

        Ok(())
    }

    #[must_use]
    pub async fn inspect_host() -> TailscaleHostInfo {
        debug!("probing tailscale host via `tailscale version`");
        let version_output =
            match run_tailscale_command_with_timeout(&["version"], TAILSCALE_PROBE_TIMEOUT).await {
                Ok(output) if output.status.success() => {
                    debug!(
                        exit_code = output.status.code(),
                        stdout = summarize_command_stream(&output.stdout),
                        stderr = summarize_command_stream(&output.stderr),
                        "`tailscale version` probe succeeded"
                    );
                    output
                }
                Ok(output) => {
                    debug!(
                        exit_code = output.status.code(),
                        stdout = summarize_command_stream(&output.stdout),
                        stderr = summarize_command_stream(&output.stderr),
                        "`tailscale version` probe failed"
                    );
                    return TailscaleHostInfo {
                        status: TailscaleStatus::Error("tailscale CLI not found".to_string()),
                        message: Some(
                            "Install Tailscale and ensure the `tailscale` CLI is on PATH."
                                .to_string(),
                        ),
                        ..TailscaleHostInfo::default()
                    };
                }
                Err(CommandProbeError::Io(err)) => {
                    debug!(error = %err, "`tailscale version` probe failed to spawn");
                    return TailscaleHostInfo {
                        status: TailscaleStatus::Error("tailscale CLI not found".to_string()),
                        message: Some(
                            "Install Tailscale and ensure the `tailscale` CLI is on PATH."
                                .to_string(),
                        ),
                        ..TailscaleHostInfo::default()
                    };
                }
                Err(CommandProbeError::TimedOut) => {
                    debug!("`tailscale version` probe timed out");
                    return TailscaleHostInfo {
                        status: TailscaleStatus::Error(
                            "tailscale version command timed out".to_string(),
                        ),
                        message: Some(
                            "Timed out while checking the local Tailscale CLI.".to_string(),
                        ),
                        ..TailscaleHostInfo::default()
                    };
                }
            };

        let version = String::from_utf8_lossy(&version_output.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);

        debug!("probing tailscale host via `tailscale status --json`");
        let status_output = match run_tailscale_command_with_timeout(
            &["status", "--json"],
            TAILSCALE_PROBE_TIMEOUT,
        )
        .await
        {
            Ok(output) => {
                debug!(
                    exit_code = output.status.code(),
                    stdout = summarize_command_stream(&output.stdout),
                    stderr = summarize_command_stream(&output.stderr),
                    "`tailscale status --json` probe completed"
                );
                output
            }
            Err(CommandProbeError::Io(err)) => {
                debug!(error = %err, "`tailscale status --json` probe failed to spawn");
                return TailscaleHostInfo {
                    status: TailscaleStatus::Error(format!(
                        "failed to get tailscale status: {err}"
                    )),
                    version,
                    message: Some("Unable to query local Tailscale status.".to_string()),
                    ..TailscaleHostInfo::default()
                };
            }
            Err(CommandProbeError::TimedOut) => {
                debug!("`tailscale status --json` probe timed out");
                return TailscaleHostInfo {
                    status: TailscaleStatus::Error(
                        "tailscale status command timed out".to_string(),
                    ),
                    version,
                    message: Some(
                        "Timed out while querying the local Tailscale daemon.".to_string(),
                    ),
                    ..TailscaleHostInfo::default()
                };
            }
        };

        let parsed_status = serde_json::from_slice::<serde_json::Value>(&status_output.stdout);
        if !status_output.status.success() && parsed_status.is_err() {
            let stderr = command_error_output(&status_output);
            let disconnected = looks_like_not_logged_in(&stderr);
            debug!(
                disconnected,
                stderr,
                "`tailscale status --json` reported a non-success exit status without a parseable json payload"
            );
            return TailscaleHostInfo {
                status: if disconnected {
                    TailscaleStatus::Disconnected
                } else {
                    TailscaleStatus::Error(format!("failed to get tailscale status: {stderr}"))
                },
                version,
                message: Some(if disconnected {
                    "Tailscale is installed but not logged in.".to_string()
                } else {
                    stderr
                }),
                ..TailscaleHostInfo::default()
            };
        }

        if !status_output.status.success() {
            debug!(
                exit_code = status_output.status.code(),
                stderr = summarize_command_stream(&status_output.stderr),
                "`tailscale status --json` returned a non-success exit status but produced a parseable json payload; continuing with parsed status"
            );
        }

        let status: serde_json::Value = match parsed_status {
            Ok(status) => {
                debug!("parsed `tailscale status --json` probe payload");
                status
            }
            Err(err) => {
                debug!(error = %err, "`tailscale status --json` payload parse failed");
                return TailscaleHostInfo {
                    status: TailscaleStatus::Error(format!(
                        "failed to parse tailscale status: {err}"
                    )),
                    version,
                    message: Some("Tailscale returned an unreadable status payload.".to_string()),
                    ..TailscaleHostInfo::default()
                };
            }
        };

        let backend_state = status["BackendState"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let dns_name = status["Self"]["DNSName"]
            .as_str()
            .map(str::trim)
            .map(|value| value.trim_end_matches('.'))
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let public_url = dns_name
            .as_ref()
            .map(|dns_name| format!("https://{dns_name}/"));

        let status_kind = match backend_state.as_deref() {
            Some("Running") => TailscaleStatus::Connected,
            Some(_) => TailscaleStatus::Disconnected,
            None => TailscaleStatus::Error("tailscale status missing BackendState".to_string()),
        };
        let message = match &status_kind {
            TailscaleStatus::Connected => {
                Some("Tailscale is connected on this machine.".to_string())
            }
            TailscaleStatus::Disconnected => Some(match backend_state.as_deref() {
                Some(other) => format!("Tailscale backend state: {other}"),
                None => "Tailscale is not connected.".to_string(),
            }),
            TailscaleStatus::Error(err) => Some(err.clone()),
        };

        TailscaleHostInfo {
            status: status_kind,
            backend_state,
            dns_name,
            public_url,
            version,
            message,
        }
    }

    pub async fn setup(&self) -> Result<TailscaleRuntimeInfo, TailscaleError> {
        if self.mode == TailscaleMode::Off {
            return Ok(TailscaleRuntimeInfo {
                mode: TailscaleMode::Off,
                status: TailscaleStatus::Disconnected,
                public_url: None,
                message: None,
            });
        }

        Self::check_prerequisites().await?;

        let backend = format!("127.0.0.1:{}", self.port);
        let result = match self.mode {
            TailscaleMode::Funnel => self.run_funnel(&backend).await,
            TailscaleMode::Serve => self.run_serve(&backend).await,
            TailscaleMode::Off => unreachable!(),
        };

        result?;
        self.verify_active_config().await?;

        let public_url = self.get_public_url().await?;

        info!(
            mode = ?self.mode,
            public_url = %public_url,
            "tailscale configured"
        );

        Ok(TailscaleRuntimeInfo {
            mode: self.mode,
            status: TailscaleStatus::Connected,
            public_url: Some(public_url.clone()),
            message: Some(format!("Exposed at {}", public_url)),
        })
    }

    async fn run_funnel(&self, backend: &str) -> Result<(), TailscaleError> {
        let output = run_tailscale_command_with_timeout(
            &["funnel", "--bg", backend],
            TAILSCALE_SETUP_TIMEOUT,
        )
        .await
        .map_err(setup_command_error)?;

        if !output.status.success() {
            let stderr = command_error_output(&output);
            if stderr.contains("HTTPS") || stderr.contains("https") {
                return Err(TailscaleError::HttpsNotEnabled);
            }
            return Err(TailscaleError::SetupFailed(stderr));
        }

        Ok(())
    }

    async fn run_serve(&self, backend: &str) -> Result<(), TailscaleError> {
        let output = run_tailscale_command_with_timeout(
            &["serve", "--bg", backend],
            TAILSCALE_SETUP_TIMEOUT,
        )
        .await
        .map_err(setup_command_error)?;

        if !output.status.success() {
            return Err(TailscaleError::SetupFailed(command_error_output(&output)));
        }

        Ok(())
    }

    async fn verify_active_config(&self) -> Result<(), TailscaleError> {
        let subcommand = match self.mode {
            TailscaleMode::Funnel => "funnel",
            TailscaleMode::Serve => "serve",
            TailscaleMode::Off => return Ok(()),
        };
        let output = run_tailscale_command_with_timeout(
            &[subcommand, "status", "--json"],
            TAILSCALE_SETUP_TIMEOUT,
        )
        .await
        .map_err(status_command_error)?;

        if !output.status.success() {
            return Err(TailscaleError::StatusFailed(command_error_output(&output)));
        }

        let status: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| TailscaleError::StatusFailed(e.to_string()))?;
        let is_active = match self.mode {
            TailscaleMode::Funnel => has_active_funnel_config(&status),
            TailscaleMode::Serve => has_active_serve_config(&status),
            TailscaleMode::Off => false,
        };

        if is_active {
            Ok(())
        } else {
            Err(TailscaleError::SetupFailed(match self.mode {
                TailscaleMode::Funnel => {
                    "tailscale funnel did not report an active funnel configuration".to_string()
                }
                TailscaleMode::Serve => {
                    "tailscale serve did not report an active serve configuration".to_string()
                }
                TailscaleMode::Off => unreachable!(),
            }))
        }
    }

    async fn get_public_url(&self) -> Result<String, TailscaleError> {
        let output =
            run_tailscale_command_with_timeout(&["status", "--json"], TAILSCALE_SETUP_TIMEOUT)
                .await
                .map_err(status_command_error)?;

        let status: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| TailscaleError::StatusFailed(e.to_string()))?;

        let dns_name = status["Self"]["DNSName"]
            .as_str()
            .ok_or_else(|| TailscaleError::StatusFailed("DNSName not found".to_string()))?;

        let dns_name = dns_name.trim_end_matches('.');

        Ok(format!("https://{}/", dns_name))
    }

    pub async fn teardown(&self) {
        if !self.reset_on_exit || self.mode == TailscaleMode::Off {
            return;
        }

        let result = match self.mode {
            TailscaleMode::Funnel => self.reset_funnel().await,
            TailscaleMode::Serve => self.reset_serve().await,
            TailscaleMode::Off => return,
        };

        if let Err(e) = result {
            warn!(error = %e, "failed to reset tailscale serve");
        } else {
            info!("tailscale serve reset");
        }
    }

    async fn reset_funnel(&self) -> Result<(), TailscaleError> {
        let output =
            run_tailscale_command_with_timeout(&["funnel", "reset"], TAILSCALE_SETUP_TIMEOUT)
                .await
                .map_err(reset_command_error)?;

        if !output.status.success() {
            return Err(TailscaleError::ResetFailed(command_error_output(&output)));
        }

        Ok(())
    }

    async fn reset_serve(&self) -> Result<(), TailscaleError> {
        let output =
            run_tailscale_command_with_timeout(&["serve", "reset"], TAILSCALE_SETUP_TIMEOUT)
                .await
                .map_err(reset_command_error)?;

        if !output.status.success() {
            return Err(TailscaleError::ResetFailed(command_error_output(&output)));
        }

        Ok(())
    }
}

fn tailscale_command() -> Command {
    let mut command = Command::new("tailscale");
    if let Some(path) = command_search_path() {
        command.env("PATH", path);
    }
    command
}

async fn run_tailscale_command_with_timeout(
    args: &[&str],
    timeout: Duration,
) -> Result<Output, CommandProbeError> {
    let mut command = tailscale_command();
    command.args(args);
    run_command_with_timeout(&mut command, timeout).await
}

fn prerequisite_version_error(err: CommandProbeError) -> TailscaleError {
    match err {
        CommandProbeError::Io(_) => TailscaleError::CliNotFound,
        CommandProbeError::TimedOut => {
            TailscaleError::SetupFailed("tailscale version command timed out".to_string())
        }
    }
}

fn status_command_error(err: CommandProbeError) -> TailscaleError {
    match err {
        CommandProbeError::Io(err) => TailscaleError::StatusFailed(err.to_string()),
        CommandProbeError::TimedOut => {
            TailscaleError::StatusFailed("tailscale status command timed out".to_string())
        }
    }
}

fn setup_command_error(err: CommandProbeError) -> TailscaleError {
    match err {
        CommandProbeError::Io(err) => TailscaleError::SetupFailed(err.to_string()),
        CommandProbeError::TimedOut => {
            TailscaleError::SetupFailed("tailscale setup command timed out".to_string())
        }
    }
}

fn reset_command_error(err: CommandProbeError) -> TailscaleError {
    match err {
        CommandProbeError::Io(err) => TailscaleError::ResetFailed(err.to_string()),
        CommandProbeError::TimedOut => {
            TailscaleError::ResetFailed("tailscale reset command timed out".to_string())
        }
    }
}

async fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<Output, CommandProbeError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match tokio::time::timeout(timeout, command.output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(err)) => Err(CommandProbeError::Io(err)),
        Err(_) => Err(CommandProbeError::TimedOut),
    }
}

fn command_error_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    "command failed without error output".to_string()
}

fn summarize_command_stream(stream: &[u8]) -> String {
    const MAX_LEN: usize = 200;
    let text = String::from_utf8_lossy(stream).trim().replace('\n', "\\n");
    if text.len() <= MAX_LEN {
        return text;
    }
    format!("{}...", &text[..MAX_LEN])
}

fn looks_like_not_logged_in(stderr: &str) -> bool {
    let normalized = stderr.trim().to_ascii_lowercase();
    normalized.contains("logged out")
        || normalized.contains("not logged in")
        || normalized.contains("needs login")
        || normalized.contains("state: needslogin")
}

fn has_active_funnel_config(status: &serde_json::Value) -> bool {
    if let Some(allow_funnel) = status.get("AllowFunnel") {
        return has_truthy_entries(allow_funnel);
    }
    has_active_serve_config(status)
}

fn has_active_serve_config(status: &serde_json::Value) -> bool {
    status
        .as_object()
        .map(|map| {
            ["TCP", "Web", "Services", "Foreground"]
                .iter()
                .filter_map(|key| map.get(*key))
                .any(has_non_empty_value)
        })
        .unwrap_or(false)
}

fn has_truthy_entries(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Object(map) => map.values().any(has_truthy_entries),
        serde_json::Value::Array(items) => items.iter().any(has_truthy_entries),
        serde_json::Value::Number(number) => number.as_u64().is_some_and(|value| value > 0),
        serde_json::Value::String(value) => !value.trim().is_empty(),
        serde_json::Value::Null => false,
    }
}

fn has_non_empty_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => !map.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Null => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandProbeError, TailscaleError, TailscaleStatus, has_active_funnel_config,
        has_active_serve_config, looks_like_not_logged_in, run_command_with_timeout,
        setup_command_error,
    };
    use serde_json::json;
    use std::{process::Output, time::Duration};
    use tokio::process::Command;

    #[test]
    fn detects_active_funnel_from_allow_funnel_flag() {
        let status = json!({
            "AllowFunnel": {
                "443": true
            },
            "Web": {
                "example.ts.net:443": {
                    "/": {
                        "Proxy": "http://127.0.0.1:18080"
                    }
                }
            }
        });

        assert!(has_active_funnel_config(&status));
    }

    #[test]
    fn detects_active_serve_from_web_config() {
        let status = json!({
            "Web": {
                "example.ts.net:443": {
                    "/": {
                        "Proxy": "http://127.0.0.1:18080"
                    }
                }
            }
        });

        assert!(has_active_serve_config(&status));
        assert!(has_active_funnel_config(&status));
    }

    #[test]
    fn empty_status_is_not_active() {
        let status = json!({});

        assert!(!has_active_serve_config(&status));
        assert!(!has_active_funnel_config(&status));
    }

    #[test]
    fn login_detector_matches_common_cli_messages() {
        assert!(looks_like_not_logged_in("backend state: NeedsLogin"));
        assert!(looks_like_not_logged_in("not logged in"));
        assert!(!looks_like_not_logged_in(
            "failed to connect to local Tailscaled process"
        ));
    }

    #[test]
    fn non_login_status_failure_should_not_be_disconnected() {
        let stderr = "failed to connect to local Tailscaled process and failed to enumerate processes while looking for it";
        let output = Output {
            status: exit_status(1),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        };

        let info = host_info_from_status_failure(&output);

        assert!(matches!(info.status, TailscaleStatus::Error(_)));
        assert_eq!(info.message.as_deref(), Some(stderr));
    }

    #[test]
    fn login_status_failure_remains_disconnected() {
        let output = Output {
            status: exit_status(1),
            stdout: Vec::new(),
            stderr: b"backend state: NeedsLogin".to_vec(),
        };

        let info = host_info_from_status_failure(&output);

        assert_eq!(info.status, TailscaleStatus::Disconnected);
        assert_eq!(
            info.message.as_deref(),
            Some("Tailscale is installed but not logged in.")
        );
    }

    #[test]
    fn parseable_running_json_should_win_even_when_exit_status_is_non_zero() {
        let output = Output {
            status: exit_status(1),
            stdout: br#"{"BackendState":"Running","Self":{"DNSName":"demo.ts.net."}}"#.to_vec(),
            stderr: br#"Warning: client version "1.94.1" != tailscaled server version "1.96.2""#
                .to_vec(),
        };

        let parsed_status = serde_json::from_slice::<serde_json::Value>(&output.stdout);
        let status = parsed_status.expect("json payload should parse");
        let backend_state = status["BackendState"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);

        assert_eq!(backend_state.as_deref(), Some("Running"));
        assert!(!output.status.success());
    }

    #[cfg(unix)]
    fn exit_status(code: i32) -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(code << 8)
    }

    fn host_info_from_status_failure(output: &Output) -> super::TailscaleHostInfo {
        let stderr = super::command_error_output(output);
        let disconnected = looks_like_not_logged_in(&stderr);
        super::TailscaleHostInfo {
            status: if disconnected {
                TailscaleStatus::Disconnected
            } else {
                TailscaleStatus::Error(format!("failed to get tailscale status: {stderr}"))
            },
            version: Some("1.94.1".to_string()),
            message: Some(if disconnected {
                "Tailscale is installed but not logged in.".to_string()
            } else {
                stderr
            }),
            ..Default::default()
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probe_timeout_aborts_slow_command() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 1"]);

        let result = run_command_with_timeout(&mut command, Duration::from_millis(50)).await;

        assert!(matches!(result, Err(CommandProbeError::TimedOut)));
    }

    #[test]
    fn setup_timeout_is_reported_as_setup_failure() {
        let err = setup_command_error(CommandProbeError::TimedOut);

        assert!(
            matches!(err, TailscaleError::SetupFailed(message) if message.contains("timed out"))
        );
    }
}
