use crate::runtime::SharedChannelRuntime;
use klaw_channel::dingtalk::{DingtalkChannel, DingtalkChannelConfig, DingtalkProxyConfig};
use klaw_config::DingtalkConfig;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle, time};
use tracing::{info, warn};

const CHANNEL_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub fn spawn_enabled_channels(
    configs: Vec<DingtalkConfig>,
    adapter: Arc<SharedChannelRuntime>,
    shutdown_rx: watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    let mut handles = Vec::new();
    for channel_config in configs.into_iter().filter(|cfg| cfg.enabled) {
        let adapter = Arc::clone(&adapter);
        let mut channel_shutdown = shutdown_rx.clone();
        let handle = tokio::task::spawn_local(async move {
            let account_id = channel_config.id.clone();
            let channel_config = DingtalkChannelConfig {
                account_id: channel_config.id,
                client_id: channel_config.client_id,
                client_secret: channel_config.client_secret,
                bot_title: channel_config.bot_title,
                show_reasoning: channel_config.show_reasoning,
                allowlist: channel_config.allowlist,
                proxy: DingtalkProxyConfig {
                    enabled: channel_config.proxy.enabled,
                    url: channel_config.proxy.url,
                },
            };
            let mut channel = match DingtalkChannel::new(channel_config) {
                Ok(channel) => channel,
                Err(err) => {
                    warn!(
                        account_id = account_id.as_str(),
                        error = %err,
                        "failed to initialize dingtalk channel"
                    );
                    return;
                }
            };
            info!(
                account_id = account_id.as_str(),
                "starting dingtalk channel"
            );
            if let Err(err) = channel
                .run_until_shutdown(adapter.as_ref(), &mut channel_shutdown)
                .await
            {
                warn!(
                    account_id = account_id.as_str(),
                    error = %err,
                    "dingtalk channel stopped"
                );
            }
        });
        handles.push(handle);
    }
    handles
}

pub async fn wait_for_channels_shutdown(handles: &mut Vec<JoinHandle<()>>) {
    for handle in handles.drain(..) {
        if let Err(err) = time::timeout(CHANNEL_SHUTDOWN_TIMEOUT, handle).await {
            warn!(error = %err, "timed out waiting dingtalk channel to stop");
        }
    }
}
