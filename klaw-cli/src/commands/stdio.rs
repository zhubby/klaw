use clap::Args;
use klaw_runtime::{build_hosted_runtime, shutdown_runtime_bundle};
use std::sync::Arc;

use super::startup_display::print_startup_banner;
use crate::commands::signal::shutdown_signal;
use klaw_channel::{Channel, stdio::StdioChannel};
use klaw_config::AppConfig;

#[derive(Debug, Args)]
pub struct StdioCommand {
    /// Session key used for local conversation. Auto-generated as `stdio:<uuid>` when omitted.
    #[arg(long)]
    pub session_key: Option<String>,
    /// Print model reasoning when provider returns it.
    #[arg(long, default_value_t = false)]
    pub show_reasoning: bool,
    /// Print tracing logs directly in the terminal instead of writing them to ~/.klaw/logs/stdio.log.
    #[arg(long, default_value_t = false)]
    pub verbose_terminal: bool,
}

impl StdioCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let hosted = build_hosted_runtime(config.as_ref()).await?;
        print_startup_banner(config.as_ref(), &hosted.startup_report);

        let mut channel = StdioChannel::new(self.session_key, self.show_reasoning);
        let run_result = tokio::select! {
            result = channel.run(hosted.adapter.as_ref()) => result,
            _ = shutdown_signal() => {
                println!("\nShutdown signal received. Bye.");
                Ok(())
            }
        };
        let shutdown_result = tokio::select! {
            result = shutdown_runtime_bundle(hosted.runtime.as_ref()) => result,
            _ = shutdown_signal() => {
                std::process::exit(130);
            }
        };
        run_result?;
        shutdown_result
    }
}
