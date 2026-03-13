use clap::Args;
use std::sync::Arc;

use crate::runtime::service_loop::{BackgroundServiceConfig, BackgroundServices};
use crate::runtime::{build_runtime_bundle, shutdown_runtime_bundle, SharedChannelRuntime};
use klaw_channel::{stdio::StdioChannel, Channel};
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
        let runtime = Arc::new(build_runtime_bundle(config.as_ref()).await?);
        let background = Arc::new(BackgroundServices::new(
            runtime.as_ref(),
            BackgroundServiceConfig::from_app_config(config.as_ref()),
        ));
        let adapter = SharedChannelRuntime::new(runtime.clone(), background);

        let mut channel = StdioChannel::new(self.session_key, self.show_reasoning);
        let run_result = channel.run(&adapter).await;
        let shutdown_result = shutdown_runtime_bundle(runtime.as_ref()).await;
        run_result?;
        shutdown_result
    }
}
