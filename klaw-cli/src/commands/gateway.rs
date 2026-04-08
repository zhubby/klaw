use clap::Args;
use klaw_channel::{ChannelConfigSnapshot, ChannelManager};
use klaw_config::AppConfig;
use klaw_gateway::run_gateway_with_options;
use klaw_runtime::{
    build_channel_driver_factory, build_hosted_runtime, shutdown_runtime_bundle, webhook,
};
use std::{io, sync::Arc};
use tokio::sync::watch;

use super::startup_display::print_startup_banner;
use crate::commands::signal::shutdown_signal;
use tracing::info;

#[derive(Debug, Args)]
pub struct GatewayCommand {}

impl GatewayCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let hosted = build_hosted_runtime(config.as_ref()).await?;
        print_startup_banner(config.as_ref(), &hosted.startup_report);
        let channel_snapshot = ChannelConfigSnapshot::from_channels_config(&config.channels)
            .map_err(io::Error::other)?;
        let gateway_config = config.gateway.clone();

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                let (shutdown_tx, _shutdown_rx) = watch::channel(false);
                let channel_factory = build_channel_driver_factory(config.as_ref())?;
                let mut channel_manager =
                    ChannelManager::with_factory(Arc::clone(&hosted.adapter), channel_factory);
                channel_manager.sync(channel_snapshot).await;
                let gateway_options = webhook::gateway_options(Arc::clone(&hosted.runtime));

                let mut gateway_task = tokio::task::spawn_local(async move {
                    run_gateway_with_options(&gateway_config, gateway_options).await
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
                let shutdown_result = shutdown_runtime_bundle(hosted.runtime.as_ref()).await;
                run_result?;
                shutdown_result
            })
            .await?;
        Ok(())
    }
}
