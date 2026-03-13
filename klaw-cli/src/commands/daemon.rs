use clap::{Args, Subcommand};
use klaw_config::default_config_path;
use klaw_storage::StoragePaths;
use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;

const SERVICE_NAME: &str = "gateway";
#[cfg(any(target_os = "linux", test))]
const SYSTEMD_UNIT_NAME: &str = "klaw-gateway.service";
#[cfg(any(target_os = "macos", test))]
const LAUNCHD_LABEL: &str = "com.klaw.gateway";

#[derive(Debug, Args)]
pub struct DaemonCommand {
    #[command(subcommand)]
    pub command: DaemonSubcommands,
}

#[derive(Debug, Subcommand)]
pub enum DaemonSubcommands {
    /// Install and start the user-level gateway service.
    Install(DaemonInstallCommand),
    /// Show current gateway service status.
    Status(DaemonStatusCommand),
    /// Uninstall and stop the user-level gateway service.
    Uninstall(DaemonUninstallCommand),
    /// Start the installed gateway service.
    Start(DaemonStartCommand),
    /// Stop the installed gateway service.
    Stop(DaemonStopCommand),
    /// Restart the installed gateway service.
    Restart(DaemonRestartCommand),
}

#[derive(Debug, Args, Default)]
pub struct DaemonInstallCommand {}

#[derive(Debug, Args, Default)]
pub struct DaemonStatusCommand {}

#[derive(Debug, Args, Default)]
pub struct DaemonUninstallCommand {}

#[derive(Debug, Args, Default)]
pub struct DaemonStartCommand {}

#[derive(Debug, Args, Default)]
pub struct DaemonStopCommand {}

#[derive(Debug, Args, Default)]
pub struct DaemonRestartCommand {}

#[derive(Debug, Clone)]
struct ServiceContext {
    executable_path: PathBuf,
    config_path: PathBuf,
    working_directory: PathBuf,
    stdout_log_path: PathBuf,
    stderr_log_path: PathBuf,
}

#[derive(Debug, Clone)]
struct ServiceStatus {
    service: &'static str,
    manager: &'static str,
    installed: bool,
    loaded: bool,
    running: bool,
    unit_path: PathBuf,
    stdout_log_path: PathBuf,
    stderr_log_path: PathBuf,
    details: Vec<(&'static str, String)>,
}

trait DaemonManager {
    fn install(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError>;
    fn status(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError>;
    fn uninstall(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError>;
    fn start(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError>;
    fn stop(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError>;
    fn restart(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError>;
}

#[derive(Debug, Error)]
enum DaemonError {
    #[error("daemon management is only supported on macOS and Linux")]
    UnsupportedPlatform,
    #[error("failed to resolve default config path: {0}")]
    ConfigPath(String),
    #[error("failed to resolve current executable path: {0}")]
    CurrentExecutable(std::io::Error),
    #[error("failed to resolve user data directory: {0}")]
    Storage(#[from] klaw_storage::StorageError),
    #[error("failed to create directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write service file '{path}': {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("installed service file is missing at '{0}'")]
    NotInstalled(PathBuf),
    #[error("required command '{command}' was not found in PATH")]
    CommandNotFound { command: &'static str },
    #[error("command `{command}` failed (exit {exit_code})\nstdout:\n{stdout}\nstderr:\n{stderr}")]
    CommandFailed {
        command: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    #[error("failed to inspect launchd status: {0}")]
    LaunchdStatus(String),
}

#[derive(Debug)]
struct CommandOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

impl DaemonCommand {
    pub fn run(self, config_path: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
        let context = ServiceContext::resolve(config_path)?;
        let manager = current_manager()?;
        match self.command {
            DaemonSubcommands::Install(_) => {
                let status = manager.install(&context)?;
                print_status("Daemon Installed", &status);
            }
            DaemonSubcommands::Status(_) => {
                let status = manager.status(&context)?;
                print_status("Daemon Status", &status);
            }
            DaemonSubcommands::Uninstall(_) => {
                let status = manager.uninstall(&context)?;
                print_status("Daemon Uninstalled", &status);
            }
            DaemonSubcommands::Start(_) => {
                let status = manager.start(&context)?;
                print_status("Daemon Started", &status);
            }
            DaemonSubcommands::Stop(_) => {
                let status = manager.stop(&context)?;
                print_status("Daemon Stopped", &status);
            }
            DaemonSubcommands::Restart(_) => {
                let status = manager.restart(&context)?;
                print_status("Daemon Restarted", &status);
            }
        }
        Ok(())
    }
}

impl ServiceContext {
    fn resolve(config_path: Option<&Path>) -> Result<Self, DaemonError> {
        let executable_path = env::current_exe().map_err(DaemonError::CurrentExecutable)?;
        let config_path = match config_path {
            Some(path) => absolutize_path(path)?,
            None => {
                default_config_path().map_err(|err| DaemonError::ConfigPath(err.to_string()))?
            }
        };
        let storage_paths = StoragePaths::from_home_dir()?;
        let log_dir = storage_paths.root_dir.join("logs");
        Ok(Self {
            executable_path,
            config_path,
            working_directory: storage_paths.root_dir.clone(),
            stdout_log_path: log_dir.join("gateway.stdout.log"),
            stderr_log_path: log_dir.join("gateway.stderr.log"),
        })
    }
}

fn absolutize_path(path: &Path) -> Result<PathBuf, DaemonError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(DaemonError::CurrentExecutable)
}

fn current_manager() -> Result<Box<dyn DaemonManager>, DaemonError> {
    #[cfg(target_os = "macos")]
    {
        return Ok(Box::new(LaunchdUserManager::default()));
    }

    #[cfg(target_os = "linux")]
    {
        return Ok(Box::new(SystemdUserManager::default()));
    }

    #[allow(unreachable_code)]
    Err(DaemonError::UnsupportedPlatform)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Default)]
struct SystemdUserManager;

#[cfg(any(target_os = "linux", test))]
impl SystemdUserManager {
    fn unit_path() -> Result<PathBuf, DaemonError> {
        let home = env::var_os("HOME")
            .ok_or_else(|| DaemonError::ConfigPath("HOME is not set".to_string()))?;
        Ok(PathBuf::from(home)
            .join(".config")
            .join("systemd")
            .join("user")
            .join(SYSTEMD_UNIT_NAME))
    }

    fn ensure_service_dirs(context: &ServiceContext) -> Result<PathBuf, DaemonError> {
        let unit_path = Self::unit_path()?;
        if let Some(parent) = unit_path.parent() {
            fs::create_dir_all(parent).map_err(|source| DaemonError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        if let Some(parent) = context.stdout_log_path.parent() {
            fs::create_dir_all(parent).map_err(|source| DaemonError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::create_dir_all(&context.working_directory).map_err(|source| {
            DaemonError::CreateDir {
                path: context.working_directory.clone(),
                source,
            }
        })?;
        Ok(unit_path)
    }

    fn render_unit(context: &ServiceContext) -> String {
        let exec_start = format!(
            "{} --config {} gateway",
            systemd_quote(&context.executable_path),
            systemd_quote(&context.config_path)
        );
        format!(
            "[Unit]\nDescription=Klaw Gateway user service\nAfter=network.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nExecStart={}\nRestart=on-failure\nRestartSec=5\nStandardOutput=append:{}\nStandardError=append:{}\n\n[Install]\nWantedBy=default.target\n",
            systemd_quote(&context.working_directory),
            exec_start,
            context.stdout_log_path.display(),
            context.stderr_log_path.display(),
        )
    }

    fn daemon_reload(&self) -> Result<(), DaemonError> {
        run_command("systemctl", ["--user", "daemon-reload"])?;
        Ok(())
    }
}

#[cfg(any(target_os = "linux", test))]
impl DaemonManager for SystemdUserManager {
    fn install(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let unit_path = Self::ensure_service_dirs(context)?;
        fs::write(&unit_path, Self::render_unit(context)).map_err(|source| {
            DaemonError::WriteFile {
                path: unit_path.clone(),
                source,
            }
        })?;
        self.daemon_reload()?;
        run_command(
            "systemctl",
            ["--user", "enable", "--now", SYSTEMD_UNIT_NAME],
        )?;
        self.status(context)
    }

    fn status(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let unit_path = Self::unit_path()?;
        let installed = unit_path.exists();
        let enabled_output =
            run_command_allow_failure("systemctl", ["--user", "is-enabled", SYSTEMD_UNIT_NAME])?;
        let active_output =
            run_command_allow_failure("systemctl", ["--user", "is-active", SYSTEMD_UNIT_NAME])?;
        let show_output = run_command_allow_failure(
            "systemctl",
            [
                "--user",
                "show",
                SYSTEMD_UNIT_NAME,
                "--property=LoadState",
                "--property=UnitFileState",
                "--property=ActiveState",
                "--property=SubState",
            ],
        )?;
        let show_map = parse_systemd_show(&show_output.stdout);
        let loaded = matches!(
            show_map.get("LoadState").map(String::as_str),
            Some("loaded")
        ) && !matches!(
            show_map.get("UnitFileState").map(String::as_str),
            Some("not-found")
        );
        let running = matches!(
            show_map.get("ActiveState").map(String::as_str),
            Some("active")
        ) || active_output.stdout.trim() == "active";
        Ok(ServiceStatus {
            service: SERVICE_NAME,
            manager: "systemd-user",
            installed,
            loaded,
            running,
            unit_path,
            stdout_log_path: context.stdout_log_path.clone(),
            stderr_log_path: context.stderr_log_path.clone(),
            details: vec![
                ("enabled", enabled_output.stdout.trim().to_string()),
                (
                    "active_state",
                    show_map.get("ActiveState").cloned().unwrap_or_default(),
                ),
                (
                    "sub_state",
                    show_map.get("SubState").cloned().unwrap_or_default(),
                ),
                (
                    "unit_file_state",
                    show_map.get("UnitFileState").cloned().unwrap_or_default(),
                ),
            ],
        })
    }

    fn uninstall(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let unit_path = Self::unit_path()?;
        let _ = run_command_allow_failure(
            "systemctl",
            ["--user", "disable", "--now", SYSTEMD_UNIT_NAME],
        )?;
        if unit_path.exists() {
            fs::remove_file(&unit_path).map_err(|source| DaemonError::WriteFile {
                path: unit_path.clone(),
                source,
            })?;
        }
        self.daemon_reload()?;
        self.status(context)
    }

    fn start(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        ensure_installed(&Self::unit_path()?)?;
        run_command("systemctl", ["--user", "start", SYSTEMD_UNIT_NAME])?;
        self.status(context)
    }

    fn stop(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        ensure_installed(&Self::unit_path()?)?;
        let _ = run_command_allow_failure("systemctl", ["--user", "stop", SYSTEMD_UNIT_NAME])?;
        self.status(context)
    }

    fn restart(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        ensure_installed(&Self::unit_path()?)?;
        let before = self.status(context)?;
        if before.running {
            run_command("systemctl", ["--user", "restart", SYSTEMD_UNIT_NAME])?;
        } else {
            run_command("systemctl", ["--user", "start", SYSTEMD_UNIT_NAME])?;
        }
        self.status(context)
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default)]
struct LaunchdUserManager;

#[cfg(any(target_os = "macos", test))]
impl LaunchdUserManager {
    fn plist_path() -> Result<PathBuf, DaemonError> {
        let home = env::var_os("HOME")
            .ok_or_else(|| DaemonError::ConfigPath("HOME is not set".to_string()))?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist")))
    }

    fn domain_target() -> Result<String, DaemonError> {
        let output = run_command("id", ["-u"])?;
        let uid = output.stdout.trim();
        if uid.is_empty() {
            return Err(DaemonError::LaunchdStatus(
                "failed to resolve current user id".to_string(),
            ));
        }
        Ok(format!("gui/{uid}"))
    }

    fn service_target() -> Result<String, DaemonError> {
        Ok(format!("{}/{}", Self::domain_target()?, LAUNCHD_LABEL))
    }

    fn ensure_service_dirs(context: &ServiceContext) -> Result<PathBuf, DaemonError> {
        let plist_path = Self::plist_path()?;
        if let Some(parent) = plist_path.parent() {
            fs::create_dir_all(parent).map_err(|source| DaemonError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        if let Some(parent) = context.stdout_log_path.parent() {
            fs::create_dir_all(parent).map_err(|source| DaemonError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::create_dir_all(&context.working_directory).map_err(|source| {
            DaemonError::CreateDir {
                path: context.working_directory.clone(),
                source,
            }
        })?;
        Ok(plist_path)
    }

    fn render_plist(context: &ServiceContext) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>--config</string>
    <string>{}</string>
    <string>gateway</string>
  </array>
  <key>WorkingDirectory</key>
  <string>{}</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
            xml_escape(LAUNCHD_LABEL),
            xml_escape_path(&context.executable_path),
            xml_escape_path(&context.config_path),
            xml_escape_path(&context.working_directory),
            xml_escape_path(&context.stdout_log_path),
            xml_escape_path(&context.stderr_log_path),
        )
    }
}

#[cfg(any(target_os = "macos", test))]
impl DaemonManager for LaunchdUserManager {
    fn install(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let plist_path = Self::ensure_service_dirs(context)?;
        fs::write(&plist_path, Self::render_plist(context)).map_err(|source| {
            DaemonError::WriteFile {
                path: plist_path.clone(),
                source,
            }
        })?;
        let _ = run_command_allow_failure("launchctl", ["bootout", &Self::service_target()?])?;
        run_command(
            "launchctl",
            [
                "bootstrap",
                &Self::domain_target()?,
                plist_path.to_string_lossy().as_ref(),
            ],
        )?;
        self.status(context)
    }

    fn status(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let plist_path = Self::plist_path()?;
        let installed = plist_path.exists();
        let print_output =
            run_command_allow_failure("launchctl", ["print", &Self::service_target()?])?;
        let loaded = print_output.exit_code == 0;
        let running = loaded && launchd_is_running(&print_output.stdout);
        let detail = summarize_launchd_status(&print_output.stdout, &print_output.stderr);
        Ok(ServiceStatus {
            service: SERVICE_NAME,
            manager: "launchd-user",
            installed,
            loaded,
            running,
            unit_path: plist_path,
            stdout_log_path: context.stdout_log_path.clone(),
            stderr_log_path: context.stderr_log_path.clone(),
            details: vec![("launchd_state", detail)],
        })
    }

    fn uninstall(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let plist_path = Self::plist_path()?;
        let _ = run_command_allow_failure("launchctl", ["bootout", &Self::service_target()?])?;
        if plist_path.exists() {
            fs::remove_file(&plist_path).map_err(|source| DaemonError::WriteFile {
                path: plist_path.clone(),
                source,
            })?;
        }
        self.status(context)
    }

    fn start(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let plist_path = Self::plist_path()?;
        ensure_installed(&plist_path)?;
        let status = self.status(context)?;
        if !status.loaded {
            run_command(
                "launchctl",
                [
                    "bootstrap",
                    &Self::domain_target()?,
                    plist_path.to_string_lossy().as_ref(),
                ],
            )?;
        }
        self.status(context)
    }

    fn stop(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let plist_path = Self::plist_path()?;
        ensure_installed(&plist_path)?;
        let _ = run_command_allow_failure("launchctl", ["bootout", &Self::service_target()?])?;
        self.status(context)
    }

    fn restart(&self, context: &ServiceContext) -> Result<ServiceStatus, DaemonError> {
        let plist_path = Self::plist_path()?;
        ensure_installed(&plist_path)?;
        let _ = run_command_allow_failure("launchctl", ["bootout", &Self::service_target()?])?;
        run_command(
            "launchctl",
            [
                "bootstrap",
                &Self::domain_target()?,
                plist_path.to_string_lossy().as_ref(),
            ],
        )?;
        self.status(context)
    }
}

fn print_status(title: &str, status: &ServiceStatus) {
    println!("{title}");
    println!("  service: {}", status.service);
    println!("  manager: {}", status.manager);
    println!("  installed: {}", status.installed);
    println!("  loaded: {}", status.loaded);
    println!("  running: {}", status.running);
    println!("  unit_path: {}", status.unit_path.display());
    println!("  stdout_log: {}", status.stdout_log_path.display());
    println!("  stderr_log: {}", status.stderr_log_path.display());
    for (key, value) in &status.details {
        if !value.is_empty() {
            println!("  {key}: {value}");
        }
    }
}

fn ensure_installed(path: &Path) -> Result<(), DaemonError> {
    if path.exists() {
        Ok(())
    } else {
        Err(DaemonError::NotInstalled(path.to_path_buf()))
    }
}

fn run_command<const N: usize>(
    command: &'static str,
    args: [&str; N],
) -> Result<CommandOutput, DaemonError> {
    let output = run_command_allow_failure(command, args)?;
    if output.exit_code == 0 {
        Ok(output)
    } else {
        Err(DaemonError::CommandFailed {
            command: format!("{command} {}", args.join(" ")),
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

fn run_command_allow_failure<const N: usize>(
    command: &'static str,
    args: [&str; N],
) -> Result<CommandOutput, DaemonError> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                DaemonError::CommandNotFound { command }
            } else {
                DaemonError::CommandFailed {
                    command: format!("{command} {}", args.join(" ")),
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: source.to_string(),
                }
            }
        })?;
    Ok(CommandOutput {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

#[cfg(any(target_os = "linux", test))]
fn parse_systemd_show(output: &str) -> std::collections::BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn launchd_is_running(output: &str) -> bool {
    output.contains("state = running")
        || output
            .lines()
            .any(|line| line.trim_start().starts_with("pid ="))
}

#[cfg(any(target_os = "macos", test))]
fn summarize_launchd_status(stdout: &str, stderr: &str) -> String {
    if stdout.is_empty() {
        return stderr.lines().next().unwrap_or_default().trim().to_string();
    }
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("state =") || trimmed.starts_with("pid =") {
            return trimmed.to_string();
        }
    }
    stdout.lines().next().unwrap_or_default().trim().to_string()
}

#[cfg(any(target_os = "linux", test))]
fn systemd_quote(path: &Path) -> String {
    format!("\"{}\"", path.display().to_string().replace('"', "\\\""))
}

fn xml_escape_path(path: &Path) -> String {
    xml_escape(&path.display().to_string())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

impl fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "service={} installed={} loaded={} running={}",
            self.service, self.installed, self.loaded, self.running
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn sample_context() -> ServiceContext {
        ServiceContext {
            executable_path: PathBuf::from("/tmp/klaw/bin/klaw"),
            config_path: PathBuf::from("/tmp/klaw/config.toml"),
            working_directory: PathBuf::from("/tmp/klaw"),
            stdout_log_path: PathBuf::from("/tmp/klaw/logs/gateway.stdout.log"),
            stderr_log_path: PathBuf::from("/tmp/klaw/logs/gateway.stderr.log"),
        }
    }

    #[test]
    fn renders_systemd_unit_with_expected_exec_start() {
        let unit = SystemdUserManager::render_unit(&sample_context());
        assert!(unit.contains(
            "ExecStart=\"/tmp/klaw/bin/klaw\" --config \"/tmp/klaw/config.toml\" gateway"
        ));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("StandardOutput=append:/tmp/klaw/logs/gateway.stdout.log"));
    }

    #[test]
    fn renders_launchd_plist_with_expected_program_arguments() {
        let plist = LaunchdUserManager::render_plist(&sample_context());
        assert!(plist.contains("<string>/tmp/klaw/bin/klaw</string>"));
        assert!(plist.contains("<string>--config</string>"));
        assert!(plist.contains("<string>/tmp/klaw/config.toml</string>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn parses_systemd_show_output() {
        let parsed = parse_systemd_show(
            "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=running\n",
        );
        assert_eq!(parsed.get("LoadState").map(String::as_str), Some("loaded"));
        assert_eq!(parsed.get("SubState").map(String::as_str), Some("running"));
    }

    #[test]
    fn detects_launchd_running_state() {
        assert!(launchd_is_running("state = running\npid = 101\n"));
        assert!(!launchd_is_running("state = waiting\n"));
    }

    #[test]
    fn daemon_cli_parses_lifecycle_subcommands() {
        #[derive(Parser)]
        struct Wrapper {
            #[command(subcommand)]
            command: DaemonSubcommands,
        }

        for args in [
            ["test", "install"],
            ["test", "status"],
            ["test", "uninstall"],
            ["test", "start"],
            ["test", "stop"],
            ["test", "restart"],
        ] {
            let parsed = Wrapper::try_parse_from(args).expect("daemon subcommand should parse");
            match parsed.command {
                DaemonSubcommands::Install(_)
                | DaemonSubcommands::Status(_)
                | DaemonSubcommands::Uninstall(_)
                | DaemonSubcommands::Start(_)
                | DaemonSubcommands::Stop(_)
                | DaemonSubcommands::Restart(_) => {}
            }
        }
    }
}
