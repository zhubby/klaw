use klaw_config::{AppConfig, McpServerConfig, McpServerMode};
use klaw_runtime::StartupReport;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupDisplaySummary {
    pub version: String,
    pub skills: String,
    pub tools: String,
    pub mcp: String,
}

pub fn print_startup_banner(config: &AppConfig, report: &StartupReport) {
    let summary = build_startup_summary(config, report);
    let width = 18usize;

    println!(
        r#" _  ___      ___        __
| |/ / |    /   | |    / /
| ' /| |   / /| | | /|/ /
| . \| |__/ ___ | |/ | /
|_|\_\____/_/  |_|__/|__\

  ✨ crafted for calm, sharp loops ✨"#
    );
    println!();
    println!(
        "{:<width$} {}",
        "🚀 Version",
        summary.version,
        width = width
    );
    println!("{:<width$} {}", "🧠 Skills", summary.skills, width = width);
    println!("{:<width$} {}", "🛠️  Tools", summary.tools, width = width);
    println!("{:<width$} {}", "🔌 MCP", summary.mcp, width = width);
    println!();
}

pub fn build_startup_summary(config: &AppConfig, report: &StartupReport) -> StartupDisplaySummary {
    StartupDisplaySummary {
        version: env!("CARGO_PKG_VERSION").to_string(),
        tools: tools_for_display(report),
        skills: join_or_dash(&report.skill_names),
        mcp: format_mcp_status(config, report),
    }
}

fn tools_for_display(report: &StartupReport) -> String {
    join_or_dash(&report.tool_names)
}

fn format_mcp_status(config: &AppConfig, report: &StartupReport) -> String {
    let configured_servers: Vec<&McpServerConfig> = config
        .mcp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .collect();
    if configured_servers.is_empty() {
        return match &report.mcp_summary {
            Some(_) => "ready, no servers configured".to_string(),
            None => "bootstrapping, no servers configured".to_string(),
        };
    }

    let stdio_count = configured_servers
        .iter()
        .filter(|server| server.mode == McpServerMode::Stdio)
        .count();
    let sse_count = configured_servers.len().saturating_sub(stdio_count);
    let topology = match (stdio_count, sse_count) {
        (0, sse) => format!("{sse} sse"),
        (stdio, 0) => format!("{stdio} stdio"),
        (stdio, sse) => format!("{stdio} stdio, {sse} sse"),
    };

    match &report.mcp_summary {
        Some(summary) => {
            let failed_count = summary
                .statuses
                .iter()
                .filter(|s| s.state == klaw_mcp::McpLifecycleState::Failed)
                .count();
            let failure_suffix = if failed_count == 0 {
                String::new()
            } else {
                format!(", failures {}", failed_count)
            };
            format!(
                "ready, servers {}/{}, tools {}, {}{}",
                summary.active_servers.len(),
                configured_servers.len(),
                summary.tool_count,
                topology,
                failure_suffix
            )
        }
        None => format!("bootstrapping, {}", topology),
    }
}

fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use klaw_config::{AppConfig, McpServerConfig, McpServerMode};
    use klaw_mcp::{McpLifecycleState, McpServerKey, McpServerStatus, McpSyncResult};
    use klaw_runtime::StartupReport;

    use super::{
        StartupDisplaySummary, build_startup_summary, format_mcp_status, join_or_dash,
        tools_for_display,
    };

    #[test]
    fn tools_display_includes_enabled_optional_tools() {
        let mut config = AppConfig::default();
        config.tools.web_fetch.enabled = true;
        config.tools.web_search.enabled = true;
        config.tools.sub_agent.enabled = true;

        let report = StartupReport {
            skill_names: vec!["git-commit".to_string()],
            tool_names: vec![
                "apply_patch".to_string(),
                "memory".to_string(),
                "sub_agent".to_string(),
                "web_fetch".to_string(),
                "web_search".to_string(),
                "remote_browser".to_string(),
            ],
            mcp_summary: Some(McpSyncResult {
                keep: vec![McpServerKey::new("local")],
                start: vec![],
                restart: vec![],
                stop: vec![],
                statuses: vec![McpServerStatus {
                    key: McpServerKey::new("local"),
                    mode: McpServerMode::Stdio,
                    enabled: true,
                    state: McpLifecycleState::Running,
                    last_error: None,
                    tool_count: 2,
                }],
                active_servers: vec!["local".to_string()],
                tool_count: 2,
            }),
        };

        let rendered = tools_for_display(&report);
        assert!(rendered.contains("memory"));
        assert!(rendered.contains("web_fetch"));
        assert!(rendered.contains("web_search"));
        assert!(rendered.contains("sub_agent"));
        assert!(rendered.contains("remote_browser"));
    }

    #[test]
    fn mcp_status_reports_ready_counts() {
        let mut config = AppConfig::default();
        config.mcp.servers = vec![McpServerConfig {
            id: "local".to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            command: Some("npx".to_string()),
            args: Vec::new(),
            env: Default::default(),
            cwd: None,
            url: None,
            headers: Default::default(),
        }];

        let report = StartupReport {
            skill_names: Vec::new(),
            tool_names: Vec::new(),
            mcp_summary: Some(McpSyncResult {
                keep: vec![McpServerKey::new("local")],
                start: vec![],
                restart: vec![],
                stop: vec![],
                statuses: vec![McpServerStatus {
                    key: McpServerKey::new("local"),
                    mode: McpServerMode::Stdio,
                    enabled: true,
                    state: McpLifecycleState::Running,
                    last_error: None,
                    tool_count: 3,
                }],
                active_servers: vec!["local".to_string()],
                tool_count: 3,
            }),
        };

        let rendered = format_mcp_status(&config, &report);
        assert!(rendered.contains("ready"));
        assert!(rendered.contains("servers 1/1"));
        assert!(rendered.contains("tools 3"));
    }

    #[test]
    fn join_or_dash_handles_empty_values() {
        assert_eq!(join_or_dash(&[]), "-");
        assert_eq!(join_or_dash(&["a".to_string(), "b".to_string()]), "a, b");
    }

    #[test]
    fn startup_summary_collects_banner_fields() {
        let config = AppConfig::default();
        let report = StartupReport::default();
        let summary = build_startup_summary(&config, &report);
        assert_eq!(
            summary,
            StartupDisplaySummary {
                version: env!("CARGO_PKG_VERSION").to_string(),
                skills: "-".to_string(),
                tools: "-".to_string(),
                mcp: "bootstrapping, no servers configured".to_string(),
            }
        );
    }
}
