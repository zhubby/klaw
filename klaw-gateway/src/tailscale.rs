use klaw_config::TailscaleMode;
use klaw_util::command_search_path;
use std::{
    io,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use tracing::{info, warn};

const TAILSCALE_PROBE_TIMEOUT: Duration = Duration::from_millis(400);
const TAILSCALE_PROBE_POLL_INTERVAL: Duration = Duration::from_millis(25);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailscaleStatus {
    Disconnected,
    Connected,
    Error(String),
}

impl Default for TailscaleStatus {
    fn default() -> Self {
        Self::Disconnected
    }
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

    pub fn check_prerequisites() -> Result<(), TailscaleError> {
        let output = tailscale_command()
            .arg("version")
            .output()
            .map_err(|_| TailscaleError::CliNotFound)?;

        if !output.status.success() {
            return Err(TailscaleError::CliNotFound);
        }

        let status_output = tailscale_command()
            .args(["status", "--json"])
            .output()
            .map_err(|e| TailscaleError::StatusFailed(e.to_string()))?;

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
    pub fn inspect_host() -> TailscaleHostInfo {
        let mut version_command = tailscale_command();
        version_command.arg("version");
        let version_output =
            match run_command_with_timeout(&mut version_command, TAILSCALE_PROBE_TIMEOUT) {
                Ok(output) if output.status.success() => output,
                Ok(_) | Err(CommandProbeError::Io(_)) => {
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

        let mut status_command = tailscale_command();
        status_command.args(["status", "--json"]);
        let status_output =
            match run_command_with_timeout(&mut status_command, TAILSCALE_PROBE_TIMEOUT) {
                Ok(output) => output,
                Err(CommandProbeError::Io(err)) => {
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
                    return TailscaleHostInfo {
                        status: TailscaleStatus::Disconnected,
                        version,
                        message: Some(
                            "Tailscale is installed but the local daemon is unavailable."
                                .to_string(),
                        ),
                        ..TailscaleHostInfo::default()
                    };
                }
            };

        if !status_output.status.success() {
            return TailscaleHostInfo {
                status: TailscaleStatus::Disconnected,
                version,
                message: Some("Tailscale is installed but not logged in.".to_string()),
                ..TailscaleHostInfo::default()
            };
        }

        let status: serde_json::Value = match serde_json::from_slice(&status_output.stdout) {
            Ok(status) => status,
            Err(err) => {
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

    pub fn setup(&self) -> Result<TailscaleRuntimeInfo, TailscaleError> {
        if self.mode == TailscaleMode::Off {
            return Ok(TailscaleRuntimeInfo {
                mode: TailscaleMode::Off,
                status: TailscaleStatus::Disconnected,
                public_url: None,
                message: None,
            });
        }

        Self::check_prerequisites()?;

        let backend = format!("127.0.0.1:{}", self.port);
        let result = match self.mode {
            TailscaleMode::Funnel => self.run_funnel(&backend),
            TailscaleMode::Serve => self.run_serve(&backend),
            TailscaleMode::Off => unreachable!(),
        };

        result?;
        self.verify_active_config()?;

        let public_url = self.get_public_url()?;

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

    fn run_funnel(&self, backend: &str) -> Result<(), TailscaleError> {
        let output = tailscale_command()
            .args(["funnel", "--bg", backend])
            .output()
            .map_err(|e| TailscaleError::SetupFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = command_error_output(&output);
            if stderr.contains("HTTPS") || stderr.contains("https") {
                return Err(TailscaleError::HttpsNotEnabled);
            }
            return Err(TailscaleError::SetupFailed(stderr));
        }

        Ok(())
    }

    fn run_serve(&self, backend: &str) -> Result<(), TailscaleError> {
        let output = tailscale_command()
            .args(["serve", "--bg", backend])
            .output()
            .map_err(|e| TailscaleError::SetupFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(TailscaleError::SetupFailed(command_error_output(&output)));
        }

        Ok(())
    }

    fn verify_active_config(&self) -> Result<(), TailscaleError> {
        let subcommand = match self.mode {
            TailscaleMode::Funnel => "funnel",
            TailscaleMode::Serve => "serve",
            TailscaleMode::Off => return Ok(()),
        };
        let output = tailscale_command()
            .args([subcommand, "status", "--json"])
            .output()
            .map_err(|e| TailscaleError::StatusFailed(e.to_string()))?;

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

    fn get_public_url(&self) -> Result<String, TailscaleError> {
        let output = tailscale_command()
            .args(["status", "--json"])
            .output()
            .map_err(|e| TailscaleError::StatusFailed(e.to_string()))?;

        let status: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| TailscaleError::StatusFailed(e.to_string()))?;

        let dns_name = status["Self"]["DNSName"]
            .as_str()
            .ok_or_else(|| TailscaleError::StatusFailed("DNSName not found".to_string()))?;

        let dns_name = dns_name.trim_end_matches('.');

        Ok(format!("https://{}/", dns_name))
    }

    pub fn teardown(&self) {
        if !self.reset_on_exit || self.mode == TailscaleMode::Off {
            return;
        }

        let result = match self.mode {
            TailscaleMode::Funnel => self.reset_funnel(),
            TailscaleMode::Serve => self.reset_serve(),
            TailscaleMode::Off => return,
        };

        if let Err(e) = result {
            warn!(error = %e, "failed to reset tailscale serve");
        } else {
            info!("tailscale serve reset");
        }
    }

    fn reset_funnel(&self) -> Result<(), TailscaleError> {
        let output = tailscale_command()
            .args(["funnel", "reset"])
            .output()
            .map_err(|e| TailscaleError::ResetFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(TailscaleError::ResetFailed(command_error_output(&output)));
        }

        Ok(())
    }

    fn reset_serve(&self) -> Result<(), TailscaleError> {
        let output = tailscale_command()
            .args(["serve", "reset"])
            .output()
            .map_err(|e| TailscaleError::ResetFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(TailscaleError::ResetFailed(command_error_output(&output)));
        }

        Ok(())
    }
}

impl Drop for TailscaleManager {
    fn drop(&mut self) {
        self.teardown();
    }
}

fn tailscale_command() -> Command {
    let mut command = Command::new("tailscale");
    if let Some(path) = command_search_path() {
        command.env("PATH", path);
    }
    command
}

fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<Output, CommandProbeError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(CommandProbeError::Io)?;
    let deadline = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(CommandProbeError::Io),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandProbeError::TimedOut);
                }
                thread::sleep(TAILSCALE_PROBE_POLL_INTERVAL);
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CommandProbeError::Io(err));
            }
        }
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
        CommandProbeError, has_active_funnel_config, has_active_serve_config,
        run_command_with_timeout,
    };
    use serde_json::json;
    use std::{process::Command, time::Duration};

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

    #[cfg(unix)]
    #[test]
    fn probe_timeout_aborts_slow_command() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 1"]);

        let result = run_command_with_timeout(&mut command, Duration::from_millis(50));

        assert!(matches!(result, Err(CommandProbeError::TimedOut)));
    }
}
