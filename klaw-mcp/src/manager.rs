use crate::{
    client::{McpClient, McpRemoteTool, SseMcpClient, StdioMcpClient},
    hub::{
        McpBootstrapFailure, McpBootstrapResult, McpClientHub, McpRuntimeHandles, McpToolDescriptor,
    },
};
use async_trait::async_trait;
use klaw_config::{McpConfig, McpServerConfig, McpServerMode};
use klaw_tool::ToolRegistry;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{watch, Mutex},
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tracing::{info, warn};

pub struct McpManager;

impl McpManager {
    pub async fn bootstrap(config: &McpConfig) -> McpBootstrapResult {
        let factory: Arc<dyn McpClientFactory> = Arc::new(RealMcpClientFactory);
        bootstrap_with_factory(config, factory).await
    }

    pub fn spawn_bootstrap(config: McpConfig, tools: ToolRegistry) -> McpBootstrapHandle {
        let factory: Arc<dyn McpClientFactory> = Arc::new(RealMcpClientFactory);
        spawn_bootstrap_with_factory(config, tools, factory)
    }
}

#[derive(Debug, Error)]
pub enum McpBootstrapError {
    #[error("bootstrap timed out after {timeout_seconds}s")]
    Timeout { timeout_seconds: u64 },
    #[error("{0}")]
    Other(String),
}

struct ServerBootstrapOk {
    index: usize,
    server_id: String,
    mode: McpServerMode,
    client: Arc<dyn McpClient>,
    tools: Vec<McpRemoteTool>,
}

struct ServerBootstrapErr {
    reason: String,
    stderr_tail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct McpBootstrapSummary {
    pub active_servers: Vec<String>,
    pub required_stdio_servers: Vec<String>,
    pub active_stdio_servers: Vec<String>,
    pub tool_count: usize,
    pub failures: Vec<McpBootstrapFailure>,
}

#[derive(Debug, Clone)]
enum McpBootstrapState {
    Pending,
    Ready(McpBootstrapSummary),
}

pub struct McpBootstrapHandle {
    receiver: watch::Receiver<McpBootstrapState>,
    task: JoinHandle<()>,
    hub: Arc<Mutex<Option<Arc<McpClientHub>>>>,
}

impl McpBootstrapHandle {
    pub async fn wait_until_ready(&mut self) -> Result<McpBootstrapSummary, McpBootstrapError> {
        loop {
            let state = self.receiver.borrow().clone();
            match state {
                McpBootstrapState::Pending => {
                    if self.receiver.changed().await.is_err() {
                        return Err(McpBootstrapError::Other(
                            "bootstrap background task terminated unexpectedly".to_string(),
                        ));
                    }
                }
                McpBootstrapState::Ready(summary) => return Ok(summary),
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(*self.receiver.borrow(), McpBootstrapState::Ready(_))
    }

    pub async fn shutdown(&mut self) -> Result<(), McpBootstrapError> {
        if !self.is_ready() {
            self.task.abort();
            return Ok(());
        }

        let hub = self.hub.lock().await.clone();
        if let Some(hub) = hub {
            hub.shutdown_all()
                .await
                .map_err(|err| McpBootstrapError::Other(err.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for McpBootstrapHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[async_trait]
trait McpClientFactory: Send + Sync {
    async fn create_client(
        &self,
        server: &McpServerConfig,
    ) -> Result<Arc<dyn McpClient>, McpBootstrapError>;
}

struct RealMcpClientFactory;

#[async_trait]
impl McpClientFactory for RealMcpClientFactory {
    async fn create_client(
        &self,
        server: &McpServerConfig,
    ) -> Result<Arc<dyn McpClient>, McpBootstrapError> {
        match server.mode {
            McpServerMode::Stdio => {
                let client = StdioMcpClient::new(server)
                    .await
                    .map_err(|err| McpBootstrapError::Other(err.to_string()))?;
                Ok(Arc::new(client))
            }
            McpServerMode::Sse => {
                let client = SseMcpClient::new(server)
                    .map_err(|err| McpBootstrapError::Other(err.to_string()))?;
                Ok(Arc::new(client))
            }
        }
    }
}

async fn bootstrap_with_factory(
    config: &McpConfig,
    factory: Arc<dyn McpClientFactory>,
) -> McpBootstrapResult {
    if !config.enabled {
        return McpBootstrapResult {
            descriptors: Vec::new(),
            hub: McpClientHub::default(),
            runtime_handles: McpRuntimeHandles::default(),
            failures: Vec::new(),
        };
    }

    let enabled_servers: Vec<(usize, McpServerConfig)> = config
        .servers
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, server)| server.enabled)
        .collect();

    let mut join_set = JoinSet::new();
    for (index, server) in enabled_servers {
        let timeout_seconds = config.startup_timeout_seconds;
        let server_id = server.id.clone();
        let mode = server.mode.clone();
        info!(
            server = %server_id,
            mode = ?mode,
            timeout_seconds,
            status = "pending",
            "starting mcp server bootstrap"
        );
        let factory = Arc::clone(&factory);
        join_set.spawn(async move {
            let fut = async {
                let client = match factory.create_client(&server).await {
                    Ok(client) => client,
                    Err(err) => {
                        return Err(ServerBootstrapErr {
                            reason: err.to_string(),
                            stderr_tail: None,
                        });
                    }
                };
                if let Err(err) = client.initialize().await {
                    return Err(ServerBootstrapErr {
                        reason: err.to_string(),
                        stderr_tail: client.stderr_tail().await,
                    });
                }
                let tools = match client.list_tools().await {
                    Ok(tools) => tools,
                    Err(err) => {
                        return Err(ServerBootstrapErr {
                            reason: err.to_string(),
                            stderr_tail: client.stderr_tail().await,
                        });
                    }
                };
                Ok::<ServerBootstrapOk, ServerBootstrapErr>(ServerBootstrapOk {
                    index,
                    server_id: server.id.clone(),
                    mode: server.mode,
                    client,
                    tools,
                })
            };
            match timeout(Duration::from_secs(timeout_seconds), fut).await {
                Ok(outcome) => (server_id, outcome),
                Err(_) => (
                    server_id,
                    Err(ServerBootstrapErr {
                        reason: McpBootstrapError::Timeout { timeout_seconds }.to_string(),
                        stderr_tail: None,
                    }),
                ),
            }
        });
    }

    let mut oks = Vec::new();
    let mut failures = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((_server_id, Ok(ok))) => {
                info!(
                    server = %ok.server_id,
                    mode = ?ok.mode,
                    tool_count = ok.tools.len(),
                    status = "ready",
                    "mcp server bootstrap completed"
                );
                oks.push(ok);
            }
            Ok((server_id, Err(err))) => {
                let reason = format_failure_reason(&err.reason, err.stderr_tail.as_deref());
                warn!(
                    server = %server_id,
                    reason = %reason,
                    stderr_tail = err.stderr_tail.as_deref().unwrap_or(""),
                    status = "failed",
                    "mcp server bootstrap failed"
                );
                failures.push(McpBootstrapFailure { server_id, reason });
            }
            Err(err) => {
                warn!(
                    server = "<join-task>",
                    reason = %err,
                    status = "failed",
                    "mcp server bootstrap join failed"
                );
                failures.push(McpBootstrapFailure {
                    server_id: "<join-task>".to_string(),
                    reason: format!("join error: {err}"),
                });
            }
        }
    }

    oks.sort_by_key(|item| item.index);
    let mut descriptors = Vec::new();
    let mut hub = McpClientHub::default();
    let mut runtime_handles = McpRuntimeHandles::default();
    let mut seen_tool_names = BTreeSet::new();

    for item in oks {
        let has_conflict = item
            .tools
            .iter()
            .any(|tool| seen_tool_names.contains(&tool.name));
        if has_conflict {
            warn!(
                server = %item.server_id,
                status = "failed",
                reason = "tool name conflicts with another MCP server",
                "mcp server bootstrap rejected after discovery"
            );
            failures.push(McpBootstrapFailure {
                server_id: item.server_id,
                reason: "tool name conflicts with another MCP server".to_string(),
            });
            continue;
        }

        for tool in &item.tools {
            seen_tool_names.insert(tool.name.clone());
            descriptors.push(McpToolDescriptor {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
                server_id: item.server_id.clone(),
                tool_name: tool.name.clone(),
            });
        }
        if item.mode == McpServerMode::Stdio {
            runtime_handles.stdio_servers.push(item.server_id.clone());
        }
        hub.insert(item.server_id, item.client);
    }

    McpBootstrapResult {
        descriptors,
        hub,
        runtime_handles,
        failures,
    }
}

fn format_failure_reason(reason: &str, stderr_tail: Option<&str>) -> String {
    match stderr_tail {
        Some(stderr_tail) if !stderr_tail.trim().is_empty() => {
            format!("{reason}; stderr: {}", stderr_tail.replace('\n', " | "))
        }
        _ => reason.to_string(),
    }
}

fn spawn_bootstrap_with_factory(
    config: McpConfig,
    mut tools: ToolRegistry,
    factory: Arc<dyn McpClientFactory>,
) -> McpBootstrapHandle {
    let (sender, receiver) = watch::channel(McpBootstrapState::Pending);
    let hub_state = Arc::new(Mutex::new(None));
    let hub_state_for_task = Arc::clone(&hub_state);
    let task = tokio::spawn(async move {
        let bootstrap = bootstrap_with_factory(&config, factory).await;
        let active_servers = bootstrap.hub.server_ids();
        let active_stdio_servers = bootstrap.runtime_handles.stdio_servers.clone();
        let required_stdio_servers: Vec<String> = config
            .servers
            .iter()
            .filter(|server| server.enabled && server.mode == McpServerMode::Stdio)
            .map(|server| server.id.clone())
            .collect();

        if bootstrap.descriptors.is_empty() {
            if !bootstrap.failures.is_empty() {
                for failure in &bootstrap.failures {
                    warn!(
                        server = %failure.server_id,
                        reason = %failure.reason,
                        "mcp server bootstrap failed"
                    );
                }
            }
            info!(
                active_servers = ?active_servers,
                required_stdio_servers = ?required_stdio_servers,
                active_stdio_servers = ?active_stdio_servers,
                failed_servers = bootstrap.failures.len(),
                tool_count = 0,
                "mcp bootstrap summary"
            );
            let _ = sender.send(McpBootstrapState::Ready(McpBootstrapSummary {
                active_servers,
                required_stdio_servers,
                active_stdio_servers,
                tool_count: 0,
                failures: bootstrap.failures,
            }));
            return;
        }

        let mut blocked_servers = BTreeSet::new();
        let mut existing_names: BTreeSet<String> = tools.list().into_iter().collect();
        let mut by_server: BTreeMap<String, Vec<McpToolDescriptor>> = BTreeMap::new();
        for descriptor in bootstrap.descriptors {
            by_server
                .entry(descriptor.server_id.clone())
                .or_default()
                .push(descriptor);
        }

        let mut failures = bootstrap.failures;
        for (server_id, descriptors) in &by_server {
            if descriptors
                .iter()
                .any(|descriptor| existing_names.contains(&descriptor.name))
            {
                blocked_servers.insert(server_id.clone());
                failures.push(McpBootstrapFailure {
                    server_id: server_id.clone(),
                    reason: "tool name conflict with existing registry".to_string(),
                });
                warn!(
                    server = %server_id,
                    "mcp server skipped due to tool name conflict with existing registry"
                );
                continue;
            }
            for descriptor in descriptors {
                existing_names.insert(descriptor.name.clone());
            }
        }

        let hub = Arc::new(bootstrap.hub);
        {
            let mut guard = hub_state_for_task.lock().await;
            *guard = Some(Arc::clone(&hub));
        }
        let mut tool_count = 0usize;
        for (server_id, descriptors) in by_server {
            if blocked_servers.contains(&server_id) {
                continue;
            }
            for descriptor in descriptors {
                tools.register_shared(Arc::new(crate::McpProxyTool::new(
                    descriptor,
                    Arc::clone(&hub),
                )));
                tool_count += 1;
            }
        }

        info!(
            active_servers = ?active_servers,
            required_stdio_servers = ?required_stdio_servers,
            active_stdio_servers = ?active_stdio_servers,
            failed_servers = failures.len(),
            server_count = active_servers.len(),
            tool_count,
            "mcp bootstrap summary"
        );

        let _ = sender.send(McpBootstrapState::Ready(McpBootstrapSummary {
            active_servers,
            required_stdio_servers,
            active_stdio_servers,
            tool_count,
            failures,
        }));
    });

    McpBootstrapHandle {
        receiver,
        task,
        hub: hub_state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::McpClientError;
    use klaw_tool::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolRegistry};
    use serde_json::{json, Value};
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    struct MockClient {
        list_tools: Vec<McpRemoteTool>,
    }

    #[async_trait]
    impl McpClient for MockClient {
        async fn initialize(&self) -> Result<(), McpClientError> {
            Ok(())
        }

        async fn list_tools(&self) -> Result<Vec<McpRemoteTool>, McpClientError> {
            Ok(self.list_tools.clone())
        }

        async fn call_tool(
            &self,
            _tool_name: &str,
            _arguments: Value,
        ) -> Result<Value, McpClientError> {
            Ok(json!({"content":[{"type":"text","text":"ok"}]}))
        }
    }

    struct MockFactory {
        delay_ms: u64,
        calls: Arc<AtomicUsize>,
    }

    struct BuiltinTool;

    #[async_trait]
    impl Tool for BuiltinTool {
        fn name(&self) -> &str {
            "builtin"
        }

        fn description(&self) -> &str {
            "builtin"
        }

        fn parameters(&self) -> Value {
            json!({"type":"object"})
        }

        fn category(&self) -> ToolCategory {
            ToolCategory::FilesystemRead
        }

        async fn execute(&self, _args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput {
                content_for_model: "ok".to_string(),
                content_for_user: None,
            })
        }
    }

    #[async_trait]
    impl McpClientFactory for MockFactory {
        async fn create_client(
            &self,
            server: &McpServerConfig,
        ) -> Result<Arc<dyn McpClient>, McpBootstrapError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }
            if server.id == "bad" {
                return Err(McpBootstrapError::Other("boom".to_string()));
            }
            let tool_name = if server.id == "s2" {
                "same"
            } else {
                &server.id
            };
            let tools = vec![McpRemoteTool {
                name: if server.id == "s1" { "same" } else { tool_name }.to_string(),
                description: "d".to_string(),
                parameters: json!({"type":"object"}),
            }];
            Ok(Arc::new(MockClient { list_tools: tools }))
        }
    }

    fn server(id: &str) -> McpServerConfig {
        McpServerConfig {
            id: id.to_string(),
            enabled: true,
            mode: McpServerMode::Stdio,
            command: Some("echo".to_string()),
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: None,
            headers: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn bootstrap_degrades_on_partial_failures() {
        let calls = Arc::new(AtomicUsize::new(0));
        let factory = MockFactory {
            delay_ms: 0,
            calls: Arc::clone(&calls),
        };
        let cfg = McpConfig {
            enabled: true,
            startup_timeout_seconds: 30,
            servers: vec![server("ok"), server("bad")],
        };
        let out = bootstrap_with_factory(&cfg, Arc::new(factory)).await;
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(out.descriptors.len(), 1);
        assert_eq!(out.hub.server_ids(), vec!["ok".to_string()]);
        assert_eq!(out.failures.len(), 1);
    }

    #[tokio::test]
    async fn bootstrap_enforces_timeout() {
        let factory = MockFactory {
            delay_ms: 200,
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let cfg = McpConfig {
            enabled: true,
            startup_timeout_seconds: 0,
            servers: vec![server("slow")],
        };
        let out = bootstrap_with_factory(&cfg, Arc::new(factory)).await;
        assert!(out.descriptors.is_empty());
        assert_eq!(out.failures.len(), 1);
        assert!(out.failures[0].reason.contains("timed out"));
    }

    #[tokio::test]
    async fn bootstrap_rejects_conflicting_tool_names_between_servers() {
        let factory = MockFactory {
            delay_ms: 0,
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let cfg = McpConfig {
            enabled: true,
            startup_timeout_seconds: 30,
            servers: vec![server("s1"), server("s2")],
        };
        let out = bootstrap_with_factory(&cfg, Arc::new(factory)).await;
        assert_eq!(out.descriptors.len(), 1);
        assert_eq!(out.hub.server_ids(), vec!["s1".to_string()]);
        assert_eq!(out.failures.len(), 1);
        assert!(out.failures[0].reason.contains("conflicts"));
    }

    #[tokio::test]
    async fn spawned_bootstrap_registers_tools_into_shared_registry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let factory = MockFactory {
            delay_ms: 20,
            calls,
        };
        let cfg = McpConfig {
            enabled: true,
            startup_timeout_seconds: 30,
            servers: vec![server("ok")],
        };
        let mut tools = ToolRegistry::default();
        tools.register(BuiltinTool);
        let runtime_view = tools.clone();

        let mut handle = spawn_bootstrap_with_factory(cfg, tools, Arc::new(factory));
        assert!(!handle.is_ready());
        let summary = handle.wait_until_ready().await.unwrap_or_else(|err| {
            panic!("bootstrap should succeed: {err}");
        });

        assert_eq!(summary.active_servers, vec!["ok".to_string()]);
        assert_eq!(summary.required_stdio_servers, vec!["ok".to_string()]);
        assert_eq!(summary.active_stdio_servers, vec!["ok".to_string()]);
        assert_eq!(summary.tool_count, 1);
        assert!(runtime_view.get("ok").is_some());
        assert!(runtime_view.get("builtin").is_some());
    }

    #[tokio::test]
    async fn spawned_bootstrap_reports_failures_after_completion() {
        let calls = Arc::new(AtomicUsize::new(0));
        let factory = MockFactory { delay_ms: 0, calls };
        let cfg = McpConfig {
            enabled: true,
            startup_timeout_seconds: 30,
            servers: vec![server("bad")],
        };

        let mut handle =
            spawn_bootstrap_with_factory(cfg, ToolRegistry::default(), Arc::new(factory));
        let summary = handle.wait_until_ready().await.unwrap_or_else(|err| {
            panic!("bootstrap should complete: {err}");
        });

        assert!(summary.active_servers.is_empty());
        assert_eq!(summary.required_stdio_servers, vec!["bad".to_string()]);
        assert!(summary.active_stdio_servers.is_empty());
        assert_eq!(summary.failures.len(), 1);
        assert_eq!(summary.failures[0].server_id, "bad");
    }
}
