mod commands;
mod env_check;
mod runtime;

use clap::{Parser, Subcommand, ValueEnum};
use commands::{
    agent::AgentCommand, archive::ArchiveCommand, config::ConfigCommand, daemon::DaemonCommand,
    gateway::GatewayCommand, gui::GuiCommand, session::SessionCommand, stdio::StdioCommand,
};
use klaw_storage::StoragePaths;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
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
    command: Option<Commands>,
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
    let command = command.unwrap_or(Commands::Gui(GuiCommand {}));
    let gui_path_update = initialize_gui_process_environment(&command);
    let gui_log_sender = create_gui_log_sender_for_command(&command);
    init_tracing(&command, log_level, gui_log_sender)?;
    if let Some(update) = &gui_path_update {
        info!(
            added_paths = ?update.added_paths,
            "augmented PATH for macOS GUI launch"
        );
    }
    if is_pre_runtime_command(&command) {
        match command {
            Commands::Config(cmd) => cmd.run(config.as_deref())?,
            Commands::Daemon(cmd) => cmd.run(config.as_deref())?,
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
        Commands::Gateway(cmd) => cmd.run(Arc::clone(&app_config)).await?,
        Commands::Gui(cmd) => cmd.run(app_config).await?,
        Commands::Session(cmd) => cmd.run().await?,
        Commands::Archive(cmd) => cmd.run().await?,
        Commands::Config(_) => unreachable!("handled above"),
        Commands::Daemon(_) => unreachable!("handled above"),
    }

    Ok(())
}

fn is_pre_runtime_command(command: &Commands) -> bool {
    matches!(command, Commands::Config(_) | Commands::Daemon(_))
}

#[derive(Debug)]
struct PathUpdate {
    added_paths: Vec<PathBuf>,
}

fn initialize_gui_process_environment(command: &Commands) -> Option<PathUpdate> {
    if !matches!(command, Commands::Gui(_)) {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        return augment_path_for_macos_gui();
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn augment_path_for_macos_gui() -> Option<PathUpdate> {
    const MACOS_GUI_PATHS: &[&str] = &[
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/opt/local/bin",
        "/opt/local/sbin",
    ];

    let current_path = env::var_os("PATH");
    let installed_candidates = MACOS_GUI_PATHS
        .iter()
        .map(PathBuf::from)
        .filter(|candidate| candidate.exists())
        .collect::<Vec<_>>();
    let (merged_paths, added_paths) =
        compute_augmented_path_entries(current_path, installed_candidates);

    if added_paths.is_empty() {
        return None;
    }

    if let Ok(joined) = env::join_paths(&merged_paths) {
        env::set_var("PATH", joined);
        return Some(PathUpdate { added_paths });
    }

    None
}

fn compute_augmented_path_entries(
    current_path: Option<std::ffi::OsString>,
    installed_candidates: Vec<PathBuf>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut merged_paths: Vec<PathBuf> = current_path
        .as_deref()
        .map(env::split_paths)
        .into_iter()
        .flatten()
        .collect();
    let mut added_paths = Vec::new();

    for candidate in installed_candidates.into_iter().rev() {
        if merged_paths.iter().any(|existing| existing == &candidate) {
            continue;
        }
        merged_paths.insert(0, candidate.clone());
        added_paths.push(candidate);
    }

    added_paths.reverse();
    (merged_paths, added_paths)
}

fn init_tracing(
    command: &Commands,
    log_level: Option<LogLevel>,
    gui_log_sender: Option<mpsc::SyncSender<String>>,
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
            let writer = FanoutTracingWriter::new(
                PrimaryTracingWriter::File(FileTracingWriter::new(log_file)),
                gui_log_sender,
            );
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .compact()
                .with_ansi(false)
                .with_writer(move || writer.clone())
                .init();
        }
        Commands::Gui(_) => {
            let writer = FanoutTracingWriter::new(PrimaryTracingWriter::Stdout, gui_log_sender);
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_target(false)
                .compact()
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

fn create_gui_log_sender_for_command(command: &Commands) -> Option<mpsc::SyncSender<String>> {
    if !matches!(command, Commands::Gui(_)) {
        return None;
    }
    let (sender, receiver) = mpsc::sync_channel(2048);
    klaw_gui::install_log_receiver(receiver);
    Some(sender)
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

#[derive(Debug, Clone)]
enum PrimaryTracingWriter {
    Stdout,
    File(FileTracingWriter),
}

impl Write for PrimaryTracingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout => {
                let mut stdout = io::stdout();
                stdout.write(buf)
            }
            Self::File(file_writer) => file_writer.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout => {
                let mut stdout = io::stdout();
                stdout.flush()
            }
            Self::File(file_writer) => file_writer.flush(),
        }
    }
}

#[derive(Debug, Clone)]
struct GuiTracingWriter {
    sender: mpsc::SyncSender<String>,
}

impl GuiTracingWriter {
    fn new(sender: mpsc::SyncSender<String>) -> Self {
        Self { sender }
    }
}

impl Write for GuiTracingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // GUI sink must never block or fail the primary logging path.
        let payload = String::from_utf8_lossy(buf).to_string();
        let _ = self.sender.try_send(payload);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct FanoutTracingWriter {
    primary: PrimaryTracingWriter,
    gui: Option<GuiTracingWriter>,
}

impl FanoutTracingWriter {
    fn new(primary: PrimaryTracingWriter, gui_sender: Option<mpsc::SyncSender<String>>) -> Self {
        Self {
            primary,
            gui: gui_sender.map(GuiTracingWriter::new),
        }
    }
}

impl Write for FanoutTracingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.primary.write(buf)?;
        if let Some(gui_writer) = self.gui.as_mut() {
            let _ = gui_writer.write(buf);
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.primary.flush()?;
        if let Some(gui_writer) = self.gui.as_mut() {
            let _ = gui_writer.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::{
        fs::OpenOptions,
        sync::mpsc,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parse_global_log_level_before_subcommand() {
        let cli = Cli::parse_from(["klaw", "--log-level", "debug", "stdio"]);
        assert_eq!(cli.log_level, Some(LogLevel::Debug));
        assert!(matches!(cli.command, Some(Commands::Stdio(_))));
    }

    #[test]
    fn parse_global_log_level_after_subcommand() {
        let cli = Cli::parse_from(["klaw", "stdio", "--log-level", "trace"]);
        assert_eq!(cli.log_level, Some(LogLevel::Trace));
        assert!(matches!(cli.command, Some(Commands::Stdio(_))));
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
        assert!(matches!(cli.command, Some(Commands::Gui(_))));
    }

    #[test]
    fn gui_is_pre_runtime_command() {
        let cli = Cli::parse_from(["klaw", "gui"]);
        let command = cli.command.as_ref().expect("command should be present");
        assert!(!is_pre_runtime_command(command));
    }

    #[test]
    fn gui_tracing_writer_disconnected_sink_is_non_fatal() {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let mut writer = GuiTracingWriter::new(sender);
        let written = writer.write(b"hello").expect("write should not fail");
        assert_eq!(written, 5);
    }

    #[test]
    fn fanout_writer_with_file_primary_survives_gui_sink_drop() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("klaw-fanout-{suffix}.log"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("create temp log file");
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let mut writer = FanoutTracingWriter::new(
            PrimaryTracingWriter::File(FileTracingWriter::new(file)),
            Some(sender),
        );

        let written = writer
            .write(b"fanout-check")
            .expect("fanout write should succeed");
        assert_eq!(written, 12);
        writer.flush().expect("flush should succeed");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn compute_augmented_path_entries_prepends_missing_candidates_once() {
        let existing = env::join_paths([
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
        .expect("join test PATH");
        let candidates = vec![
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/opt/local/bin"),
        ];

        let (merged_paths, added_paths) =
            compute_augmented_path_entries(Some(existing), candidates);

        assert_eq!(
            merged_paths,
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/opt/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/usr/local/bin"),
            ]
        );
        assert_eq!(
            added_paths,
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/opt/local/bin"),
            ]
        );
    }

    #[test]
    fn compute_augmented_path_entries_handles_empty_path() {
        let candidates = vec![PathBuf::from("/opt/homebrew/bin")];

        let (merged_paths, added_paths) = compute_augmented_path_entries(None, candidates);

        assert_eq!(merged_paths, vec![PathBuf::from("/opt/homebrew/bin")]);
        assert_eq!(added_paths, vec![PathBuf::from("/opt/homebrew/bin")]);
    }
}
