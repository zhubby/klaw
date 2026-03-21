use klaw_util::{DependencyCategory, DependencyStatus, EnvironmentCheckReport};
use std::process::Command;
use tracing::{info, warn};

struct BinaryDependency {
    name: &'static str,
    description: &'static str,
    version_args: &'static [&'static str],
    required: bool,
    category: DependencyCategory,
    version_parser: fn(&str) -> Option<String>,
}

const DEPENDENCIES: &[BinaryDependency] = &[
    BinaryDependency {
        name: "git",
        description: "Skills registry synchronization",
        version_args: &["--version"],
        required: true,
        category: DependencyCategory::Required,
        version_parser: parse_git_version,
    },
    BinaryDependency {
        name: "rg",
        description: "Local file content search (ripgrep)",
        version_args: &["--version"],
        required: true,
        category: DependencyCategory::Required,
        version_parser: parse_rg_version,
    },
    BinaryDependency {
        name: "zellij",
        description: "Terminal multiplexer (preferred)",
        version_args: &["--version"],
        required: false,
        category: DependencyCategory::OptionalWithFallback,
        version_parser: parse_zellij_version,
    },
    BinaryDependency {
        name: "tmux",
        description: "Terminal multiplexer (fallback)",
        version_args: &["-V"],
        required: false,
        category: DependencyCategory::OptionalWithFallback,
        version_parser: parse_tmux_version,
    },
];

pub fn check_environment() -> EnvironmentCheckReport {
    info!("Checking environment dependencies...");

    let mut checks = Vec::with_capacity(DEPENDENCIES.len());

    for dep in DEPENDENCIES {
        let status = check_dependency(dep);
        log_dependency_status(&status);

        checks.push(status);
    }

    let available_count = checks.iter().filter(|c| c.available).count();
    let total = checks.len();
    let all_required = checks.iter().filter(|c| c.required).all(|c| c.available);
    let tm_available = checks
        .iter()
        .filter(|c| c.name == "zellij" || c.name == "tmux")
        .any(|c| c.available);

    if all_required && tm_available {
        info!(
            available = available_count,
            total, "Environment check completed: all dependencies available"
        );
    } else if all_required {
        warn!(
            available = available_count,
            total, "Environment check completed: terminal multiplexer not available"
        );
    } else {
        warn!(
            available = available_count,
            total, "Environment check completed: some required dependencies missing"
        );
    }

    EnvironmentCheckReport {
        checks,
        checked_at: time::OffsetDateTime::now_utc(),
    }
}

fn check_dependency(dep: &BinaryDependency) -> DependencyStatus {
    let output = Command::new(dep.name).args(dep.version_args).output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr);
            let version = (dep.version_parser)(&combined);

            DependencyStatus {
                name: dep.name.to_string(),
                description: dep.description.to_string(),
                available: true,
                version,
                required: dep.required,
                category: dep.category,
            }
        }
        Ok(_) => DependencyStatus {
            name: dep.name.to_string(),
            description: dep.description.to_string(),
            available: false,
            version: None,
            required: dep.required,
            category: dep.category,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DependencyStatus {
            name: dep.name.to_string(),
            description: dep.description.to_string(),
            available: false,
            version: None,
            required: dep.required,
            category: dep.category,
        },
        Err(e) => {
            warn!(
                dependency = dep.name,
                error = %e,
                "Failed to check dependency"
            );
            DependencyStatus {
                name: dep.name.to_string(),
                description: dep.description.to_string(),
                available: false,
                version: None,
                required: dep.required,
                category: dep.category,
            }
        }
    }
}

fn log_dependency_status(status: &DependencyStatus) {
    if status.available {
        let version_str = status
            .version
            .as_deref()
            .map(|v| format!(" ({})", v))
            .unwrap_or_default();
        info!(
            dependency = status.name.as_str(),
            available = true,
            version = status.version.as_deref(),
            "{}: available{}",
            status.name,
            version_str
        );
    } else if status.required {
        warn!(
            dependency = status.name.as_str(),
            available = false,
            required = true,
            "{}: NOT FOUND (required)",
            status.name
        );
    } else {
        info!(
            dependency = status.name.as_str(),
            available = false,
            required = false,
            "{}: not found (optional)",
            status.name
        );
    }
}

fn parse_git_version(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("git version "))
        .map(|v| v.trim().to_string())
}

fn parse_rg_version(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("ripgrep "))
        .map(|v| v.trim().to_string())
}

fn parse_zellij_version(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("zellij "))
        .map(|v| v.trim().to_string())
}

fn parse_tmux_version(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("tmux "))
        .map(|v| v.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_git_version_extracts_version() {
        let output = "git version 2.43.0\n";
        assert_eq!(parse_git_version(output), Some("2.43.0".to_string()));
    }

    #[test]
    fn parse_rg_version_extracts_version() {
        let output = "ripgrep 14.1.0\n";
        assert_eq!(parse_rg_version(output), Some("14.1.0".to_string()));
    }

    #[test]
    fn parse_zellij_version_extracts_version() {
        let output = "zellij 0.40.0\n";
        assert_eq!(parse_zellij_version(output), Some("0.40.0".to_string()));
    }

    #[test]
    fn parse_tmux_version_extracts_version() {
        let output = "tmux 3.4\n";
        assert_eq!(parse_tmux_version(output), Some("3.4".to_string()));
    }
}
