use klaw_config::TailscaleMode;
use std::process::Command;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Clone, Error)]
pub enum TailscaleError {
    #[error("tailscale CLI not found")]
    CliNotFound,
    #[error("tailscale not logged in")]
    NotLoggedIn,
    #[error("tailscale serve setup failed: {0}")]
    SetupFailed(String),
    #[error("tailscale serve reset failed: {0}")]
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

impl TailscaleManager {
    pub fn new(mode: TailscaleMode, port: u16, reset_on_exit: bool) -> Self {
        Self {
            mode,
            port,
            reset_on_exit,
        }
    }

    pub fn check_prerequisites() -> Result<(), TailscaleError> {
        let output = Command::new("tailscale")
            .arg("version")
            .output()
            .map_err(|_| TailscaleError::CliNotFound)?;

        if !output.status.success() {
            return Err(TailscaleError::CliNotFound);
        }

        let status_output = Command::new("tailscale")
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
        let version_output = match Command::new("tailscale").arg("version").output() {
            Ok(output) if output.status.success() => output,
            Ok(_) | Err(_) => {
                return TailscaleHostInfo {
                    status: TailscaleStatus::Error("tailscale CLI not found".to_string()),
                    message: Some(
                        "Install Tailscale and ensure the `tailscale` CLI is on PATH.".to_string(),
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

        let status_output = match Command::new("tailscale")
            .args(["status", "--json"])
            .output()
        {
            Ok(output) => output,
            Err(err) => {
                return TailscaleHostInfo {
                    status: TailscaleStatus::Error(format!(
                        "failed to get tailscale status: {err}"
                    )),
                    version,
                    message: Some("Unable to query local Tailscale status.".to_string()),
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
        let output = Command::new("tailscale")
            .args(["funnel", "443", "--bg", backend])
            .output()
            .map_err(|e| TailscaleError::SetupFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("HTTPS") || stderr.contains("https") {
                return Err(TailscaleError::HttpsNotEnabled);
            }
            return Err(TailscaleError::SetupFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn run_serve(&self, backend: &str) -> Result<(), TailscaleError> {
        let output = Command::new("tailscale")
            .args(["serve", "--bg", backend])
            .output()
            .map_err(|e| TailscaleError::SetupFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TailscaleError::SetupFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn get_public_url(&self) -> Result<String, TailscaleError> {
        let output = Command::new("tailscale")
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
        let output = Command::new("tailscale")
            .args(["funnel", "--reset"])
            .output()
            .map_err(|e| TailscaleError::ResetFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TailscaleError::ResetFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn reset_serve(&self) -> Result<(), TailscaleError> {
        let output = Command::new("tailscale")
            .args(["serve", "--reset"])
            .output()
            .map_err(|e| TailscaleError::ResetFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TailscaleError::ResetFailed(stderr.to_string()));
        }

        Ok(())
    }
}

impl Drop for TailscaleManager {
    fn drop(&mut self) {
        self.teardown();
    }
}
