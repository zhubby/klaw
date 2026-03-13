use clap::Args;
use klaw_channel::{
    dingtalk::{DingtalkChannel, DingtalkChannelConfig},
    Channel,
};
use klaw_config::AppConfig;
use std::sync::Arc;

use crate::runtime::service_loop::{BackgroundServiceConfig, BackgroundServices};
use crate::runtime::{build_runtime_bundle, SharedChannelRuntime};
use tracing::{info, warn};

#[derive(Debug, Args)]
pub struct GatewayCommand {}

impl GatewayCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = Arc::new(build_runtime_bundle(config.as_ref()).await?);
        let background = Arc::new(BackgroundServices::new(
            runtime.as_ref(),
            BackgroundServiceConfig::from_app_config(config.as_ref()),
        ));
        let adapter = Arc::new(SharedChannelRuntime::new(runtime, background));
        let dingtalk_configs = config.channels.dingtalk.clone();
        let gateway_config = config.gateway.clone();

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                for channel_config in dingtalk_configs.into_iter().filter(|cfg| cfg.enabled) {
                    let adapter = Arc::clone(&adapter);
                    tokio::task::spawn_local(async move {
                        let account_id = channel_config.id.clone();
                        let mut channel = DingtalkChannel::new(DingtalkChannelConfig {
                            account_id: channel_config.id,
                            client_id: channel_config.client_id,
                            client_secret: channel_config.client_secret,
                            bot_title: channel_config.bot_title,
                            show_reasoning: channel_config.show_reasoning,
                            allowlist: channel_config.allowlist,
                        });
                        info!(
                            account_id = account_id.as_str(),
                            "starting dingtalk channel"
                        );
                        if let Err(err) = channel.run(adapter.as_ref()).await {
                            warn!(
                                account_id = account_id.as_str(),
                                error = %err,
                                "dingtalk channel stopped"
                            );
                        }
                    });
                }

                klaw_gateway::run_gateway(&gateway_config).await
            })
            .await?;
        Ok(())
    }
}
