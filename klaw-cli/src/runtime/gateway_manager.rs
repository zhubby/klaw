use super::{RuntimeBundle, webhook};
use klaw_config::{AppConfig, ConfigError, ConfigStore, TailscaleMode};
use klaw_gateway::{GatewayHandle, TailscaleManager, spawn_gateway_with_options};
use klaw_gui::GatewayStatusSnapshot;
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

pub struct GatewayManager {
    runtime: Arc<RuntimeBundle>,
    handle: Option<GatewayHandle>,
    configured_enabled: bool,
    transitioning: bool,
    last_error: Option<String>,
    tailscale_mode: TailscaleMode,
    tailscale_host: klaw_gateway::TailscaleHostInfo,
    auth_configured: bool,
}

impl GatewayManager {
    pub fn new(config: &AppConfig, runtime: Arc<RuntimeBundle>) -> Self {
        Self {
            runtime,
            handle: None,
            configured_enabled: config.gateway.enabled,
            transitioning: false,
            last_error: None,
            tailscale_mode: config.gateway.tailscale.mode,
            tailscale_host: TailscaleManager::inspect_host(),
            auth_configured: config.gateway.auth.is_enabled(),
        }
    }

    pub fn snapshot(&self) -> GatewayStatusSnapshot {
        let info = self.handle.as_ref().map(|handle| handle.info().clone());
        GatewayStatusSnapshot {
            configured_enabled: self.configured_enabled,
            running: self.handle.is_some(),
            transitioning: self.transitioning,
            info,
            tailscale_host: self.tailscale_host.clone(),
            last_error: self.last_error.clone(),
            auth_configured: self.auth_configured,
            tailscale_mode: self.tailscale_mode,
        }
    }

    fn sync_metadata_from_config(&mut self, config: &AppConfig) {
        self.configured_enabled = config.gateway.enabled;
        self.tailscale_mode = config.gateway.tailscale.mode;
        self.auth_configured = config.gateway.auth.is_enabled();
        self.tailscale_host = TailscaleManager::inspect_host();
    }

    pub async fn start_from_config(
        &mut self,
        config: &AppConfig,
    ) -> Result<GatewayStatusSnapshot, String> {
        self.sync_metadata_from_config(config);

        if self.handle.is_some() {
            return Ok(self.snapshot());
        }

        self.transitioning = true;
        let start_result = spawn_gateway_with_options(
            &config.gateway,
            webhook::gateway_options(Arc::clone(&self.runtime)),
        )
        .await;
        self.transitioning = false;

        match start_result {
            Ok(handle) => {
                self.handle = Some(handle);
                self.last_error = None;
                Ok(self.snapshot())
            }
            Err(err) => {
                let message = err.to_string();
                self.last_error = Some(message.clone());
                Err(message)
            }
        }
    }

    pub async fn stop(&mut self) -> Result<GatewayStatusSnapshot, String> {
        self.transitioning = true;
        let stop_result = match self.handle.take() {
            Some(handle) => handle.shutdown().await.map_err(|err| err.to_string()),
            None => Ok(()),
        };
        self.transitioning = false;

        match stop_result {
            Ok(()) => {
                self.last_error = None;
                Ok(self.snapshot())
            }
            Err(err) => {
                self.last_error = Some(err.clone());
                Err(err)
            }
        }
    }

    pub async fn start_if_enabled(
        &mut self,
        config: &AppConfig,
    ) -> Result<GatewayStatusSnapshot, String> {
        self.sync_metadata_from_config(config);
        if !config.gateway.enabled {
            return Ok(self.snapshot());
        }
        self.start_from_config(config).await
    }

    pub fn refresh_from_store(&mut self) -> Result<GatewayStatusSnapshot, String> {
        let config = load_config_from_store()?;
        self.sync_metadata_from_config(&config);
        Ok(self.snapshot())
    }

    pub async fn start_from_store(&mut self) -> Result<GatewayStatusSnapshot, String> {
        let config = load_config_from_store()?;
        self.sync_metadata_from_config(&config);
        if !config.gateway.enabled {
            let message = "gateway is disabled in config".to_string();
            self.last_error = Some(message.clone());
            return Err(message);
        }
        self.start_from_config(&config).await
    }

    pub async fn restart_from_store(&mut self) -> Result<GatewayStatusSnapshot, String> {
        let config = load_config_from_store()?;
        self.sync_metadata_from_config(&config);

        if !config.gateway.enabled {
            let message = "gateway is disabled in config".to_string();
            self.last_error = Some(message.clone());
            return Err(message);
        }

        if let Err(err) = self.stop().await {
            warn!(error = %err, "failed to stop gateway before restart");
        }

        self.start_from_config(&config).await
    }

    pub async fn set_enabled(&mut self, enabled: bool) -> Result<GatewayStatusSnapshot, String> {
        if enabled {
            let config = save_gateway_enabled(true)?;
            self.start_from_config(&config).await
        } else {
            self.stop().await?;
            match save_gateway_enabled(false) {
                Ok(config) => {
                    self.configured_enabled = config.gateway.enabled;
                    self.last_error = None;
                    Ok(self.snapshot())
                }
                Err(err) => {
                    self.last_error = Some(err.clone());
                    Err(err)
                }
            }
        }
    }

    pub async fn set_tailscale_mode(
        &mut self,
        mode: TailscaleMode,
    ) -> Result<GatewayStatusSnapshot, String> {
        if mode == TailscaleMode::Funnel && !self.auth_configured {
            let message =
                "funnel mode requires authentication. Configure gateway.auth first.".to_string();
            self.last_error = Some(message.clone());
            return Err(message);
        }

        let previous_mode = self.tailscale_mode;
        let config = save_tailscale_mode(mode)?;
        self.tailscale_mode = mode;
        self.auth_configured = config.gateway.auth.is_enabled();

        if self.handle.is_some() {
            if let Err(err) = self.stop().await {
                warn!(error = %err, "failed to stop gateway before tailscale mode change");
            }
            if config.gateway.enabled {
                match self.start_from_config(&config).await {
                    Ok(status) => Ok(status),
                    Err(err) => {
                        if let Err(revert_err) = save_tailscale_mode(previous_mode) {
                            warn!(
                                error = %revert_err,
                                previous_mode = ?previous_mode,
                                "failed to revert tailscale mode after start failure"
                            );
                        }
                        self.tailscale_mode = previous_mode;
                        self.last_error = Some(err.clone());
                        Err(err)
                    }
                }
            } else {
                Ok(self.snapshot())
            }
        } else {
            Ok(self.snapshot())
        }
    }
}

fn load_config_from_store() -> Result<AppConfig, String> {
    ConfigStore::open(None)
        .map_err(|err| err.to_string())
        .map(|store| store.snapshot().config)
}

fn save_gateway_enabled(enabled: bool) -> Result<AppConfig, String> {
    update_gateway_config(None, |config| {
        config.gateway.enabled = enabled;
        Ok(())
    })
}

fn save_tailscale_mode(mode: TailscaleMode) -> Result<AppConfig, String> {
    update_gateway_config(None, |config| {
        config.gateway.tailscale.mode = mode;
        Ok(())
    })
}

fn update_gateway_config<F>(config_path: Option<&Path>, mutate: F) -> Result<AppConfig, String>
where
    F: FnOnce(&mut AppConfig) -> Result<(), String>,
{
    let store = ConfigStore::open(config_path).map_err(|err| err.to_string())?;
    store
        .update_config(|config| {
            mutate(config).map_err(ConfigError::InvalidConfig)?;
            Ok(())
        })
        .map(|(snapshot, ())| snapshot.config)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::update_gateway_config;
    use klaw_config::{ConfigStore, TailscaleMode};
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path() -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        env::temp_dir()
            .join(format!("klaw-gateway-manager-test-{suffix}"))
            .join("config.toml")
    }

    fn write_gateway_config(path: &std::path::Path) {
        let root = path.parent().expect("temp config should have a parent");
        fs::create_dir_all(root).expect("should create temp root");
        fs::write(
            path,
            r#"
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"

[gateway]
enabled = false
listen_ip = "127.0.0.1"
listen_port = 0

[gateway.auth]
enabled = true
env_key = "KLAW_GATEWAY_TOKEN"

[gateway.tailscale]
mode = "off"
reset_on_exit = false
"#,
        )
        .expect("should write source config");
    }

    #[test]
    fn gateway_helper_updates_enabled_without_clobbering_other_gateway_fields() {
        let path = temp_config_path();
        let root = path
            .parent()
            .expect("temp config path should have root directory")
            .to_path_buf();
        write_gateway_config(&path);

        let stale_store = ConfigStore::open(Some(&path)).expect("stale store should open");
        stale_store
            .update_config(|config| {
                config.gateway.auth.enabled = false;
                config.gateway.auth.env_key = Some("UPDATED_GATEWAY_TOKEN".to_string());
                Ok(())
            })
            .expect("stale store update should succeed");

        let saved = update_gateway_config(Some(&path), |config| {
            config.gateway.enabled = true;
            Ok(())
        })
        .expect("gateway enabled update should succeed");

        assert!(saved.gateway.enabled);
        assert!(!saved.gateway.auth.enabled);
        assert_eq!(
            saved.gateway.auth.env_key.as_deref(),
            Some("UPDATED_GATEWAY_TOKEN")
        );

        let disk_raw = fs::read_to_string(&path).expect("saved config should be readable");
        assert!(disk_raw.contains("enabled = true"));
        assert!(disk_raw.contains("env_key = \"UPDATED_GATEWAY_TOKEN\""));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gateway_helper_updates_tailscale_mode_without_clobbering_auth_changes() {
        let path = temp_config_path();
        let root = path
            .parent()
            .expect("temp config path should have root directory")
            .to_path_buf();
        write_gateway_config(&path);

        let stale_store = ConfigStore::open(Some(&path)).expect("stale store should open");
        stale_store
            .update_config(|config| {
                config.gateway.auth.token = Some("secret-token".to_string());
                Ok(())
            })
            .expect("stale store update should succeed");

        let saved = update_gateway_config(Some(&path), |config| {
            config.gateway.tailscale.mode = TailscaleMode::Serve;
            Ok(())
        })
        .expect("tailscale mode update should succeed");

        assert_eq!(saved.gateway.tailscale.mode, TailscaleMode::Serve);
        assert_eq!(saved.gateway.auth.token.as_deref(), Some("secret-token"));

        let disk_raw = fs::read_to_string(&path).expect("saved config should be readable");
        assert!(disk_raw.contains("mode = \"serve\""));
        assert!(disk_raw.contains("token = \"secret-token\""));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&root);
    }
}
