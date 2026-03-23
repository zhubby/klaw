use async_trait::async_trait;
use klaw_config::McpServerConfig;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::Path,
    process::Stdio,
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::Mutex,
    task::JoinHandle,
    time::{timeout, Duration},
};
use tracing::{info, warn};

const STDERR_TAIL_LIMIT: usize = 50;

#[derive(Debug, Clone)]
pub(crate) struct McpRemoteTool {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: Value,
}

#[derive(Debug, Error)]
pub(crate) enum McpClientError {
    #[error("io: {0}")]
    Io(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("request: {0}")]
    Request(String),
}

impl From<std::io::Error> for McpClientError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

#[async_trait]
pub(crate) trait McpClient: Send + Sync {
    async fn initialize(&self) -> Result<(), McpClientError>;
    async fn list_tools(&self) -> Result<Vec<McpRemoteTool>, McpClientError>;
    async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value, McpClientError>;
    async fn shutdown(&self) -> Result<(), McpClientError> {
        Ok(())
    }
    async fn stderr_tail(&self) -> Option<String> {
        None
    }
}

#[derive(Debug)]
struct StdioIo {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub(crate) struct StdioMcpClient {
    io: Mutex<StdioIo>,
    next_id: AtomicU64,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stderr_task: Mutex<Option<JoinHandle<()>>>,
}

impl StdioMcpClient {
    pub(crate) async fn new(server: &McpServerConfig) -> Result<Self, McpClientError> {
        let command = server.command.clone().unwrap_or_default();
        let mut cmd = Command::new(command.trim());
        cmd.kill_on_drop(true);
        cmd.args(server.args.clone());
        if let Some(cwd) = &server.cwd {
            cmd.current_dir(cwd);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &server.env {
            cmd.env(key, value);
        }
        if should_force_npx_yes(command.trim(), &server.env) {
            cmd.env("npm_config_yes", "true");
        }

        let mut child = cmd
            .spawn()
            .map_err(|err| McpClientError::Request(format!("spawn failed: {err}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpClientError::Request("missing child stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpClientError::Request("missing child stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| McpClientError::Request("missing child stderr".to_string()))?;
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LIMIT)));
        let stderr_task = tokio::spawn(capture_stderr(stderr, Arc::clone(&stderr_tail)));

        Ok(Self {
            io: Mutex::new(StdioIo {
                child,
                stdin,
                stdout: BufReader::new(stdout),
            }),
            next_id: AtomicU64::new(1),
            stderr_tail,
            stderr_task: Mutex::new(Some(stderr_task)),
        })
    }

    async fn rpc_request(&self, method: &str, params: Value) -> Result<Value, McpClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut io = self.io.lock().await;
        let _ = io.child.id();
        write_stdio_frame(&mut io.stdin, &payload).await?;
        io.stdin.flush().await?;
        loop {
            let response = read_stdio_frame(&mut io.stdout).await?;
            let value: Value = serde_json::from_slice(&response)
                .map_err(|err| McpClientError::Protocol(err.to_string()))?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(err) = value.get("error") {
                return Err(McpClientError::Request(err.to_string()));
            }
            return Ok(value
                .get("result")
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default())));
        }
    }

    async fn rpc_notify(&self, method: &str, params: Value) -> Result<(), McpClientError> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let mut io = self.io.lock().await;
        let _ = io.child.id();
        write_stdio_frame(&mut io.stdin, &payload).await?;
        io.stdin.flush().await?;
        Ok(())
    }

    async fn shutdown_process(&self) -> Result<(), McpClientError> {
        let mut io = self.io.lock().await;
        info!("shutting down stdio mcp child process");
        io.stdin.shutdown().await?;

        match timeout(Duration::from_millis(500), io.child.wait()).await {
            Ok(Ok(status)) => {
                info!(?status, "stdio mcp child exited gracefully");
                self.stop_stderr_task().await;
                Ok(())
            }
            Ok(Err(err)) => Err(McpClientError::Io(err.to_string())),
            Err(_) => {
                warn!("stdio mcp child did not exit after stdin shutdown, force killing");
                io.child
                    .start_kill()
                    .map_err(|err| McpClientError::Io(err.to_string()))?;
                let status = io
                    .child
                    .wait()
                    .await
                    .map_err(|err| McpClientError::Io(err.to_string()))?;
                info!(?status, "stdio mcp child exited after force kill");
                self.stop_stderr_task().await;
                Ok(())
            }
        }
    }

    async fn stop_stderr_task(&self) {
        let mut guard = self.stderr_task.lock().await;
        if let Some(task) = guard.take() {
            task.abort();
            let _ = task.await;
        }
    }

    async fn stderr_tail_snapshot(&self) -> Option<String> {
        let lines = self.stderr_tail.lock().await;
        if lines.is_empty() {
            None
        } else {
            Some(lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        }
    }
}

fn should_force_npx_yes(command: &str, env: &std::collections::BTreeMap<String, String>) -> bool {
    if env.contains_key("npm_config_yes") {
        return false;
    }

    let Some(file_name) = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    file_name.eq_ignore_ascii_case("npx")
}

async fn write_stdio_frame<W: AsyncWrite + Unpin>(
    stdin: &mut W,
    payload: &Value,
) -> Result<(), McpClientError> {
    let mut bytes =
        serde_json::to_vec(payload).map_err(|err| McpClientError::Protocol(err.to_string()))?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await?;
    Ok(())
}

async fn read_stdio_frame<R: AsyncBufRead + Unpin>(
    stdout: &mut R,
) -> Result<Vec<u8>, McpClientError> {
    let mut first_line = String::new();
    let n = stdout.read_line(&mut first_line).await?;
    if n == 0 {
        return Err(McpClientError::Protocol(
            "unexpected EOF while reading MCP stdio message".to_string(),
        ));
    }

    let trimmed = first_line.trim_end_matches(['\r', '\n']);
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Ok(trimmed.as_bytes().to_vec());
    }

    let mut content_length = None;
    let mut current_line = first_line;
    loop {
        let trimmed = current_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let (name, value) = trimmed
            .split_once(':')
            .ok_or_else(|| McpClientError::Protocol("invalid MCP header".to_string()))?;
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .trim()
                .parse::<usize>()
                .map_err(|err| McpClientError::Protocol(err.to_string()))?;
            content_length = Some(parsed);
        }

        current_line.clear();
        let n = stdout.read_line(&mut current_line).await?;
        if n == 0 {
            return Err(McpClientError::Protocol(
                "unexpected EOF while reading MCP headers".to_string(),
            ));
        }
    }

    let len = content_length
        .ok_or_else(|| McpClientError::Protocol("missing Content-Length header".to_string()))?;
    let mut body = vec![0u8; len];
    stdout.read_exact(&mut body).await?;
    Ok(body)
}

#[async_trait]
impl McpClient for StdioMcpClient {
    async fn initialize(&self) -> Result<(), McpClientError> {
        let _ = self
            .rpc_request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "klaw",
                        "version": "0.1.0"
                    }
                }),
            )
            .await?;
        let _ = self
            .rpc_notify("notifications/initialized", json!({}))
            .await;
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<McpRemoteTool>, McpClientError> {
        let result = self.rpc_request("tools/list", json!({})).await?;
        parse_tools_list_result(&result)
    }

    async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value, McpClientError> {
        self.rpc_request(
            "tools/call",
            json!({
                "name": tool_name,
                "arguments": arguments
            }),
        )
        .await
    }

    async fn shutdown(&self) -> Result<(), McpClientError> {
        self.shutdown_process().await
    }

    async fn stderr_tail(&self) -> Option<String> {
        self.stderr_tail_snapshot().await
    }
}

async fn capture_stderr(stderr: ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut reader = BufReader::new(stderr).lines();
    loop {
        match reader.next_line().await {
            Ok(Some(line)) => {
                let mut guard = tail.lock().await;
                if guard.len() == STDERR_TAIL_LIMIT {
                    guard.pop_front();
                }
                guard.push_back(line);
            }
            Ok(None) => break,
            Err(err) => {
                let mut guard = tail.lock().await;
                if guard.len() == STDERR_TAIL_LIMIT {
                    guard.pop_front();
                }
                guard.push_back(format!("[stderr read error] {err}"));
                break;
            }
        }
    }
}

pub(crate) struct SseMcpClient {
    client: reqwest::Client,
    url: String,
    headers: HeaderMap,
    next_id: AtomicU64,
}

impl SseMcpClient {
    pub(crate) fn new(server: &McpServerConfig) -> Result<Self, McpClientError> {
        let url = server.url.clone().unwrap_or_default();
        let mut headers = HeaderMap::new();
        // Default SSE/JSON media-type negotiation for MCP servers (e.g. context7).
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        for (key, value) in &server.headers {
            let name = HeaderName::from_str(key)
                .map_err(|err| McpClientError::Request(format!("invalid header name: {err}")))?;
            let value = HeaderValue::from_str(value).map_err(|err| {
                McpClientError::Request(format!("invalid header value for {key}: {err}"))
            })?;
            headers.insert(name, value);
        }
        Ok(Self {
            client: reqwest::Client::new(),
            url,
            headers,
            next_id: AtomicU64::new(1),
        })
    }

    async fn rpc_request(&self, method: &str, params: Value) -> Result<Value, McpClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let response = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .json(&payload)
            .send()
            .await
            .map_err(|err| McpClientError::Request(err.to_string()))?;
        let status = response.status();
        let value: Value = response
            .json()
            .await
            .map_err(|err| McpClientError::Protocol(err.to_string()))?;
        if !status.is_success() {
            return Err(McpClientError::Request(format!(
                "http status {}: {}",
                status, value
            )));
        }
        if let Some(err) = value.get("error") {
            return Err(McpClientError::Request(err.to_string()));
        }
        Ok(value
            .get("result")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default())))
    }

    async fn rpc_notify(&self, method: &str, params: Value) -> Result<(), McpClientError> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.client
            .post(&self.url)
            .headers(self.headers.clone())
            .json(&payload)
            .send()
            .await
            .map_err(|err| McpClientError::Request(err.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl McpClient for SseMcpClient {
    async fn initialize(&self) -> Result<(), McpClientError> {
        let _ = self
            .rpc_request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "klaw",
                        "version": "0.1.0"
                    }
                }),
            )
            .await?;
        let _ = self
            .rpc_notify("notifications/initialized", json!({}))
            .await;
        Ok(())
    }

    async fn list_tools(&self) -> Result<Vec<McpRemoteTool>, McpClientError> {
        let result = self.rpc_request("tools/list", json!({})).await?;
        parse_tools_list_result(&result)
    }

    async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value, McpClientError> {
        self.rpc_request(
            "tools/call",
            json!({
                "name": tool_name,
                "arguments": arguments
            }),
        )
        .await
    }
}

fn parse_tools_list_result(result: &Value) -> Result<Vec<McpRemoteTool>, McpClientError> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpClientError::Protocol("tools/list result missing `tools`".to_string()))?;

    let mut out = Vec::new();
    for item in tools {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| McpClientError::Protocol("tool entry missing `name`".to_string()))?;
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("MCP tool `{name}`"));
        let parameters = item
            .get("inputSchema")
            .cloned()
            .or_else(|| item.get("parameters").cloned())
            .unwrap_or_else(|| json!({"type":"object"}));
        out.push(McpRemoteTool {
            name: name.to_string(),
            description,
            parameters,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{McpServerConfig, McpServerMode};
    use std::collections::BTreeMap;

    fn sse_server(headers: BTreeMap<String, String>) -> McpServerConfig {
        McpServerConfig {
            id: "test-sse".to_string(),
            enabled: true,
            mode: McpServerMode::Sse,
            command: None,
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
            url: Some("https://example.com/sse".to_string()),
            headers,
        }
    }

    #[test]
    fn sse_client_uses_default_headers_when_not_provided() {
        let client = SseMcpClient::new(&sse_server(BTreeMap::new())).expect("should construct");
        assert_eq!(
            client.headers.get(ACCEPT).and_then(|h| h.to_str().ok()),
            Some("application/json, text/event-stream")
        );
        assert_eq!(
            client
                .headers
                .get(CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("application/json")
        );
    }

    #[test]
    fn sse_client_allows_user_headers_to_override_defaults() {
        let mut headers = BTreeMap::new();
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("Content-Type".to_string(), "application/custom".to_string());
        let client = SseMcpClient::new(&sse_server(headers)).expect("should construct");
        assert_eq!(
            client.headers.get(ACCEPT).and_then(|h| h.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            client
                .headers
                .get(CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("application/custom")
        );
    }

    #[test]
    fn force_npx_yes_for_noninteractive_launch() {
        assert!(should_force_npx_yes("npx", &BTreeMap::new()));
        assert!(should_force_npx_yes(
            "/opt/homebrew/bin/npx",
            &BTreeMap::new()
        ));
    }

    #[test]
    fn preserves_explicit_npm_yes_override() {
        let mut env = BTreeMap::new();
        env.insert("npm_config_yes".to_string(), "false".to_string());
        assert!(!should_force_npx_yes("npx", &env));
        assert!(!should_force_npx_yes("uvx", &BTreeMap::new()));
    }

    #[tokio::test]
    async fn read_stdio_frame_accepts_jsonl_messages() {
        let (client, server) = tokio::io::duplex(256);
        let data = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
        tokio::spawn(async move {
            let mut server = server;
            server.write_all(data).await.expect("write duplex");
        });
        let mut reader = BufReader::new(client);

        let frame = read_stdio_frame(&mut reader).await.expect("read frame");

        assert_eq!(
            std::str::from_utf8(&frame).expect("utf8"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}"
        );
    }
}
