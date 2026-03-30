use agent_client_protocol as acp;
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    sync::Mutex,
};
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcpSessionUpdateLog {
    pub answer: String,
    pub reasoning: String,
    pub tool_updates: Vec<String>,
    pub raw_updates: Vec<String>,
}

impl AcpSessionUpdateLog {
    #[must_use]
    pub fn final_output(&self) -> String {
        let answer = self.answer.trim();
        if !answer.is_empty() {
            return answer.to_string();
        }
        if !self.tool_updates.is_empty() {
            return self.tool_updates.join("\n");
        }
        self.raw_updates.join("\n")
    }
}

#[derive(Debug, Default)]
struct TerminalOutputBuffer {
    text: String,
    truncated: bool,
    output_byte_limit: Option<u64>,
}

impl TerminalOutputBuffer {
    fn append(&mut self, fragment: &str) {
        self.text.push_str(fragment);
        let Some(limit) = self.output_byte_limit else {
            return;
        };
        let limit = limit as usize;
        if self.text.len() <= limit {
            return;
        }
        let mut trim_index = self.text.len().saturating_sub(limit);
        while trim_index < self.text.len() && !self.text.is_char_boundary(trim_index) {
            trim_index += 1;
        }
        if trim_index >= self.text.len() {
            self.text.clear();
        } else {
            self.text.drain(..trim_index);
        }
        self.truncated = true;
    }
}

struct TrackedTerminal {
    child: Child,
    output: Arc<Mutex<TerminalOutputBuffer>>,
    exit_status: Option<acp::TerminalExitStatus>,
}

#[derive(Clone)]
pub struct KlawAcpClient {
    session_root: PathBuf,
    updates: Arc<Mutex<BTreeMap<String, AcpSessionUpdateLog>>>,
    terminals: Arc<Mutex<BTreeMap<String, Arc<Mutex<TrackedTerminal>>>>>,
}

impl KlawAcpClient {
    #[must_use]
    pub fn new(session_root: PathBuf) -> Self {
        Self {
            session_root,
            updates: Arc::new(Mutex::new(BTreeMap::new())),
            terminals: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub async fn session_log(&self, session_id: &str) -> Option<AcpSessionUpdateLog> {
        self.updates.lock().await.get(session_id).cloned()
    }

    async fn mutate_session_log(
        &self,
        session_id: String,
        mutate: impl FnOnce(&mut AcpSessionUpdateLog),
    ) {
        let mut guard = self.updates.lock().await;
        let entry = guard.entry(session_id).or_default();
        mutate(entry);
    }

    fn resolve_scoped_path(&self, requested: &Path) -> acp::Result<PathBuf> {
        let candidate = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.session_root.join(requested)
        };

        let resolved = if candidate.exists() {
            std::fs::canonicalize(&candidate).map_err(acp::Error::into_internal_error)?
        } else {
            let parent = candidate
                .parent()
                .ok_or_else(acp::Error::invalid_params)?
                .to_path_buf();
            let resolved_parent =
                std::fs::canonicalize(parent).map_err(acp::Error::into_internal_error)?;
            let file_name = candidate
                .file_name()
                .ok_or_else(acp::Error::invalid_params)?
                .to_owned();
            resolved_parent.join(file_name)
        };

        if resolved.starts_with(&self.session_root) {
            Ok(resolved)
        } else {
            Err(acp::Error::resource_not_found(Some(
                requested.display().to_string(),
            )))
        }
    }
}

#[async_trait(?Send)]
impl acp::Client for KlawAcpClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        let outcome = args
            .options
            .first()
            .map(|option| {
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    option.option_id.clone(),
                ))
            })
            .unwrap_or(acp::RequestPermissionOutcome::Cancelled);
        Ok(acp::RequestPermissionResponse::new(outcome))
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let session_id = args.session_id.to_string();
        self.mutate_session_log(session_id, |entry| {
            entry.raw_updates.push(format!("{:?}", args.update));
            match args.update {
                acp::SessionUpdate::AgentMessageChunk(chunk) => {
                    entry.answer.push_str(&render_content_block(&chunk.content));
                }
                acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                    entry
                        .reasoning
                        .push_str(&render_content_block(&chunk.content));
                }
                acp::SessionUpdate::ToolCall(tool_call) => {
                    entry.tool_updates.push(format!(
                        "tool call: {:?} {}",
                        tool_call.kind, tool_call.title
                    ));
                }
                acp::SessionUpdate::ToolCallUpdate(update) => {
                    entry
                        .tool_updates
                        .push(format!("tool update: {:?}", update));
                }
                acp::SessionUpdate::Plan(plan) => {
                    entry.tool_updates.push(format!("plan: {:?}", plan));
                }
                acp::SessionUpdate::UserMessageChunk(_)
                | acp::SessionUpdate::AvailableCommandsUpdate(_)
                | acp::SessionUpdate::CurrentModeUpdate(_)
                | acp::SessionUpdate::ConfigOptionUpdate(_)
                | acp::SessionUpdate::SessionInfoUpdate(_)
                | _ => {}
            }
        })
        .await;
        Ok(())
    }

    async fn read_text_file(
        &self,
        args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        let path = self.resolve_scoped_path(&args.path)?;
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(acp::Error::into_internal_error)?;
        let content = slice_lines(&content, args.line, args.limit);
        Ok(acp::ReadTextFileResponse::new(content))
    }

    async fn write_text_file(
        &self,
        args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        let path = self.resolve_scoped_path(&args.path)?;
        let parent = path.parent().ok_or_else(acp::Error::invalid_params)?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(acp::Error::into_internal_error)?;
        tokio::fs::write(path, args.content)
            .await
            .map_err(acp::Error::into_internal_error)?;
        Ok(acp::WriteTextFileResponse::default())
    }

    async fn create_terminal(
        &self,
        args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        let cwd = args
            .cwd
            .as_ref()
            .map(|cwd| self.resolve_scoped_path(cwd))
            .transpose()?;
        let mut command = Command::new(args.command.trim());
        command
            .args(args.args)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        } else {
            command.current_dir(&self.session_root);
        }
        for env in args.env {
            command.env(env.name, env.value);
        }
        let mut child = command.spawn().map_err(acp::Error::into_internal_error)?;
        let output = Arc::new(Mutex::new(TerminalOutputBuffer {
            text: String::new(),
            truncated: false,
            output_byte_limit: args.output_byte_limit,
        }));
        if let Some(stdout) = child.stdout.take() {
            tokio::task::spawn_local(read_terminal_stream(stdout, Arc::clone(&output)));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::task::spawn_local(read_terminal_stream(stderr, Arc::clone(&output)));
        }

        let terminal_id = Uuid::new_v4().to_string();
        self.terminals.lock().await.insert(
            terminal_id.clone(),
            Arc::new(Mutex::new(TrackedTerminal {
                child,
                output,
                exit_status: None,
            })),
        );
        Ok(acp::CreateTerminalResponse::new(terminal_id))
    }

    async fn terminal_output(
        &self,
        args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        let terminal = self
            .terminals
            .lock()
            .await
            .get(args.terminal_id.to_string().as_str())
            .cloned()
            .ok_or_else(|| acp::Error::resource_not_found(Some(args.terminal_id.to_string())))?;
        let mut guard = terminal.lock().await;
        let exit_status = refresh_exit_status(&mut guard)
            .await
            .map_err(acp::Error::into_internal_error)?;
        let output_guard = guard.output.lock().await;
        Ok(
            acp::TerminalOutputResponse::new(output_guard.text.clone(), output_guard.truncated)
                .exit_status(exit_status),
        )
    }

    async fn release_terminal(
        &self,
        args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        let Some(terminal) = self
            .terminals
            .lock()
            .await
            .remove(args.terminal_id.to_string().as_str())
        else {
            return Ok(acp::ReleaseTerminalResponse::default());
        };
        let mut guard = terminal.lock().await;
        if refresh_exit_status(&mut guard)
            .await
            .map_err(acp::Error::into_internal_error)?
            .is_none()
        {
            let _ = guard.child.kill().await;
            let _ = refresh_exit_status(&mut guard).await;
        }
        Ok(acp::ReleaseTerminalResponse::default())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        let terminal = self
            .terminals
            .lock()
            .await
            .get(args.terminal_id.to_string().as_str())
            .cloned()
            .ok_or_else(|| acp::Error::resource_not_found(Some(args.terminal_id.to_string())))?;
        let mut guard = terminal.lock().await;
        if let Some(exit_status) = guard.exit_status.clone() {
            return Ok(acp::WaitForTerminalExitResponse::new(exit_status));
        }
        let status = guard
            .child
            .wait()
            .await
            .map_err(acp::Error::into_internal_error)?;
        let exit_status = convert_exit_status(status);
        guard.exit_status = Some(exit_status.clone());
        Ok(acp::WaitForTerminalExitResponse::new(exit_status))
    }

    async fn kill_terminal(
        &self,
        args: acp::KillTerminalRequest,
    ) -> acp::Result<acp::KillTerminalResponse> {
        let terminal = self
            .terminals
            .lock()
            .await
            .get(args.terminal_id.to_string().as_str())
            .cloned()
            .ok_or_else(|| acp::Error::resource_not_found(Some(args.terminal_id.to_string())))?;
        let mut guard = terminal.lock().await;
        if guard.exit_status.is_none() {
            guard
                .child
                .kill()
                .await
                .map_err(acp::Error::into_internal_error)?;
            let _ = refresh_exit_status(&mut guard)
                .await
                .map_err(acp::Error::into_internal_error)?;
        }
        Ok(acp::KillTerminalResponse::default())
    }
}

fn render_content_block(content: &acp::ContentBlock) -> &str {
    match content {
        acp::ContentBlock::Text(text) => text.text.as_str(),
        _ => "",
    }
}

fn slice_lines(content: &str, line: Option<u32>, limit: Option<u32>) -> String {
    let start = line.unwrap_or(1).saturating_sub(1) as usize;
    let limit = limit.map(|value| value as usize).unwrap_or(usize::MAX);
    content
        .lines()
        .skip(start)
        .take(limit)
        .collect::<Vec<_>>()
        .join("\n")
}

async fn read_terminal_stream<R>(mut reader: R, output: Arc<Mutex<TerminalOutputBuffer>>)
where
    R: AsyncRead + Unpin + 'static,
{
    let mut buf = vec![0u8; 4096];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(read_bytes) => {
                let fragment = String::from_utf8_lossy(&buf[..read_bytes]).to_string();
                output.lock().await.append(&fragment);
            }
            Err(err) => {
                warn!(error = %err, "failed to read terminal stream");
                break;
            }
        }
    }
}

async fn refresh_exit_status(
    terminal: &mut TrackedTerminal,
) -> std::io::Result<Option<acp::TerminalExitStatus>> {
    if terminal.exit_status.is_some() {
        return Ok(terminal.exit_status.clone());
    }
    let maybe_status = terminal.child.try_wait()?;
    let status = maybe_status.map(convert_exit_status);
    terminal.exit_status = status.clone();
    Ok(status)
}

fn convert_exit_status(status: std::process::ExitStatus) -> acp::TerminalExitStatus {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|value| value.to_string())
    };
    #[cfg(not(unix))]
    let signal = None;

    acp::TerminalExitStatus::default()
        .exit_code(status.code().map(|value| value as u32))
        .signal(signal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("klaw-acp-client-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn final_output_prefers_answer_chunks() {
        let log = AcpSessionUpdateLog {
            answer: "done".to_string(),
            reasoning: "thinking".to_string(),
            tool_updates: vec!["tool".to_string()],
            raw_updates: vec!["raw".to_string()],
        };
        assert_eq!(log.final_output(), "done");
    }

    #[test]
    fn terminal_output_buffer_truncates_at_char_boundary() {
        let mut buffer = TerminalOutputBuffer {
            text: String::new(),
            truncated: false,
            output_byte_limit: Some(4),
        };
        buffer.append("a");
        buffer.append("你");
        buffer.append("b");
        assert_eq!(buffer.text, "你b");
        assert!(buffer.truncated);
    }

    #[test]
    fn slice_lines_applies_one_based_window() {
        let content = "a\nb\nc\nd";
        assert_eq!(slice_lines(content, Some(2), Some(2)), "b\nc");
    }

    #[test]
    fn resolve_scoped_path_rejects_escape_outside_root() {
        let root = temp_dir();
        let client = KlawAcpClient::new(root.clone());
        let outside = root.join("..").join("outside.txt");
        let result = client.resolve_scoped_path(&outside);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
