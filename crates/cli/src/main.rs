mod commands;

use clap::{Parser, Subcommand};
use commands::{once::OnceCommand, stdio::StdioCommand};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "klaw-cli", about = "Klaw command line interface")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Start local stdin/stdout interactive agent loop.
    Stdio(StdioCommand),
    /// Execute one request and print one response.
    Once(OnceCommand),
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Stdio(cmd) => cmd.run().await?,
        Commands::Once(cmd) => cmd.run().await?,
    }

    Ok(())
}
