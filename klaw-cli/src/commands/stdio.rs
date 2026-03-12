use clap::Args;
use std::{sync::Arc, time::Duration};

use crate::runtime::service_loop::{BackgroundServiceConfig, BackgroundServices};
use crate::runtime::{build_runtime_bundle, submit_and_get_output, RuntimeBundle};
use klaw_channel::{stdio::StdioChannel, Channel};
use klaw_channel::{ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime};
use klaw_config::AppConfig;

#[derive(Debug, Args)]
pub struct StdioCommand {
    /// Session key used for local conversation. Auto-generated as `stdio:<uuid>` when omitted.
    #[arg(long)]
    pub session_key: Option<String>,
    /// Print model reasoning when provider returns it.
    #[arg(long, default_value_t = false)]
    pub show_reasoning: bool,
}

impl StdioCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let runtime = build_runtime_bundle(config.as_ref()).await?;
        let background = BackgroundServices::new(
            &runtime,
            BackgroundServiceConfig::from_app_config(config.as_ref()),
        );
        let adapter = CliChannelRuntime {
            runtime,
            background,
        };

        let mut channel = StdioChannel::new(self.session_key, self.show_reasoning);
        channel.run(&adapter).await
    }
}

struct CliChannelRuntime {
    runtime: RuntimeBundle,
    background: BackgroundServices,
}

#[async_trait::async_trait(?Send)]
impl ChannelRuntime for CliChannelRuntime {
    async fn submit(&self, request: ChannelRequest) -> ChannelResult<Option<ChannelResponse>> {
        let maybe_output = submit_and_get_output(
            &self.runtime,
            request.input,
            request.session_key,
            request.chat_id,
        )
        .await?;

        Ok(maybe_output.map(|output| ChannelResponse {
            content: output.content,
            reasoning: output.reasoning,
        }))
    }

    fn cron_tick_interval(&self) -> Duration {
        self.background.cron_tick_interval()
    }

    fn runtime_tick_interval(&self) -> Duration {
        self.background.runtime_tick_interval()
    }

    async fn on_cron_tick(&self) {
        self.background.on_cron_tick().await;
    }

    async fn on_runtime_tick(&self) {
        self.background.on_runtime_tick(&self.runtime).await;
    }
}
