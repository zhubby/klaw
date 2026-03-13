mod commands;
mod runtime;

use clap::{Parser, Subcommand};
use commands::{
    agent::AgentCommand, config::ConfigCommand, daemon::DaemonCommand, gateway::GatewayCommand,
    session::SessionCommand, stdio::StdioCommand,
};
use std::{path::PathBuf, sync::Arc};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "klaw", about = "Klaw command line interface")]
struct Cli {
    /// Path to config file (TOML). Defaults to ~/.klaw/config.toml.
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Manage config files.
    Config(ConfigCommand),
    /// Manage the user-level gateway daemon.
    Daemon(DaemonCommand),
    /// Start local stdin/stdout interactive agent loop.
    Stdio(StdioCommand),
    /// Execute one request and print one response.
    Agent(AgentCommand),
    /// Start websocket gateway service.
    Gateway(GatewayCommand),
    /// Manage local session indexes in klaw.db.
    Session(SessionCommand),
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Cli { config, command } = Cli::parse();
    init_tracing(&command);
    let command = match command {
        Commands::Config(cmd) => {
            cmd.run(config.as_deref())?;
            return Ok(());
        }
        Commands::Daemon(cmd) => {
            cmd.run(config.as_deref())?;
            return Ok(());
        }
        other => other,
    };

    let loaded = klaw_config::load_or_init(config.as_deref())?;
    if loaded.created_default {
        info!(
            config_path = %loaded.path.display(),
            "default config file created"
        );
    }
    let app_config = Arc::new(loaded.config);

    match command {
        Commands::Stdio(cmd) => cmd.run(Arc::clone(&app_config)).await?,
        Commands::Agent(cmd) => cmd.run(app_config).await?,
        Commands::Gateway(cmd) => cmd.run(app_config).await?,
        Commands::Session(cmd) => cmd.run().await?,
        Commands::Config(_) => unreachable!("handled above"),
        Commands::Daemon(_) => unreachable!("handled above"),
    }

    Ok(())
}

fn init_tracing(command: &Commands) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    match command {
        Commands::Stdio(_) => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .compact()
                .with_writer(klaw_channel::stdio::make_tracing_writer)
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .compact()
                .init();
        }
    }
}
