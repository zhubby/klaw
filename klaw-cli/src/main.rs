mod commands;
mod runtime;

use clap::{Parser, Subcommand};
use commands::{
    agent::AgentCommand, config::ConfigCommand, daemon::DaemonCommand, gateway::GatewayCommand,
    session::SessionCommand, stdio::StdioCommand,
};
use klaw_storage::StoragePaths;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};
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
    init_tracing(&command)?;
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

fn init_tracing(command: &Commands) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    match command {
        Commands::Stdio(cmd) if cmd.verbose_terminal => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .compact()
                .init();
        }
        Commands::Stdio(_) => {
            let storage_paths = StoragePaths::from_home_dir()?;
            let log_dir = storage_paths.root_dir.join("logs");
            fs::create_dir_all(&log_dir)?;
            let log_path = log_dir.join("stdio.log");
            let log_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)?;
            let writer = FileTracingWriter::new(log_file);
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .compact()
                .with_ansi(false)
                .with_writer(move || writer.clone())
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

    Ok(())
}

#[derive(Debug, Clone)]
struct FileTracingWriter {
    file: Arc<Mutex<std::fs::File>>,
}

impl FileTracingWriter {
    fn new(file: std::fs::File) -> Self {
        Self {
            file: Arc::new(Mutex::new(file)),
        }
    }
}

impl Write for FileTracingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut file = self.file.lock().unwrap_or_else(|err| err.into_inner());
        file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut file = self.file.lock().unwrap_or_else(|err| err.into_inner());
        file.flush()
    }
}
