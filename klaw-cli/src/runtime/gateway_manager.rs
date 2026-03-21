use klaw_config::{AppConfig, ConfigStore};
use klaw_gateway::{spawn_gateway, GatewayHandle};
use klaw_gui::GatewayStatusSnapshot;
use tracing::warn;

pub struct GatewayManager {
    handle: Option<GatewayHandle>,
    configured_enabled: bool,
    transitioning: bool,
    last_error: Option<String>,
}

impl GatewayManager {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            handle: None,
            configured_enabled: config.gateway.enabled,
            transitioning: false,
            last_error: None,
        }
    }

    pub fn snapshot(&self) -> GatewayStatusSnapshot {
        GatewayStatusSnapshot {
            configured_enabled: self.configured_enabled,
            running: self.handle.is_some(),
            transitioning: self.transitioning,
            info: self.handle.as_ref().map(|handle| handle.info().clone()),
            last_error: self.last_error.clone(),
        }
    }

    pub async fn start_from_config(
        &mut self,
        config: &AppConfig,
    ) -> Result<GatewayStatusSnapshot, String> {
        self.configured_enabled = config.gateway.enabled;
        if self.handle.is_some() {
            return Ok(self.snapshot());
        }

        self.transitioning = true;
        let start_result = spawn_gateway(&config.gateway).await;
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

    pub async fn set_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<GatewayStatusSnapshot, String> {
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
