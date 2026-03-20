use clap::Args;
use klaw_channel::{ChannelConfigSnapshot, ChannelManager};
use klaw_config::AppConfig;
use std::{io, sync::Arc};
use tokio::sync::watch;

use super::startup_display::print_startup_banner;
use crate::commands::signal::shutdown_signal;
use crate::runtime::service_loop::{BackgroundServiceConfig, BackgroundServices};
use crate::runtime::{
    build_runtime_bundle, finalize_startup_report, shutdown_runtime_bundle, SharedChannelRuntime,
};
use tracing::info;

#[derive(Debug, Args)]
pub struct GatewayCommand {}

impl GatewayCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let mut runtime = build_runtime_bundle(config.as_ref()).await?;
        let startup_report = finalize_startup_report(&mut runtime).await?;
        print_startup_banner(config.as_ref(), &startup_report);

        let runtime = Arc::new(runtime);
        let background = Arc::new(BackgroundServices::new(
            runtime.as_ref(),
            BackgroundServiceConfig::from_app_config(config.as_ref()),
        ));
        let adapter = Arc::new(SharedChannelRuntime::new(runtime.clone(), background));
        let channel_snapshot = ChannelConfigSnapshot::from_channels_config(&config.channels)
            .map_err(io::Error::other)?;
        let gateway_config = config.gateway.clone();

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                let (shutdown_tx, _shutdown_rx) = watch::channel(false);
                let mut channel_manager = ChannelManager::new(Arc::clone(&adapter));
                channel_manager.sync(channel_snapshot).await;

                let mut gateway_task = tokio::task::spawn_local(async move {
                    klaw_gateway::run_gateway(&gateway_config).await
                });
                let run_result = tokio::select! {
                    result = &mut gateway_task => {
                        match result {
                            Ok(result) => result.map_err(Box::<dyn std::error::Error>::from),
                            Err(err) => Err(Box::<dyn std::error::Error>::from(err)),
                        }
                    }
                    _ = shutdown_signal() => {
                        info!("shutdown signal received, stopping gateway");
                        let _ = shutdown_tx.send(true);
                        gateway_task.abort();
                        let _ = gateway_task.await;
                        Ok(())
                    }
                };

                let _ = shutdown_tx.send(true);
                channel_manager.shutdown_all().await;
                let shutdown_result = shutdown_runtime_bundle(runtime.as_ref()).await;
                run_result?;
                shutdown_result
            })
            .await?;
        Ok(())
    }
}
