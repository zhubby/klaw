use clap::Args;
use klaw_channel::terminal::{TERMINAL_CHANNEL_NAME, resolve_session_key};
use klaw_runtime::{build_hosted_runtime, shutdown_runtime_bundle};
use klaw_tui::{TuiMeta, run_tui};
use std::sync::Arc;

use super::startup_display::build_startup_summary;
use crate::commands::signal::shutdown_signal;
use klaw_config::AppConfig;

#[derive(Debug, Args)]
pub struct TuiCommand {
    /// Session key used for local conversation. Auto-generated as `terminal:<uuid>` when omitted.
    #[arg(long)]
    pub session_key: Option<String>,
    /// Print model reasoning when provider returns it.
    #[arg(long, default_value_t = false)]
    pub show_reasoning: bool,
    /// Print tracing logs directly in the terminal instead of writing them to ~/.klaw/logs/terminal.log.
    #[arg(long, default_value_t = false)]
    pub verbose_terminal: bool,
}

impl TuiCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let hosted = build_hosted_runtime(config.as_ref()).await?;
        let summary = build_startup_summary(config.as_ref(), &hosted.startup_report);
        let provider_runtime = hosted.runtime.runtime.provider_runtime_snapshot();
        let session_key = resolve_session_key(self.session_key);
        let meta = TuiMeta {
            version: summary.version,
            session_key,
            channel: TERMINAL_CHANNEL_NAME.to_string(),
            provider: provider_runtime.default_provider_id,
            model: provider_runtime.default_model,
            skills: summary.skills,
            tools: summary.tools,
            mcp: summary.mcp,
            show_reasoning: self.show_reasoning,
        };
        let run_result = tokio::select! {
            result = run_tui(meta, hosted.adapter.as_ref()) => result,
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
