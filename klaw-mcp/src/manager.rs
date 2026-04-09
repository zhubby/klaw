use crate::{
    client::{McpClient, McpRemoteTool, SseMcpClient, StdioMcpClient},
    hub::{McpClientHub, McpToolDescriptor},
};
use klaw_config::{McpConfig, McpServerConfig, McpServerMode};
use klaw_tool::ToolRegistry;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
    time::timeout,
};
use tracing::{info, warn};

const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct McpServerKey(String);

impl McpServerKey {
    #[must_use]
    pub fn new(id: impl AsRef<str>) -> Self {
        Self(id.as_ref().trim().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for McpServerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for McpServerKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpLifecycleState {
    Starting,
    Running,
    Stopped,
    Failed,
}

impl McpLifecycleState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerStatus {
    pub key: McpServerKey,
    pub mode: McpServerMode,
    pub enabled: bool,
    pub state: McpLifecycleState,
    pub last_error: Option<String>,
    pub tool_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpServerDetail {
    pub key: McpServerKey,
    pub tools_list_response: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct McpRuntimeSnapshot {
    pub statuses: Vec<McpServerStatus>,
    pub details: Vec<McpServerDetail>,
}

impl McpServerStatus {
    fn from_config(
        config: &McpServerConfig,
        state: McpLifecycleState,
        last_error: Option<String>,
        tool_count: usize,
    ) -> Self {
        Self {
            key: McpServerKey::new(&config.id),
            mode: config.mode.clone(),
            enabled: config.enabled,
            state,
            last_error,
            tool_count,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpConfigSnapshot {
    pub startup_timeout_seconds: u64,
    pub servers: Vec<McpServerConfig>,
}

impl McpConfigSnapshot {
    pub fn from_mcp_config(config: &McpConfig) -> Self {
        Self {
            startup_timeout_seconds: config.startup_timeout_seconds,
            servers: config.servers.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpSyncResult {
    pub keep: Vec<McpServerKey>,
    pub start: Vec<McpServerKey>,
    pub restart: Vec<McpServerKey>,
    pub stop: Vec<McpServerKey>,
    pub statuses: Vec<McpServerStatus>,
    pub active_servers: Vec<String>,
    pub tool_count: usize,
}

#[derive(Debug, Error)]
pub enum McpBootstrapError {
    #[error("init timed out after {timeout_seconds}s")]
    Timeout { timeout_seconds: u64 },
    #[error("{0}")]
    Other(String),
}

struct McpServerHandle {
    config: McpServerConfig,
    client: Arc<dyn McpClient>,
    tool_names: Vec<String>,
}

#[derive(Debug, Clone)]
enum McpInitState {
    Pending,
    Ready(McpSyncResult),
}

pub struct McpInitHandle {
    receiver: watch::Receiver<McpInitState>,
    task: JoinHandle<()>,
    manager: Arc<Mutex<McpManager>>,
}

impl McpInitHandle {
    pub async fn wait_until_ready(&mut self) -> Result<McpSyncResult, McpBootstrapError> {
        loop {
            let state = self.receiver.borrow().clone();
            match state {
                McpInitState::Pending => {
                    if self.receiver.changed().await.is_err() {
                        return Err(McpBootstrapError::Other(
                            "init background task terminated unexpectedly".to_string(),
                        ));
                    }
                }
                McpInitState::Ready(result) => return Ok(result),
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(*self.receiver.borrow(), McpInitState::Ready(_))
    }

    pub fn manager(&self) -> Arc<Mutex<McpManager>> {
        Arc::clone(&self.manager)
    }
}

impl Drop for McpInitHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub struct McpManager {
    tools: ToolRegistry,
    hub: McpClientHub,
    servers: BTreeMap<McpServerKey, McpServerHandle>,
    statuses: Arc<StdMutex<BTreeMap<McpServerKey, McpServerStatus>>>,
    details: Arc<StdMutex<BTreeMap<McpServerKey, McpServerDetail>>>,
    config: McpConfigSnapshot,
}

impl McpManager {
    pub fn new(tools: ToolRegistry) -> Self {
        Self {
            tools,
            hub: McpClientHub::default(),
            servers: BTreeMap::new(),
            statuses: Arc::new(StdMutex::new(BTreeMap::new())),
            details: Arc::new(StdMutex::new(BTreeMap::new())),
            config: McpConfigSnapshot::default(),
        }
    }

    pub fn spawn_init(tools: ToolRegistry, config: McpConfigSnapshot) -> McpInitHandle {
        let manager = Arc::new(Mutex::new(Self::new(tools)));
        let manager_for_task = Arc::clone(&manager);
        let (sender, receiver) = watch::channel(McpInitState::Pending);

        let task = tokio::spawn(async move {
            let mut guard = manager_for_task.lock().await;
            let result = guard.do_init(&config).await;
            let _ = sender.send(McpInitState::Ready(result));
        });

        McpInitHandle {
            receiver,
            task,
            manager,
        }
    }

    pub async fn sync(&mut self, snapshot: McpConfigSnapshot) -> McpSyncResult {
        let current = self
            .servers
            .iter()
            .map(|(key, handle)| (key.clone(), handle.config.clone()))
            .collect::<BTreeMap<_, _>>();
        let plan = plan_server_updates(&current, &snapshot);
        self.config = snapshot.clone();

        for key in &plan.stop {
            self.stop_server(key).await;
        }

        for config in &plan.restart {
            self.stop_server(&McpServerKey::new(&config.id)).await;
        }

        for config in plan.start.iter().chain(plan.restart.iter()) {
            self.start_server(config.clone()).await;
        }

        self.reconcile_statuses(&snapshot);

        let active_servers: Vec<String> = self
            .servers
            .keys()
            .map(|k| k.as_str().to_string())
            .collect();
        let tool_count: usize = self.servers.values().map(|h| h.tool_names.len()).sum();

        McpSyncResult {
            keep: plan.keep,
            start: plan
                .start
                .into_iter()
                .map(|c| McpServerKey::new(&c.id))
                .collect(),
            restart: plan
                .restart
                .into_iter()
                .map(|c| McpServerKey::new(&c.id))
                .collect(),
            stop: plan.stop,
            statuses: self.snapshot_statuses(&snapshot),
            active_servers,
            tool_count,
        }
    }

    pub async fn shutdown_all(&mut self) {
        let keys = self.servers.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            self.stop_server(&key).await;
        }
    }

    pub async fn restart_server(
        &mut self,
        key: &McpServerKey,
        snapshot: &McpConfigSnapshot,
    ) -> Result<McpRuntimeSnapshot, String> {
        let Some(config) = snapshot
            .servers
            .iter()
            .find(|config| McpServerKey::new(&config.id) == *key)
            .cloned()
        else {
            return Err(format!("mcp server '{}' not found in config", key.as_str()));
        };
        if config.mode != McpServerMode::Stdio {
            return Err(format!(
                "mcp server '{}' only supports restart in stdio mode",
                key.as_str()
            ));
        }
        if !config.enabled {
            return Err(format!(
                "mcp server '{}' is disabled and cannot be restarted",
                key.as_str()
            ));
        }

        self.config = snapshot.clone();
        self.stop_server(key).await;
        self.start_server(config).await;
        self.reconcile_statuses(snapshot);
        Ok(self.runtime_snapshot(snapshot))
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<McpServerStatus> {
        self.statuses
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .values()
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn runtime_snapshot(&self, snapshot: &McpConfigSnapshot) -> McpRuntimeSnapshot {
        let statuses = self.snapshot_statuses(snapshot);
        let detail_guard = self.details.lock().unwrap_or_else(|err| err.into_inner());
        let details = snapshot
            .servers
            .iter()
            .map(|config| {
                detail_guard
                    .get(&McpServerKey::new(&config.id))
                    .cloned()
                    .unwrap_or_else(|| McpServerDetail {
                        key: McpServerKey::new(&config.id),
                        tools_list_response: None,
                    })
            })
            .collect();

        McpRuntimeSnapshot { statuses, details }
    }

    async fn do_init(&mut self, config: &McpConfigSnapshot) -> McpSyncResult {
        self.sync(config.clone()).await
    }

    async fn start_server(&mut self, config: McpServerConfig) {
        let key = McpServerKey::new(&config.id);
        {
            let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
            guard.insert(
                key.clone(),
                McpServerStatus::from_config(&config, McpLifecycleState::Starting, None, 0),
            );
        }

        let timeout_seconds = self.config.startup_timeout_seconds;
        let start_result = async {
            let client = match create_client(&config).await {
                Ok(client) => client,
                Err(err) => return Err((err.to_string(), None)),
            };
            if let Err(err) = client.initialize().await {
                return Err((err.to_string(), client.stderr_tail().await));
            }
            match client.list_tools().await {
                Ok(tools) => Ok((client, tools)),
                Err(err) => Err((err.to_string(), client.stderr_tail().await)),
            }
        };

        let result = timeout(Duration::from_secs(timeout_seconds), start_result).await;

        match result {
            Ok(Ok((client, tools))) => {
                self.set_tools_list_detail(&config.id, &tools);
                let tool_names: Vec<String> = tools.iter().map(|t| t.name.clone()).collect();
                self.hub.insert(config.id.clone(), Arc::clone(&client));

                for tool in &tools {
                    let descriptor = McpToolDescriptor {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.parameters.clone(),
                        server_id: config.id.clone(),
                        tool_name: tool.name.clone(),
                    };
                    self.tools
                        .register_shared(Arc::new(crate::McpProxyTool::new(
                            descriptor,
                            self.hub.clone(),
                        )));
                }

                {
                    let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
                    guard.insert(
                        key.clone(),
                        McpServerStatus::from_config(
                            &config,
                            McpLifecycleState::Running,
                            None,
                            tool_names.len(),
                        ),
                    );
                }

                self.servers.insert(
                    key,
                    McpServerHandle {
                        config: config.clone(),
                        client,
                        tool_names,
                    },
                );

                info!(
                    server = %config.id,
                    mode = ?config.mode,
                    status = "running",
                    "mcp server started"
                );
            }
            Ok(Err((reason, stderr))) => {
                self.clear_tools_list_detail(&config.id);
                let message = format_failure_reason(&reason, stderr.as_deref());
                warn!(
                    server = %config.id,
                    reason = %message,
                    status = "failed",
                    "mcp server failed to start"
                );
                let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
                guard.insert(
                    key,
                    McpServerStatus::from_config(
                        &config,
                        McpLifecycleState::Failed,
                        Some(message),
                        0,
                    ),
                );
            }
            Err(_) => {
                self.clear_tools_list_detail(&config.id);
                let message = format!("timeout after {timeout_seconds}s");
                warn!(
                    server = %config.id,
                    reason = %message,
                    status = "failed",
                    "mcp server failed to start"
                );
                let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
                guard.insert(
                    key,
                    McpServerStatus::from_config(
                        &config,
                        McpLifecycleState::Failed,
                        Some(message),
                        0,
                    ),
                );
            }
        }
    }

    async fn stop_server(&mut self, key: &McpServerKey) {
        let Some(handle) = self.servers.remove(key) else {
            return;
        };

        let tool_names: Vec<&str> = handle.tool_names.iter().map(String::as_str).collect();
        self.tools.unregister_many(&tool_names);
        self.clear_tools_list_detail(key.as_str());

        if handle.config.mode == McpServerMode::Stdio {
            let client = Arc::clone(&handle.client);
            let key_for_log = key.clone();
            if let Err(err) = timeout(SERVER_SHUTDOWN_TIMEOUT, client.shutdown()).await {
                warn!(
                    server = %key_for_log,
                    error = %err,
                    "mcp server shutdown timed out"
                );
            } else {
                info!(
                    server = %key,
                    mode = "stdio",
                    status = "stopped",
                    "mcp server stopped"
                );
            }
        } else {
            info!(
                server = %key,
                mode = "sse",
                status = "stopped",
                "mcp server stopped"
            );
        }

        self.hub.remove(key.as_str());

        {
            let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
            guard.insert(
                key.clone(),
                McpServerStatus::from_config(&handle.config, McpLifecycleState::Stopped, None, 0),
            );
        }
    }

    fn reconcile_statuses(&mut self, snapshot: &McpConfigSnapshot) {
        let desired_keys: BTreeSet<McpServerKey> = snapshot
            .servers
            .iter()
            .map(|s| McpServerKey::new(&s.id))
            .collect();

        let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        guard.retain(|key, _| desired_keys.contains(key));
        drop(guard);

        let mut detail_guard = self.details.lock().unwrap_or_else(|err| err.into_inner());
        detail_guard.retain(|key, _| desired_keys.contains(key));
        drop(detail_guard);

        let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());

        for config in &snapshot.servers {
            let key = McpServerKey::new(&config.id);
            match guard.get_mut(&key) {
                Some(status) => {
                    status.enabled = config.enabled;
                    status.mode = config.mode.clone();
                    if !config.enabled {
                        status.state = McpLifecycleState::Stopped;
                        status.last_error = None;
                    }
                }
                None => {
                    guard.insert(
                        key,
                        McpServerStatus::from_config(config, McpLifecycleState::Stopped, None, 0),
                    );
                }
            }
        }
    }

    fn snapshot_statuses(&self, snapshot: &McpConfigSnapshot) -> Vec<McpServerStatus> {
        let guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        snapshot
            .servers
            .iter()
            .map(|config| {
                guard
                    .get(&McpServerKey::new(&config.id))
                    .cloned()
                    .unwrap_or_else(|| {
                        McpServerStatus::from_config(config, McpLifecycleState::Stopped, None, 0)
                    })
            })
            .collect()
    }

    fn set_tools_list_detail(&self, server_id: &str, tools: &[McpRemoteTool]) {
        let mut guard = self.details.lock().unwrap_or_else(|err| err.into_inner());
        guard.insert(
            McpServerKey::new(server_id),
            McpServerDetail {
                key: McpServerKey::new(server_id),
                tools_list_response: Some(json!({
                    "tools": tools
                        .iter()
                        .map(|tool| json!({
                            "name": tool.name.clone(),
                            "description": tool.description.clone(),
                            "inputSchema": tool.parameters.clone(),
                        }))
                        .collect::<Vec<_>>()
                })),
            },
        );
    }

    fn clear_tools_list_detail(&self, server_id: &str) {
        let mut guard = self.details.lock().unwrap_or_else(|err| err.into_inner());
        guard.remove(&McpServerKey::new(server_id));
    }
}

async fn create_client(server: &McpServerConfig) -> Result<Arc<dyn McpClient>, McpBootstrapError> {
    match server.mode {
        McpServerMode::Stdio => {
            let client = StdioMcpClient::new(server)
                .await
                .map_err(|err| McpBootstrapError::Other(err.to_string()))?;
            let client: Arc<dyn McpClient> = Arc::new(client);
            Ok(client)
        }
        McpServerMode::Sse => {
            let client = SseMcpClient::new(server)
                .map_err(|err| McpBootstrapError::Other(err.to_string()))?;
            let client: Arc<dyn McpClient> = Arc::new(client);
            Ok(client)
        }
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

#[derive(Debug, Default)]
struct McpSyncPlan {
    keep: Vec<McpServerKey>,
    start: Vec<McpServerConfig>,
    restart: Vec<McpServerConfig>,
    stop: Vec<McpServerKey>,
}

fn plan_server_updates(
    current: &BTreeMap<McpServerKey, McpServerConfig>,
    desired: &McpConfigSnapshot,
) -> McpSyncPlan {
    let desired_enabled: BTreeMap<McpServerKey, McpServerConfig> = desired
        .servers
        .iter()
        .filter(|config| config.enabled)
        .map(|config| (McpServerKey::new(&config.id), config.clone()))
        .collect();

    let mut plan = McpSyncPlan::default();

    for key in current.keys() {
        if !desired_enabled.contains_key(key) {
            plan.stop.push(key.clone());
        }
    }

    for (key, desired_config) in desired_enabled {
        match current.get(&key) {
            Some(current_config) if current_config == &desired_config => {
                plan.keep.push(key);
            }
            Some(_) => {
                plan.restart.push(desired_config);
            }
            None => {
                plan.start.push(desired_config);
            }
        }
    }

    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{McpConfig, McpServerConfig, McpServerMode};
    use std::collections::BTreeMap;

    fn server(id: &str, mode: McpServerMode, enabled: bool) -> McpServerConfig {
        let is_stdio = mode == McpServerMode::Stdio;
        McpServerConfig {
            id: id.to_string(),
            enabled,
            mode,
            command: if is_stdio {
                Some("echo".to_string())
            } else {
                None
            },
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: if !is_stdio {
                Some("https://example.com/sse".to_string())
            } else {
                None
            },
            headers: BTreeMap::new(),
        }
    }

    #[test]
    fn mcp_server_key_new_trims_whitespace() {
        let key = McpServerKey::new("  test  ");
        assert_eq!(key.as_str(), "test");
    }

    #[test]
    fn mcp_lifecycle_state_as_str() {
        assert_eq!(McpLifecycleState::Starting.as_str(), "starting");
        assert_eq!(McpLifecycleState::Running.as_str(), "running");
        assert_eq!(McpLifecycleState::Stopped.as_str(), "stopped");
        assert_eq!(McpLifecycleState::Failed.as_str(), "failed");
    }

    #[test]
    fn mcp_config_snapshot_from_config() {
        let config = McpConfig {
            startup_timeout_seconds: 30,
            servers: vec![server("test", McpServerMode::Stdio, true)],
        };
        let snapshot = McpConfigSnapshot::from_mcp_config(&config);
        assert_eq!(snapshot.startup_timeout_seconds, 30);
        assert_eq!(snapshot.servers.len(), 1);
    }

    #[test]
    fn plan_updates_empty_to_empty() {
        let current = BTreeMap::new();
        let desired = McpConfigSnapshot::default();
        let plan = plan_server_updates(&current, &desired);
        assert!(plan.keep.is_empty());
        assert!(plan.start.is_empty());
        assert!(plan.restart.is_empty());
        assert!(plan.stop.is_empty());
    }

    #[test]
    fn plan_updates_starts_new_servers() {
        let current = BTreeMap::new();
        let desired = McpConfigSnapshot {
            startup_timeout_seconds: 30,
            servers: vec![server("new", McpServerMode::Stdio, true)],
        };
        let plan = plan_server_updates(&current, &desired);
        assert!(plan.keep.is_empty());
        assert_eq!(plan.start.len(), 1);
        assert!(plan.restart.is_empty());
        assert!(plan.stop.is_empty());
    }

    #[test]
    fn plan_updates_stops_removed_servers() {
        let mut current = BTreeMap::new();
        current.insert(
            McpServerKey::new("old"),
            server("old", McpServerMode::Stdio, true),
        );
        let desired = McpConfigSnapshot::default();
        let plan = plan_server_updates(&current, &desired);
        assert!(plan.keep.is_empty());
        assert!(plan.start.is_empty());
        assert!(plan.restart.is_empty());
        assert_eq!(plan.stop.len(), 1);
    }

    #[test]
    fn plan_updates_restarts_changed_servers() {
        let mut current = BTreeMap::new();
        current.insert(
            McpServerKey::new("test"),
            server("test", McpServerMode::Stdio, true),
        );
        let desired = McpConfigSnapshot {
            startup_timeout_seconds: 30,
            servers: vec![server("test", McpServerMode::Sse, true)],
        };
        let plan = plan_server_updates(&current, &desired);
        assert!(plan.keep.is_empty());
        assert!(plan.start.is_empty());
        assert_eq!(plan.restart.len(), 1);
        assert!(plan.stop.is_empty());
    }

    #[test]
    fn plan_updates_keeps_unchanged_servers() {
        let config = server("test", McpServerMode::Stdio, true);
        let mut current = BTreeMap::new();
        current.insert(McpServerKey::new("test"), config.clone());
        let desired = McpConfigSnapshot {
            startup_timeout_seconds: 30,
            servers: vec![config],
        };
        let plan = plan_server_updates(&current, &desired);
        assert_eq!(plan.keep.len(), 1);
        assert!(plan.start.is_empty());
        assert!(plan.restart.is_empty());
        assert!(plan.stop.is_empty());
    }

    #[test]
    fn plan_updates_ignores_disabled_servers() {
        let current = BTreeMap::new();
        let desired = McpConfigSnapshot {
            startup_timeout_seconds: 30,
            servers: vec![server("disabled", McpServerMode::Stdio, false)],
        };
        let plan = plan_server_updates(&current, &desired);
        assert!(plan.start.is_empty());
    }

    #[tokio::test]
    async fn initial_sync_uses_snapshot_timeout_for_fresh_manager() {
        let mut manager = McpManager::new(ToolRegistry::default());
        let snapshot = McpConfigSnapshot {
            startup_timeout_seconds: 1,
            servers: vec![McpServerConfig {
                id: "sleepy".to_string(),
                enabled: true,
                mode: McpServerMode::Stdio,
                command: Some("sleep".to_string()),
                args: vec!["2".to_string()],
                env: BTreeMap::new(),
                cwd: None,
                url: None,
                headers: BTreeMap::new(),
            }],
        };

        let result = manager.sync(snapshot).await;
        let status = result
            .statuses
            .into_iter()
            .find(|status| status.key.as_str() == "sleepy")
            .expect("sync should report sleepy status");

        assert_eq!(status.state, McpLifecycleState::Failed);
        assert_eq!(status.last_error.as_deref(), Some("timeout after 1s"));
    }

    #[tokio::test]
    async fn restart_server_rejects_non_stdio_servers() {
        let mut manager = McpManager::new(ToolRegistry::default());
        let snapshot = McpConfigSnapshot {
            startup_timeout_seconds: 30,
            servers: vec![server("remote", McpServerMode::Sse, true)],
        };

        let err = manager
            .restart_server(&McpServerKey::new("remote"), &snapshot)
            .await
            .expect_err("sse restart should be rejected");

        assert!(err.contains("only supports restart in stdio mode"));
    }

    #[tokio::test]
    async fn restart_server_rejects_disabled_stdio_servers() {
        let mut manager = McpManager::new(ToolRegistry::default());
        let snapshot = McpConfigSnapshot {
            startup_timeout_seconds: 30,
            servers: vec![server("disabled", McpServerMode::Stdio, false)],
        };

        let err = manager
            .restart_server(&McpServerKey::new("disabled"), &snapshot)
            .await
            .expect_err("disabled restart should be rejected");

        assert!(err.contains("is disabled"));
    }
}
