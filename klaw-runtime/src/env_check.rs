use klaw_util::{DependencyCategory, DependencyStatus, EnvironmentCheckReport};
use std::process::Command;
use tracing::{info, warn};

struct BinaryDependency {
    name: &'static str,
    description: &'static str,
    project_url: Option<&'static str>,
    version_args: &'static [&'static str],
    required: bool,
    category: DependencyCategory,
    version_parser: fn(&str) -> Option<String>,
}

const DEPENDENCIES: &[BinaryDependency] = &[
    BinaryDependency {
        name: "git",
        description: "Skills registry synchronization",
        project_url: None,
        version_args: &["--version"],
        required: true,
        category: DependencyCategory::Required,
        version_parser: parse_git_version,
    },
    BinaryDependency {
        name: "rg",
        description: "Local file content search (ripgrep)",
        project_url: Some("https://github.com/BurntSushi/ripgrep"),
        version_args: &["--version"],
        required: false,
        category: DependencyCategory::Preferred,
        version_parser: parse_rg_version,
    },
    BinaryDependency {
        name: "tailscale",
        description: "Gateway public exposure (serve/funnel)",
        project_url: Some("https://tailscale.com"),
        version_args: &["version"],
        required: false,
        category: DependencyCategory::OptionalWithFallback,
        version_parser: parse_tailscale_version,
    },
    BinaryDependency {
        name: "zellij",
        description: "Terminal multiplexer (preferred)",
        project_url: Some("https://github.com/zellij-org/zellij"),
        version_args: &["--version"],
        required: false,
        category: DependencyCategory::OptionalWithFallback,
        version_parser: parse_zellij_version,
    },
    BinaryDependency {
        name: "tmux",
        description: "Terminal multiplexer (fallback)",
        project_url: Some("https://github.com/tmux/tmux"),
        version_args: &["-V"],
        required: false,
        category: DependencyCategory::OptionalWithFallback,
        version_parser: parse_tmux_version,
    },
    BinaryDependency {
        name: "docker",
        description: "Container CLI and image tooling",
        project_url: Some("https://www.docker.com"),
        version_args: &["--version"],
        required: false,
        category: DependencyCategory::OptionalWithFallback,
        version_parser: parse_docker_version,
    },
    BinaryDependency {
        name: "container",
        description: "Apple container CLI for macOS-native containers",
        project_url: Some("https://github.com/apple/container"),
        version_args: &["--version"],
        required: false,
        category: DependencyCategory::OptionalWithFallback,
        version_parser: parse_container_version,
    },
    BinaryDependency {
        name: "rtk",
        description: "Command proxy for token-optimized shell output",
        project_url: None,
        version_args: &["--version"],
        required: false,
        category: DependencyCategory::Preferred,
        version_parser: parse_rtk_version,
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
    let all_preferred = checks
        .iter()
        .filter(|c| matches!(c.category, DependencyCategory::Preferred))
        .all(|c| c.available);
    let tm_available = checks
        .iter()
        .filter(|c| c.name == "zellij" || c.name == "tmux")
        .any(|c| c.available);

    if all_required && all_preferred && tm_available {
        info!(
            available = available_count,
            total, "Environment check completed: all dependencies available"
        );
    } else if all_required && all_preferred {
        warn!(
            available = available_count,
            total, "Environment check completed: terminal multiplexer not available"
        );
    } else if all_required {
        warn!(
            available = available_count,
            total, "Environment check completed: some preferred dependencies missing"
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
    let output = dependency_command(dep.name).args(dep.version_args).output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr);
            let version = (dep.version_parser)(&combined);

            DependencyStatus {
                name: dep.name.to_string(),
                description: dep.description.to_string(),
                project_url: dep.project_url.map(ToString::to_string),
                available: true,
                version,
                required: dep.required,
                category: dep.category,
            }
        }
        Ok(_) => DependencyStatus {
            name: dep.name.to_string(),
            description: dep.description.to_string(),
            project_url: dep.project_url.map(ToString::to_string),
            available: false,
            version: None,
            required: dep.required,
            category: dep.category,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DependencyStatus {
            name: dep.name.to_string(),
            description: dep.description.to_string(),
            project_url: dep.project_url.map(ToString::to_string),
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
                project_url: dep.project_url.map(ToString::to_string),
                available: false,
                version: None,
                required: dep.required,
                category: dep.category,
            }
        }
    }
}

fn dependency_command(binary: &str) -> Command {
    let mut command = Command::new(binary);
    if let Some(path) = klaw_util::command_search_path() {
        command.env("PATH", path);
    }
    command
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
    } else if matches!(status.category, DependencyCategory::Preferred) {
        warn!(
            dependency = status.name.as_str(),
            available = false,
            required = false,
            "{}: not found (preferred)",
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

fn parse_tailscale_version(output: &str) -> Option<String> {
    output.lines().next().map(|v| v.trim().to_string())
}

fn parse_docker_version(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Docker version "))
        .map(|line| line.split(',').next().unwrap_or(line).trim().to_string())
}

fn parse_container_version(output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }

    if let Some(version) = line.strip_prefix("container version ") {
        return Some(version.trim().to_string());
    }

    if let Some(version) = line.strip_prefix("container ") {
        return Some(version.trim().to_string());
    }

    Some(line.to_string())
}

fn parse_rtk_version(output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }

    if let Some(version) = line.strip_prefix("rtk version ") {
        return Some(version.trim().to_string());
    }

    if let Some(version) = line.strip_prefix("rtk ") {
        return Some(version.trim().to_string());
    }

    Some(line.to_string())
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

    #[test]
    fn parse_docker_version_extracts_semver() {
        let output = "Docker version 28.0.1, build 068a01e\n";
        assert_eq!(parse_docker_version(output), Some("28.0.1".to_string()));
    }

    #[test]
    fn parse_container_version_extracts_prefixed_version() {
        let output = "container 0.10.0\n";
        assert_eq!(parse_container_version(output), Some("0.10.0".to_string()));
    }

    #[test]
    fn parse_container_version_keeps_plain_version() {
        let output = "0.10.0\n";
        assert_eq!(parse_container_version(output), Some("0.10.0".to_string()));
    }

    #[test]
    fn rg_is_preferred_not_required() {
        let rg = DEPENDENCIES
            .iter()
            .find(|dep| dep.name == "rg")
            .expect("rg dependency should exist");

        assert!(!rg.required);
        assert!(matches!(rg.category, DependencyCategory::Preferred));
        assert_eq!(
            rg.project_url,
            Some("https://github.com/BurntSushi/ripgrep")
        );
    }

    #[test]
    fn tailscale_is_optional() {
        let ts = DEPENDENCIES
            .iter()
            .find(|dep| dep.name == "tailscale")
            .expect("tailscale dependency should exist");

        assert!(!ts.required);
        assert!(matches!(
            ts.category,
            DependencyCategory::OptionalWithFallback
        ));
        assert_eq!(ts.project_url, Some("https://tailscale.com"));
    }

    #[test]
    fn docker_and_container_are_optional() {
        for name in ["docker", "container"] {
            let dep = DEPENDENCIES
                .iter()
                .find(|dep| dep.name == name)
                .expect("container dependency should exist");

            assert!(!dep.required);
            assert!(matches!(
                dep.category,
                DependencyCategory::OptionalWithFallback
            ));
        }
    }

    #[test]
    fn parse_tailscale_version_extracts_version() {
        let output = "1.76.6\n";
        assert_eq!(parse_tailscale_version(output), Some("1.76.6".to_string()));
    }

    #[test]
    fn parse_rtk_version_extracts_prefixed_version() {
        let output = "rtk 0.3.0\n";
        assert_eq!(parse_rtk_version(output), Some("0.3.0".to_string()));
    }

    #[test]
    fn parse_rtk_version_extracts_version_prefix() {
        let output = "rtk version 0.3.0\n";
        assert_eq!(parse_rtk_version(output), Some("0.3.0".to_string()));
    }

    #[test]
    fn parse_rtk_version_keeps_plain_version() {
        let output = "0.3.0\n";
        assert_eq!(parse_rtk_version(output), Some("0.3.0".to_string()));
    }

    #[test]
    fn rtk_is_preferred_not_required() {
        let rtk = DEPENDENCIES
            .iter()
            .find(|dep| dep.name == "rtk")
            .expect("rtk dependency should exist");

        assert!(!rtk.required);
        assert!(matches!(rtk.category, DependencyCategory::Preferred));
        assert!(rtk.project_url.is_none());
    }
}
