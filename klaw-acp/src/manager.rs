use crate::{
    client::KlawAcpClient,
    hub::AcpAgentHub,
    runtime::{AcpExecutionError, AcpProxyTool, AcpToolDescriptor},
};
use agent_client_protocol as acp;
use klaw_config::{AcpAgentConfig, AcpConfig};
use klaw_tool::ToolRegistry;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use thiserror::Error;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AcpAgentKey(String);

impl AcpAgentKey {
    #[must_use]
    pub fn new(id: impl AsRef<str>) -> Self {
        Self(id.as_ref().trim().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AcpAgentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpLifecycleState {
    Starting,
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentStatus {
    pub key: AcpAgentKey,
    pub enabled: bool,
    pub state: AcpLifecycleState,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcpConfigSnapshot {
    pub startup_timeout_seconds: u64,
    pub agents: Vec<AcpAgentConfig>,
}

impl AcpConfigSnapshot {
    #[must_use]
    pub fn from_config(config: &AcpConfig) -> Self {
        Self {
            startup_timeout_seconds: config.startup_timeout_seconds,
            agents: config.agents.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcpSyncResult {
    pub keep: Vec<AcpAgentKey>,
    pub start: Vec<AcpAgentKey>,
    pub restart: Vec<AcpAgentKey>,
    pub stop: Vec<AcpAgentKey>,
    pub statuses: Vec<AcpAgentStatus>,
    pub active_agents: Vec<String>,
    pub tool_count: usize,
}

#[derive(Debug, Error)]
pub enum AcpBootstrapError {
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
enum AcpInitState {
    Pending,
    Ready(AcpSyncResult),
}

pub struct AcpInitHandle {
    receiver: watch::Receiver<AcpInitState>,
    task: JoinHandle<()>,
    manager: Arc<Mutex<AcpManager>>,
}

impl AcpInitHandle {
    pub async fn wait_until_ready(&mut self) -> Result<AcpSyncResult, AcpBootstrapError> {
        loop {
            let state = self.receiver.borrow().clone();
            match state {
                AcpInitState::Pending => {
                    if self.receiver.changed().await.is_err() {
                        return Err(AcpBootstrapError::Other(
                            "acp init background task terminated unexpectedly".to_string(),
                        ));
                    }
                }
                AcpInitState::Ready(result) => return Ok(result),
            }
        }
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(*self.receiver.borrow(), AcpInitState::Ready(_))
    }

    #[must_use]
    pub fn manager(&self) -> Arc<Mutex<AcpManager>> {
        Arc::clone(&self.manager)
    }
}

impl Drop for AcpInitHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Clone)]
struct AcpAgentHandle {
    config: AcpAgentConfig,
    tool_name: String,
}

pub struct AcpManager {
    tools: ToolRegistry,
    hub: AcpAgentHub,
    agents: BTreeMap<AcpAgentKey, AcpAgentHandle>,
    statuses: Arc<StdMutex<BTreeMap<AcpAgentKey, AcpAgentStatus>>>,
    config: AcpConfigSnapshot,
}

impl AcpManager {
    #[must_use]
    pub fn new(tools: ToolRegistry) -> Self {
        Self {
            tools,
            hub: AcpAgentHub::default(),
            agents: BTreeMap::new(),
            statuses: Arc::new(StdMutex::new(BTreeMap::new())),
            config: AcpConfigSnapshot::default(),
        }
    }

    #[must_use]
    pub fn spawn_init(tools: ToolRegistry, config: AcpConfigSnapshot) -> AcpInitHandle {
        let manager = Arc::new(Mutex::new(Self::new(tools)));
        let manager_for_task = Arc::clone(&manager);
        let (sender, receiver) = watch::channel(AcpInitState::Pending);
        let task = tokio::spawn(async move {
            let mut guard = manager_for_task.lock().await;
            let result = guard.do_init(Arc::clone(&manager_for_task), config).await;
            let _ = sender.send(AcpInitState::Ready(result));
        });
        AcpInitHandle {
            receiver,
            task,
            manager,
        }
    }

    async fn do_init(
        &mut self,
        manager_handle: Arc<Mutex<AcpManager>>,
        config: AcpConfigSnapshot,
    ) -> AcpSyncResult {
        self.sync(manager_handle, config).await
    }

    pub async fn sync(
        &mut self,
        manager_handle: Arc<Mutex<AcpManager>>,
        snapshot: AcpConfigSnapshot,
    ) -> AcpSyncResult {
        let current = self
            .agents
            .iter()
            .map(|(key, handle)| (key.clone(), handle.config.clone()))
            .collect::<BTreeMap<_, _>>();
        let plan = plan_agent_updates(&current, &snapshot);
        self.config = snapshot.clone();

        for key in &plan.stop {
            self.stop_agent(key);
        }

        for config in &plan.restart {
            self.stop_agent(&AcpAgentKey::new(&config.id));
        }

        for config in plan.start.iter().chain(plan.restart.iter()) {
            self.start_agent(Arc::clone(&manager_handle), config.clone())
                .await;
        }

        self.reconcile_statuses(&snapshot);

        AcpSyncResult {
            keep: plan.keep,
            start: plan
                .start
                .into_iter()
                .map(|config| AcpAgentKey::new(&config.id))
                .collect(),
            restart: plan
                .restart
                .into_iter()
                .map(|config| AcpAgentKey::new(&config.id))
                .collect(),
            stop: plan.stop,
            statuses: self.snapshot_statuses(&snapshot),
            active_agents: self
                .agents
                .keys()
                .map(|key| key.as_str().to_string())
                .collect(),
            tool_count: self.agents.len(),
        }
    }

    pub async fn shutdown_all(&mut self) {
        let keys = self.agents.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            self.stop_agent(&key);
        }
    }

    pub async fn execute_prompt(
        &mut self,
        agent_id: &str,
        prompt: &str,
        working_directory: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<String, AcpExecutionError> {
        let Some(handle) = self.agents.get(&AcpAgentKey::new(agent_id)) else {
            return Err(AcpExecutionError::AgentNotFound {
                agent_id: agent_id.to_string(),
            });
        };
        let config = handle.config.clone();
        let prompt = prompt.to_string();
        let working_directory = working_directory.map(ToString::to_string);
        tokio::task::spawn_blocking(move || {
            run_prompt_blocking(config, prompt, working_directory, timeout)
        })
        .await
        .map_err(|err| AcpExecutionError::WorkerJoin(err.to_string()))?
    }

    async fn start_agent(
        &mut self,
        manager_handle: Arc<Mutex<AcpManager>>,
        config: AcpAgentConfig,
    ) {
        if config.command.trim().is_empty() {
            let key = AcpAgentKey::new(&config.id);
            let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
            guard.insert(
                key,
                AcpAgentStatus {
                    key: AcpAgentKey::new(&config.id),
                    enabled: config.enabled,
                    state: AcpLifecycleState::Failed,
                    last_error: Some("command cannot be empty".to_string()),
                },
            );
            warn!(agent = %config.id, "skipping acp agent with empty command");
            return;
        }
        let key = AcpAgentKey::new(&config.id);
        {
            let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
            guard.insert(
                key.clone(),
                AcpAgentStatus {
                    key: key.clone(),
                    enabled: config.enabled,
                    state: AcpLifecycleState::Starting,
                    last_error: None,
                },
            );
        }

        let tool_name = format!("acp_agent_{}", config.id.trim());
        let description = if config.description.trim().is_empty() {
            format!(
                "Delegate coding tasks to external ACP agent `{}` started with command `{}`.",
                config.id, config.command
            )
        } else {
            config.description.clone()
        };
        self.tools.register_shared(Arc::new(AcpProxyTool::new(
            AcpToolDescriptor {
                name: tool_name.clone(),
                description,
                agent_id: config.id.clone(),
            },
            manager_handle,
        )));
        self.hub.insert(config.id.clone(), config.command.clone());
        self.agents.insert(
            key.clone(),
            AcpAgentHandle {
                config: config.clone(),
                tool_name,
            },
        );
        let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        guard.insert(
            key,
            AcpAgentStatus {
                key: AcpAgentKey::new(&config.id),
                enabled: config.enabled,
                state: AcpLifecycleState::Running,
                last_error: None,
            },
        );
        info!(agent = %config.id, command = %config.command, "registered acp agent tool");
    }

    fn stop_agent(&mut self, key: &AcpAgentKey) {
        let Some(handle) = self.agents.remove(key) else {
            return;
        };
        self.tools.unregister(&handle.tool_name);
        self.hub.remove(key.as_str());
        let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        guard.insert(
            key.clone(),
            AcpAgentStatus {
                key: key.clone(),
                enabled: false,
                state: AcpLifecycleState::Stopped,
                last_error: None,
            },
        );
    }

    fn reconcile_statuses(&mut self, snapshot: &AcpConfigSnapshot) {
        let desired_keys: BTreeSet<AcpAgentKey> = snapshot
            .agents
            .iter()
            .map(|agent| AcpAgentKey::new(&agent.id))
            .collect();
        let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        guard.retain(|key, _| desired_keys.contains(key));
        for agent in &snapshot.agents {
            let key = AcpAgentKey::new(&agent.id);
            guard.entry(key.clone()).or_insert(AcpAgentStatus {
                key,
                enabled: agent.enabled,
                state: AcpLifecycleState::Stopped,
                last_error: None,
            });
        }
    }

    fn snapshot_statuses(&self, snapshot: &AcpConfigSnapshot) -> Vec<AcpAgentStatus> {
        let guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        snapshot
            .agents
            .iter()
            .map(|agent| {
                guard
                    .get(&AcpAgentKey::new(&agent.id))
                    .cloned()
                    .unwrap_or(AcpAgentStatus {
                        key: AcpAgentKey::new(&agent.id),
                        enabled: agent.enabled,
                        state: AcpLifecycleState::Stopped,
                        last_error: None,
                    })
            })
            .collect()
    }
}

fn run_prompt_blocking(
    config: AcpAgentConfig,
    prompt: String,
    working_directory: Option<String>,
    timeout: Option<Duration>,
) -> Result<String, AcpExecutionError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| AcpExecutionError::Runtime(err.to_string()))?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async move {
        run_prompt_async(config, prompt, working_directory, timeout).await
    })
}

async fn run_prompt_async(
    config: AcpAgentConfig,
    prompt: String,
    working_directory: Option<String>,
    timeout: Option<Duration>,
) -> Result<String, AcpExecutionError> {
    use acp::Agent as _;

    let session_root = resolve_session_root(&config, working_directory.as_deref())?;
    let client = KlawAcpClient::new(session_root.clone());

    let mut command = tokio::process::Command::new(config.command.trim());
    command
        .args(config.args.iter().map(String::as_str))
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(cwd) = config.cwd.as_ref().filter(|value| !value.trim().is_empty()) {
        let cwd = resolve_base_cwd(cwd)?;
        command.current_dir(cwd);
    }
    for (key, value) in &config.env {
        command.env(key, value);
    }

    let mut child = command
        .spawn()
        .map_err(|err| AcpExecutionError::Spawn(err.to_string()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AcpExecutionError::Spawn("missing child stdin".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AcpExecutionError::Spawn("missing child stdout".to_string()))?;
    let stderr = child.stderr.take();

    let stderr_tail = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = stderr {
        let stderr_tail_task = Arc::clone(&stderr_tail);
        tokio::task::spawn_local(async move {
            capture_stderr(stderr, stderr_tail_task).await;
        });
    }

    let (conn, io_driver) = acp::ClientSideConnection::new(
        client.clone(),
        stdin.compat_write(),
        stdout.compat(),
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );
    let io_handle = tokio::task::spawn_local(async move { io_driver.await });

    let init_request = acp::InitializeRequest::new(acp::ProtocolVersion::from(1u16))
        .client_capabilities(
            acp::ClientCapabilities::default()
                .fs(acp::FileSystemCapabilities::default()
                    .read_text_file(true)
                    .write_text_file(true))
                .terminal(true),
        )
        .client_info(acp::Implementation::new("klaw", env!("CARGO_PKG_VERSION")).title("Klaw"));
    if let Err(err) = conn.initialize(init_request).await {
        return Err(AcpExecutionError::Initialize(
            with_stderr(err, &stderr_tail).await,
        ));
    }

    let new_session = match conn
        .new_session(acp::NewSessionRequest::new(session_root.clone()))
        .await
    {
        Ok(session) => session,
        Err(err) => {
            return Err(AcpExecutionError::NewSession(
                with_stderr(err, &stderr_tail).await,
            ));
        }
    };

    let prompt_request =
        acp::PromptRequest::new(new_session.session_id.clone(), vec![prompt.into()]);
    let prompt_result = if let Some(timeout) = timeout {
        tokio::time::timeout(timeout, conn.prompt(prompt_request))
            .await
            .map_err(|_| AcpExecutionError::Timeout {
                agent_id: config.id.clone(),
                timeout,
            })?
    } else {
        conn.prompt(prompt_request).await
    };
    if let Err(err) = prompt_result {
        return Err(AcpExecutionError::Prompt(
            with_stderr(err, &stderr_tail).await,
        ));
    }

    let session_id = new_session.session_id.to_string();
    let session_log = client
        .session_log(&session_id)
        .await
        .unwrap_or_default()
        .final_output();

    terminate_child(&mut child).await;
    let _ = io_handle.await;

    if session_log.trim().is_empty() {
        return Err(AcpExecutionError::EmptyResponse {
            agent_id: config.id,
        });
    }
    Ok(session_log)
}

fn resolve_base_cwd(raw: &str) -> Result<PathBuf, AcpExecutionError> {
    let candidate = PathBuf::from(raw.trim());
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        std::env::current_dir()
            .map_err(|err| AcpExecutionError::WorkingDirectory(err.to_string()))?
            .join(candidate)
    };
    std::fs::canonicalize(candidate)
        .map_err(|err| AcpExecutionError::WorkingDirectory(err.to_string()))
}

fn resolve_session_root(
    config: &AcpAgentConfig,
    override_working_directory: Option<&str>,
) -> Result<PathBuf, AcpExecutionError> {
    let base = config
        .cwd
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(resolve_base_cwd)
        .transpose()?
        .unwrap_or(
            std::env::current_dir()
                .map_err(|err| AcpExecutionError::WorkingDirectory(err.to_string()))?,
        );

    let candidate = override_working_directory
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|value| {
            if value.is_absolute() {
                value
            } else {
                base.join(value)
            }
        })
        .unwrap_or(base);

    std::fs::canonicalize(&candidate).map_err(|err| {
        AcpExecutionError::InvalidWorkingDirectory(format!("{} ({err})", candidate.display()))
    })
}

async fn with_stderr(err: acp::Error, stderr_tail: &Arc<Mutex<String>>) -> String {
    let tail = stderr_tail.lock().await.clone();
    if tail.trim().is_empty() {
        err.to_string()
    } else {
        format!("{err}; stderr: {tail}")
    }
}

async fn capture_stderr(mut stderr: tokio::process::ChildStderr, stderr_tail: Arc<Mutex<String>>) {
    use tokio::io::AsyncReadExt;

    let mut buf = vec![0u8; 4096];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) => break,
            Ok(read_bytes) => {
                let fragment = String::from_utf8_lossy(&buf[..read_bytes]).to_string();
                let mut guard = stderr_tail.lock().await;
                guard.push_str(&fragment);
                if guard.len() > 4000 {
                    let mut trim_index = guard.len().saturating_sub(4000);
                    while trim_index < guard.len() && !guard.is_char_boundary(trim_index) {
                        trim_index += 1;
                    }
                    guard.drain(..trim_index);
                }
            }
            Err(err) => {
                warn!(error = %err, "failed to read acp agent stderr");
                break;
            }
        }
    }
}

async fn terminate_child(child: &mut tokio::process::Child) {
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        Err(err) => {
            warn!(error = %err, "failed to inspect acp child process before shutdown");
        }
    }
}

#[derive(Debug, Default)]
struct AcpSyncPlan {
    keep: Vec<AcpAgentKey>,
    start: Vec<AcpAgentConfig>,
    restart: Vec<AcpAgentConfig>,
    stop: Vec<AcpAgentKey>,
}

fn plan_agent_updates(
    current: &BTreeMap<AcpAgentKey, AcpAgentConfig>,
    desired: &AcpConfigSnapshot,
) -> AcpSyncPlan {
    let desired_enabled = desired
        .agents
        .iter()
        .filter(|agent| agent.enabled)
        .map(|agent| (AcpAgentKey::new(&agent.id), agent.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut plan = AcpSyncPlan::default();

    for key in current.keys() {
        if !desired_enabled.contains_key(key) {
            plan.stop.push(key.clone());
        }
    }

    for (key, desired_config) in desired_enabled {
        match current.get(&key) {
            Some(current_config) if current_config == &desired_config => plan.keep.push(key),
            Some(_) => plan.restart.push(desired_config),
            None => plan.start.push(desired_config),
        }
    }

    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn agent(id: &str, enabled: bool) -> AcpAgentConfig {
        AcpAgentConfig {
            id: id.to_string(),
            enabled,
            command: "echo".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            description: String::new(),
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn ensure_python3() {
        let status = std::process::Command::new("python3")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("python3 should be installed for ACP mock agent tests");
        assert!(status.success(), "python3 --version should succeed");
    }

    fn write_mock_agent_script(dir: &PathBuf) -> PathBuf {
        let script = dir.join("mock_acp_agent.py");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys

session_id = "mock-session"

def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

for raw_line in sys.stdin:
    line = raw_line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    msg_id = message.get("id")
    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {},
                "authMethods": [],
                "agentInfo": {
                    "name": "mock-agent",
                    "version": "0.1.0"
                }
            }
        })
    elif method == "session/new":
        send({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {
                "sessionId": session_id
            }
        })
    elif method == "session/prompt":
        prompt_blocks = message.get("params", {}).get("prompt", [])
        prompt_text = ""
        if prompt_blocks:
            first = prompt_blocks[0]
            if first.get("type") == "text":
                prompt_text = first.get("text", "")
        send({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": {
                        "type": "text",
                        "text": "thinking about " + prompt_text
                    }
                }
            }
        })
        send({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": "mock response for: " + prompt_text
                    }
                }
            }
        })
        send({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {
                "stopReason": "end_turn"
            }
        })
        break
    else:
        send({
            "jsonrpc": "2.0",
            "id": msg_id,
            "error": {
                "code": -32601,
                "message": "method not found"
            }
        })
"#,
        )
        .expect("write mock ACP agent script");
        script
    }

    #[test]
    fn snapshot_clones_from_config() {
        let config = AcpConfig {
            startup_timeout_seconds: 15,
            agents: vec![agent("claude", true)],
        };
        let snapshot = AcpConfigSnapshot::from_config(&config);
        assert_eq!(snapshot.startup_timeout_seconds, 15);
        assert_eq!(snapshot.agents.len(), 1);
    }

    #[test]
    fn plan_updates_start_enabled_agents() {
        let current = BTreeMap::new();
        let desired = AcpConfigSnapshot {
            startup_timeout_seconds: 10,
            agents: vec![agent("claude", true)],
        };
        let plan = plan_agent_updates(&current, &desired);
        assert_eq!(plan.start.len(), 1);
        assert!(plan.keep.is_empty());
    }

    #[test]
    fn plan_updates_stop_removed_agents() {
        let mut current = BTreeMap::new();
        current.insert(AcpAgentKey::new("claude"), agent("claude", true));
        let desired = AcpConfigSnapshot::default();
        let plan = plan_agent_updates(&current, &desired);
        assert_eq!(plan.stop, vec![AcpAgentKey::new("claude")]);
    }

    #[test]
    fn plan_updates_restart_changed_agents() {
        let mut current = BTreeMap::new();
        current.insert(AcpAgentKey::new("claude"), agent("claude", true));
        let mut changed = agent("claude", true);
        changed.description = "different".to_string();
        let desired = AcpConfigSnapshot {
            startup_timeout_seconds: 10,
            agents: vec![changed],
        };
        let plan = plan_agent_updates(&current, &desired);
        assert_eq!(plan.restart.len(), 1);
    }

    #[test]
    fn resolve_session_root_prefers_override_relative_to_agent_cwd() {
        let root = std::env::temp_dir().join(format!("klaw-acp-manager-{}", uuid::Uuid::new_v4()));
        let nested = root.join("workspace").join("subdir");
        std::fs::create_dir_all(&nested).expect("create nested dir");

        let mut config = agent("claude", true);
        config.cwd = Some(root.join("workspace").display().to_string());

        let resolved = resolve_session_root(&config, Some("subdir")).expect("resolve session root");
        assert_eq!(
            resolved,
            std::fs::canonicalize(&nested).expect("canonical nested")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn execute_prompt_runs_end_to_end_against_mock_acp_agent() {
        ensure_python3();
        let root = temp_dir("klaw-acp-e2e");
        let script = write_mock_agent_script(&root);

        let mut config = agent("mock", true);
        config.command = "python3".to_string();
        config.args = vec![script.display().to_string()];
        config.cwd = Some(root.display().to_string());

        let mut manager = AcpManager::new(ToolRegistry::default());
        manager.agents.insert(
            AcpAgentKey::new("mock"),
            AcpAgentHandle {
                config,
                tool_name: "acp_agent_mock".to_string(),
            },
        );
        let working_directory = root.display().to_string();

        let result = manager
            .execute_prompt(
                "mock",
                "summarize the mock plan",
                Some(working_directory.as_str()),
                Some(Duration::from_secs(5)),
            )
            .await
            .expect("mock ACP agent prompt should succeed");

        assert_eq!(result, "mock response for: summarize the mock plan");

        let _ = std::fs::remove_dir_all(root);
    }
}
