use clap::Args;
use std::sync::Arc;

use super::startup_display::print_startup_banner;
use crate::commands::signal::shutdown_signal;
use crate::runtime::service_loop::{BackgroundServiceConfig, BackgroundServices};
use crate::runtime::{
    build_runtime_bundle, finalize_startup_report, shutdown_runtime_bundle, SharedChannelRuntime,
};
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
    /// Print tracing logs directly in the terminal instead of writing them to ~/.klaw/logs/stdio.log.
    #[arg(long, default_value_t = false)]
    pub verbose_terminal: bool,
}

impl StdioCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let mut runtime = build_runtime_bundle(config.as_ref()).await?;
        let startup_report = finalize_startup_report(&mut runtime).await?;
        print_startup_banner(config.as_ref(), &startup_report);

        let runtime = Arc::new(runtime);
        let background = Arc::new(BackgroundServices::new(
            runtime.as_ref(),
            BackgroundServiceConfig::from_app_config(config.as_ref()),
        ));
        let adapter = SharedChannelRuntime::new(runtime.clone(), background);

        let mut channel = StdioChannel::new(self.session_key, self.show_reasoning);
        let run_result = tokio::select! {
            result = channel.run(&adapter) => result,
            _ = shutdown_signal() => {
                println!("\nShutdown signal received. Bye.");
                Ok(())
            }
        };
        let shutdown_result = tokio::select! {
            result = shutdown_runtime_bundle(runtime.as_ref()) => result,
            _ = shutdown_signal() => {
                std::process::exit(130);
            }
        };
        run_result?;
        shutdown_result
    }
}
