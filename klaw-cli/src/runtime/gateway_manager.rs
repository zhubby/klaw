use super::{RuntimeBundle, webhook};
use klaw_config::{AppConfig, ConfigStore, TailscaleMode};
use klaw_gateway::{GatewayHandle, spawn_gateway_with_options};
use klaw_gui::GatewayStatusSnapshot;
use std::sync::Arc;
use tracing::warn;

pub struct GatewayManager {
    runtime: Arc<RuntimeBundle>,
    handle: Option<GatewayHandle>,
    configured_enabled: bool,
    transitioning: bool,
    last_error: Option<String>,
    tailscale_mode: TailscaleMode,
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
            last_error: self.last_error.clone(),
            auth_configured: self.auth_configured,
            tailscale_mode: self.tailscale_mode,
        }
    }

    pub async fn start_from_config(
        &mut self,
        config: &AppConfig,
    ) -> Result<GatewayStatusSnapshot, String> {
        self.configured_enabled = config.gateway.enabled;
        self.tailscale_mode = config.gateway.tailscale.mode;
        self.auth_configured = config.gateway.auth.is_enabled();

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
        self.configured_enabled = config.gateway.enabled;
        if !config.gateway.enabled {
            return Ok(self.snapshot());
        }
        self.start_from_config(config).await
    }

    pub async fn restart_from_store(&mut self) -> Result<GatewayStatusSnapshot, String> {
        let config = load_config_from_store()?;
        self.configured_enabled = config.gateway.enabled;
        self.tailscale_mode = config.gateway.tailscale.mode;
        self.auth_configured = config.gateway.auth.is_enabled();

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

        let config = save_tailscale_mode(mode)?;
        self.tailscale_mode = mode;
        self.auth_configured = config.gateway.auth.is_enabled();

        if self.handle.is_some() {
            if let Err(err) = self.stop().await {
                warn!(error = %err, "failed to stop gateway before tailscale mode change");
            }
            if config.gateway.enabled {
                self.start_from_config(&config).await
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
    let store = ConfigStore::open(None).map_err(|err| err.to_string())?;
    let mut next = store.snapshot().config;
    next.gateway.enabled = enabled;
    let raw = toml::to_string_pretty(&next).map_err(|err| err.to_string())?;
    let saved = store.save_raw_toml(&raw).map_err(|err| err.to_string())?;
    Ok(saved.config)
}

fn save_tailscale_mode(mode: TailscaleMode) -> Result<AppConfig, String> {
    let store = ConfigStore::open(None).map_err(|err| err.to_string())?;
    let mut next = store.snapshot().config;
    next.gateway.tailscale.mode = mode;
    let raw = toml::to_string_pretty(&next).map_err(|err| err.to_string())?;
    let saved = store.save_raw_toml(&raw).map_err(|err| err.to_string())?;
    Ok(saved.config)
}
