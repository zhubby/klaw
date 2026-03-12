use crate::{
    client::{McpClient, McpRemoteTool, SseMcpClient, StdioMcpClient},
    hub::{
        McpBootstrapFailure, McpBootstrapResult, McpClientHub, McpRuntimeHandles, McpToolDescriptor,
    },
};
use async_trait::async_trait;
use klaw_config::{McpConfig, McpServerConfig, McpServerMode};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{task::JoinSet, time::timeout};

pub struct McpManager;

impl McpManager {
    pub async fn bootstrap(config: &McpConfig) -> McpBootstrapResult {
        let factory: Arc<dyn McpClientFactory> = Arc::new(RealMcpClientFactory);
        bootstrap_with_factory(config, factory).await
    }
}

#[derive(Debug, Error)]
enum McpBootstrapError {
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
        let factory = Arc::clone(&factory);
        join_set.spawn(async move {
            let fut = async {
                let client = factory.create_client(&server).await?;
                client
                    .initialize()
                    .await
                    .map_err(|err| McpBootstrapError::Other(err.to_string()))?;
                let tools = client
                    .list_tools()
                    .await
                    .map_err(|err| McpBootstrapError::Other(err.to_string()))?;
                Ok::<ServerBootstrapOk, McpBootstrapError>(ServerBootstrapOk {
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
                    Err(McpBootstrapError::Timeout { timeout_seconds }),
                ),
            }
        });
    }

    let mut oks = Vec::new();
    let mut failures = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((_server_id, Ok(ok))) => oks.push(ok),
            Ok((server_id, Err(err))) => failures.push(McpBootstrapFailure {
                server_id,
                reason: err.to_string(),
            }),
            Err(err) => failures.push(McpBootstrapFailure {
                server_id: "<join-task>".to_string(),
                reason: format!("join error: {err}"),
            }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::McpClientError;
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
}
