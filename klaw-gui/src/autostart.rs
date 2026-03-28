use anyhow::bail;
#[cfg(target_os = "macos")]
use anyhow::{Context, anyhow};
#[cfg(target_os = "macos")]
use klaw_util::home_dir;
#[cfg(any(target_os = "macos", test))]
use std::ffi::OsStr;
#[cfg(target_os = "macos")]
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

const LAUNCH_AGENT_LABEL: &str = "io.klaw.app";
#[cfg(target_os = "macos")]
const MACOS_BUNDLE_UNAVAILABLE_REASON: &str =
    "Launch at startup requires running from the packaged Klaw.app bundle.";
#[cfg(not(target_os = "macos"))]
const NON_MACOS_UNAVAILABLE_REASON: &str = "Launch at startup is only available on macOS.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unsupported(String),
}

impl Availability {
    pub fn unsupported_reason(&self) -> Option<&str> {
        match self {
            Self::Available => None,
            Self::Unsupported(reason) => Some(reason.as_str()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    Unchanged,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(target_os = "macos")]
struct LaunchAgentStatus {
    installed: bool,
    loaded: bool,
}

pub fn enable_availability() -> Availability {
    #[cfg(target_os = "macos")]
    {
        match current_bundle_executable() {
            Ok(_) => Availability::Available,
            Err(err) => Availability::Unsupported(err.to_string()),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Availability::Unsupported(NON_MACOS_UNAVAILABLE_REASON.to_string())
    }
}

pub fn apply(enabled: bool) -> anyhow::Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enabled;
        bail!(NON_MACOS_UNAVAILABLE_REASON);
    }

    #[cfg(target_os = "macos")]
    {
        if enabled { enable() } else { disable() }
    }
}

pub fn reconcile(desired_enabled: bool) -> anyhow::Result<ReconcileOutcome> {
    #[cfg(not(target_os = "macos"))]
    {
        if desired_enabled {
            bail!(NON_MACOS_UNAVAILABLE_REASON);
        }
        return Ok(ReconcileOutcome::Unchanged);
    }

    #[cfg(target_os = "macos")]
    {
        let plist_path = launch_agent_plist_path()?;
        let status = launch_agent_status(&plist_path)?;

        if desired_enabled {
            let executable_path = current_bundle_executable()?;
            let expected_plist = render_launch_agent_plist(&executable_path);
            let existing_plist = fs::read_to_string(&plist_path).ok();
            let needs_install =
                !status.loaded || existing_plist.as_deref() != Some(expected_plist.as_str());
            if needs_install {
                install_launch_agent(&plist_path, &executable_path)?;
                Ok(ReconcileOutcome::Enabled)
            } else {
                Ok(ReconcileOutcome::Unchanged)
            }
        } else if status.installed || status.loaded {
            uninstall_launch_agent(&plist_path)?;
            Ok(ReconcileOutcome::Disabled)
        } else {
            Ok(ReconcileOutcome::Unchanged)
        }
    }
}

#[cfg(target_os = "macos")]
fn enable() -> anyhow::Result<()> {
    let plist_path = launch_agent_plist_path()?;
    let executable_path = current_bundle_executable()?;
    install_launch_agent(&plist_path, &executable_path)
}

#[cfg(target_os = "macos")]
fn disable() -> anyhow::Result<()> {
    let plist_path = launch_agent_plist_path()?;
    uninstall_launch_agent(&plist_path)
}

#[cfg(target_os = "macos")]
fn install_launch_agent(plist_path: &Path, executable_path: &Path) -> anyhow::Result<()> {
    let Some(parent) = plist_path.parent() else {
        bail!(
            "launch agent path '{}' does not have a parent directory",
            plist_path.display()
        );
    };
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create '{}'", parent.display()))?;
    fs::write(plist_path, render_launch_agent_plist(executable_path))
        .with_context(|| format!("failed to write '{}'", plist_path.display()))?;

    let domain = launchctl_domain_target()?;
    let service = launchctl_service_target()?;
    let plist = plist_path.to_string_lossy().into_owned();
    let _ = run_command_allow_failure("launchctl", &["bootout", service.as_str()])?;
    run_command("launchctl", &["bootstrap", domain.as_str(), plist.as_str()])?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_launch_agent(plist_path: &Path) -> anyhow::Result<()> {
    let service = launchctl_service_target()?;
    let _ = run_command_allow_failure("launchctl", &["bootout", service.as_str()])?;
    if plist_path.exists() {
        fs::remove_file(plist_path)
            .with_context(|| format!("failed to remove '{}'", plist_path.display()))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_agent_status(plist_path: &Path) -> anyhow::Result<LaunchAgentStatus> {
    let installed = plist_path.exists();
    let service = launchctl_service_target()?;
    let output = run_command_allow_failure("launchctl", &["print", service.as_str()])?;
    Ok(LaunchAgentStatus {
        installed,
        loaded: output.exit_code == 0,
    })
}

#[cfg(target_os = "macos")]
fn current_bundle_executable() -> anyhow::Result<PathBuf> {
    let path = std::env::current_exe().context("failed to resolve current executable path")?;
    bundle_executable_from_path(&path).ok_or_else(|| anyhow!(MACOS_BUNDLE_UNAVAILABLE_REASON))
}

#[cfg(target_os = "macos")]
fn launch_agent_plist_path() -> anyhow::Result<PathBuf> {
    let home = home_dir().ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(launch_agent_plist_path_in_home(&home))
}

#[cfg(any(target_os = "macos", test))]
fn launch_agent_plist_path_in_home(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

#[cfg(any(target_os = "macos", test))]
fn bundle_executable_from_path(path: &Path) -> Option<PathBuf> {
    let macos_dir = path.parent()?;
    if macos_dir.file_name()? != OsStr::new("MacOS") {
        return None;
    }

    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()? != OsStr::new("Contents") {
        return None;
    }

    let app_dir = contents_dir.parent()?;
    if app_dir.extension()? != OsStr::new("app") {
        return None;
    }

    Some(path.to_path_buf())
}

#[cfg(any(target_os = "macos", test))]
fn render_launch_agent_plist(executable_path: &Path) -> String {
    let executable_path = xml_escape_path(executable_path);
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
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <false/>
  <key>LimitLoadToSessionType</key>
  <array>
    <string>Aqua</string>
  </array>
</dict>
</plist>
"#,
        xml_escape(LAUNCH_AGENT_LABEL),
        executable_path,
    )
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape_path(path: &Path) -> String {
    xml_escape(path.to_string_lossy().as_ref())
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "macos")]
fn launchctl_domain_target() -> anyhow::Result<String> {
    let output = run_command("id", &["-u"])?;
    let uid = output.stdout.trim();
    if uid.is_empty() {
        bail!("failed to resolve current user id");
    }
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
fn launchctl_service_target() -> anyhow::Result<String> {
    Ok(format!(
        "{}/{}",
        launchctl_domain_target()?,
        LAUNCH_AGENT_LABEL
    ))
}

#[cfg(target_os = "macos")]
fn run_command(command: &str, args: &[&str]) -> anyhow::Result<CommandOutput> {
    let output = run_command_allow_failure(command, args)?;
    if output.exit_code == 0 {
        Ok(output)
    } else {
        bail!(
            "command '{}' failed with exit code {}: {}",
            format!("{command} {}", args.join(" ")),
            output.exit_code,
            command_error_details(&output)
        );
    }
}

#[cfg(target_os = "macos")]
fn run_command_allow_failure(command: &str, args: &[&str]) -> anyhow::Result<CommandOutput> {
    let output = Command::new(command).args(args).output().with_context(|| {
        format!(
            "failed to run '{}'",
            format!("{command} {}", args.join(" "))
        )
    })?;
    Ok(CommandOutput {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

#[cfg(target_os = "macos")]
fn command_error_details(output: &CommandOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }
    let stdout = output.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }
    "no output".to_string()
}

#[cfg(target_os = "macos")]
struct CommandOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

#[cfg(test)]
mod tests {
    use super::{
        LAUNCH_AGENT_LABEL, bundle_executable_from_path, launch_agent_plist_path_in_home,
        render_launch_agent_plist,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn bundle_executable_from_path_accepts_app_bundle_paths() {
        let path = Path::new("/Applications/Klaw.app/Contents/MacOS/Klaw");
        assert_eq!(bundle_executable_from_path(path), Some(path.to_path_buf()));
    }

    #[test]
    fn bundle_executable_from_path_rejects_non_bundle_paths() {
        assert_eq!(
            bundle_executable_from_path(Path::new("/usr/local/bin/klaw")),
            None
        );
        assert_eq!(
            bundle_executable_from_path(Path::new("/Applications/Klaw/Contents/MacOS/Klaw")),
            None
        );
    }

    #[test]
    fn launch_agent_plist_path_in_home_matches_expected_location() {
        assert_eq!(
            launch_agent_plist_path_in_home(Path::new("/Users/tester")),
            PathBuf::from("/Users/tester/Library/LaunchAgents")
                .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
        );
    }

    #[test]
    fn rendered_launch_agent_plist_escapes_executable_path() {
        let plist =
            render_launch_agent_plist(Path::new("/Applications/Klaw & Co.app/Contents/MacOS/Klaw"));
        assert!(plist.contains("<string>io.klaw.app</string>"));
        assert!(plist.contains("/Applications/Klaw &amp; Co.app/Contents/MacOS/Klaw"));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<false/>"));
    }
}
