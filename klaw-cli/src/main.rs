mod commands;
mod runtime;

use clap::{Parser, Subcommand, ValueEnum};
use commands::{
    agent::AgentCommand, archive::ArchiveCommand, config::ConfigCommand, daemon::DaemonCommand,
    gateway::GatewayCommand, gui::GuiCommand, session::SessionCommand, stdio::StdioCommand,
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

    /// Set tracing log level globally. Supported: trace, debug, info, warn, error.
    #[arg(long, global = true, value_enum)]
    log_level: Option<LogLevel>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    const fn as_filter(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
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
    /// Start klaw desktop workbench GUI.
    Gui(GuiCommand),
    /// Manage local session indexes in klaw.db.
    Session(SessionCommand),
    /// Manage archived media files in archive.db and archives/.
    Archive(ArchiveCommand),
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Cli {
        config,
        log_level,
        command,
    } = Cli::parse();
    init_tracing(&command, log_level)?;
    if is_pre_runtime_command(&command) {
        match command {
            Commands::Config(cmd) => cmd.run(config.as_deref())?,
            Commands::Daemon(cmd) => cmd.run(config.as_deref())?,
            Commands::Gui(cmd) => cmd.run()?,
            _ => unreachable!("pre-runtime guard must keep this branch exhaustive"),
        }
        return Ok(());
    }

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
        Commands::Gui(_) => unreachable!("handled above"),
        Commands::Session(cmd) => cmd.run().await?,
        Commands::Archive(cmd) => cmd.run().await?,
        Commands::Config(_) => unreachable!("handled above"),
        Commands::Daemon(_) => unreachable!("handled above"),
    }

    Ok(())
}

fn is_pre_runtime_command(command: &Commands) -> bool {
    matches!(
        command,
        Commands::Config(_) | Commands::Daemon(_) | Commands::Gui(_)
    )
}

fn init_tracing(
    command: &Commands,
    log_level: Option<LogLevel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = build_env_filter(log_level);

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

fn build_env_filter(log_level: Option<LogLevel>) -> EnvFilter {
    match log_level {
        Some(level) => {
            // Keep app-level debug/trace while suppressing noisy DB internals.
            let filter = format!(
                "{},sqlx=warn,sqlx::query=warn,sqlx::query::logger=warn,\
                turso=warn,turso_core=warn,turso_ext=warn,turso_sync_engine=warn,\
                turso_parser=warn,turso_sdk_kit=warn,turso_sync_sdk_kit=warn",
                level.as_filter()
            );
            EnvFilter::new(filter)
        }
        None => EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parse_global_log_level_before_subcommand() {
        let cli = Cli::parse_from(["klaw", "--log-level", "debug", "stdio"]);
        assert_eq!(cli.log_level, Some(LogLevel::Debug));
        assert!(matches!(cli.command, Commands::Stdio(_)));
    }

    #[test]
    fn parse_global_log_level_after_subcommand() {
        let cli = Cli::parse_from(["klaw", "stdio", "--log-level", "trace"]);
        assert_eq!(cli.log_level, Some(LogLevel::Trace));
        assert!(matches!(cli.command, Commands::Stdio(_)));
    }

    #[test]
    fn build_env_filter_includes_sqlx_suppression_when_log_level_is_set() {
        let filter = build_env_filter(Some(LogLevel::Debug));
        let rendered = filter.to_string();
        assert!(rendered.contains("debug"));
        assert!(rendered.contains("sqlx=warn"));
        assert!(rendered.contains("turso=warn"));
    }

    #[test]
    fn parse_gui_subcommand() {
        let cli = Cli::parse_from(["klaw", "gui"]);
        assert!(matches!(cli.command, Commands::Gui(_)));
    }

    #[test]
    fn gui_is_pre_runtime_command() {
        let cli = Cli::parse_from(["klaw", "gui"]);
        assert!(is_pre_runtime_command(&cli.command));
    }
}
