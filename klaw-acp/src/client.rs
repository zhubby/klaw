use agent_client_protocol as acp;
use async_trait::async_trait;
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
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
    pub events: Vec<AcpSessionEvent>,
    pub available_commands: Vec<AcpAvailableCommand>,
    pub current_mode_id: Option<String>,
    pub config_options: Vec<AcpConfigOption>,
    pub session_title: Option<String>,
    pub session_updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpPromptUpdate {
    SessionEvent(AcpSessionEvent),
    PermissionRequest(AcpPermissionRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpSessionEvent {
    pub session_id: String,
    pub summary: String,
    pub update: AcpSessionEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpSessionEventKind {
    UserMessageChunk {
        content: AcpContentBlockEvent,
    },
    AgentMessageChunk {
        content: AcpContentBlockEvent,
    },
    AgentThoughtChunk {
        content: AcpContentBlockEvent,
    },
    ToolCall(AcpToolCallEvent),
    ToolCallUpdate(AcpToolCallUpdateEvent),
    Plan(AcpPlanEvent),
    AvailableCommandsUpdate {
        commands: Vec<AcpAvailableCommand>,
    },
    CurrentModeUpdate {
        current_mode_id: String,
    },
    ConfigOptionUpdate {
        config_options: Vec<AcpConfigOption>,
    },
    SessionInfoUpdate {
        title: Option<Option<String>>,
        updated_at: Option<Option<String>>,
    },
    Other {
        description: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpContentBlockEvent {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        uri: Option<String>,
        data_len: usize,
    },
    Audio {
        mime_type: String,
        data_len: usize,
    },
    ResourceLink {
        name: String,
        uri: String,
        title: Option<String>,
        description: Option<String>,
        mime_type: Option<String>,
    },
    EmbeddedTextResource {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
    EmbeddedBlobResource {
        uri: String,
        mime_type: Option<String>,
        byte_len: usize,
    },
    Unsupported {
        description: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpToolCallEvent {
    pub tool_call_id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub content: Vec<String>,
    pub locations: Vec<String>,
    pub raw_input: Option<String>,
    pub raw_output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpToolCallUpdateEvent {
    pub tool_call_id: String,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub content: Option<Vec<String>>,
    pub locations: Option<Vec<String>>,
    pub raw_input: Option<String>,
    pub raw_output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPlanEvent {
    pub entries: Vec<AcpPlanEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPlanEntry {
    pub content: String,
    pub priority: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAvailableCommand {
    pub name: String,
    pub description: String,
    pub input_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpConfigOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub current_value: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPermissionRequest {
    pub session_id: String,
    pub tool_call_id: String,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub raw_input: Option<String>,
    pub raw_output: Option<String>,
    pub options: Vec<AcpPermissionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPermissionOption {
    pub option_id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpPermissionDecision {
    SelectOption { option_id: String },
    Cancelled,
}

type PromptUpdateSink = Arc<dyn Fn(AcpPromptUpdate) + Send + Sync>;
pub type AcpPermissionRequestFuture =
    Pin<Box<dyn Future<Output = AcpPermissionDecision> + Send + 'static>>;
pub type AcpPermissionRequestHandler =
    Arc<dyn Fn(AcpPermissionRequest) -> AcpPermissionRequestFuture + Send + Sync>;

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
    prompt_update_sink: Option<PromptUpdateSink>,
    permission_request_handler: Option<AcpPermissionRequestHandler>,
}

impl KlawAcpClient {
    #[must_use]
    pub fn new(session_root: PathBuf) -> Self {
        Self::with_event_handlers(session_root, None, None)
    }

    #[must_use]
    pub fn with_prompt_update_sink(
        session_root: PathBuf,
        prompt_update_sink: Option<PromptUpdateSink>,
    ) -> Self {
        Self::with_event_handlers(session_root, prompt_update_sink, None)
    }

    #[must_use]
    pub fn with_event_handlers(
        session_root: PathBuf,
        prompt_update_sink: Option<PromptUpdateSink>,
        permission_request_handler: Option<AcpPermissionRequestHandler>,
    ) -> Self {
        Self {
            session_root,
            updates: Arc::new(Mutex::new(BTreeMap::new())),
            terminals: Arc::new(Mutex::new(BTreeMap::new())),
            prompt_update_sink,
            permission_request_handler,
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

    async fn tracked_terminal(
        &self,
        terminal_id: &str,
    ) -> acp::Result<Arc<Mutex<TrackedTerminal>>> {
        self.terminals
            .lock()
            .await
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| acp::Error::resource_not_found(Some(terminal_id.to_string())))
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
        let request = map_permission_request(&args);
        let decision = if let Some(handler) = self.permission_request_handler.as_ref() {
            handler(request).await
        } else {
            default_permission_decision(&args)
        };
        let outcome = match decision {
            AcpPermissionDecision::SelectOption { option_id } => {
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    option_id,
                ))
            }
            AcpPermissionDecision::Cancelled => acp::RequestPermissionOutcome::Cancelled,
        };
        Ok(acp::RequestPermissionResponse::new(outcome))
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let session_id = args.session_id.to_string();
        let event = map_session_event(&session_id, &args.update);
        if let Some(sink) = self.prompt_update_sink.as_ref() {
            sink(AcpPromptUpdate::SessionEvent(event.clone()));
        }
        self.mutate_session_log(session_id, |entry| {
            entry.raw_updates.push(format!("{:?}", &args.update));
            entry.events.push(event.clone());
            merge_session_update_into_log(entry, &args.update);
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
        let terminal_id = args.terminal_id.to_string();
        let terminal = self.tracked_terminal(&terminal_id).await?;
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
        let terminal_id = args.terminal_id.to_string();
        let Some(terminal) = self.terminals.lock().await.remove(terminal_id.as_str()) else {
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
        let terminal_id = args.terminal_id.to_string();
        let terminal = self.tracked_terminal(&terminal_id).await?;
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
        let terminal_id = args.terminal_id.to_string();
        let terminal = self.tracked_terminal(&terminal_id).await?;
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

fn render_content_block(content: &acp::ContentBlock) -> AcpContentBlockEvent {
    match content {
        acp::ContentBlock::Text(text) => AcpContentBlockEvent::Text {
            text: text.text.clone(),
        },
        acp::ContentBlock::Image(image) => AcpContentBlockEvent::Image {
            mime_type: image.mime_type.clone(),
            uri: image.uri.clone(),
            data_len: image.data.len(),
        },
        acp::ContentBlock::Audio(audio) => AcpContentBlockEvent::Audio {
            mime_type: audio.mime_type.clone(),
            data_len: audio.data.len(),
        },
        acp::ContentBlock::ResourceLink(resource) => AcpContentBlockEvent::ResourceLink {
            name: resource.name.clone(),
            uri: resource.uri.clone(),
            title: resource.title.clone(),
            description: resource.description.clone(),
            mime_type: resource.mime_type.clone(),
        },
        acp::ContentBlock::Resource(resource) => match &resource.resource {
            acp::EmbeddedResourceResource::TextResourceContents(text) => {
                AcpContentBlockEvent::EmbeddedTextResource {
                    uri: text.uri.clone(),
                    mime_type: text.mime_type.clone(),
                    text: text.text.clone(),
                }
            }
            acp::EmbeddedResourceResource::BlobResourceContents(blob) => {
                AcpContentBlockEvent::EmbeddedBlobResource {
                    uri: blob.uri.clone(),
                    mime_type: blob.mime_type.clone(),
                    byte_len: blob.blob.len(),
                }
            }
            _ => AcpContentBlockEvent::Unsupported {
                description: format!("{resource:?}"),
            },
        },
        _ => AcpContentBlockEvent::Unsupported {
            description: format!("{content:?}"),
        },
    }
}

impl AcpContentBlockEvent {
    fn rendered_text(&self) -> String {
        match self {
            Self::Text { text } => text.clone(),
            Self::Image {
                mime_type,
                uri,
                data_len,
            } => match uri {
                Some(uri) => format!("[image {mime_type} {data_len} bytes {uri}]"),
                None => format!("[image {mime_type} {data_len} bytes]"),
            },
            Self::Audio {
                mime_type,
                data_len,
            } => format!("[audio {mime_type} {data_len} bytes]"),
            Self::ResourceLink {
                name,
                uri,
                title,
                description,
                mime_type,
            } => {
                let mut parts = vec![format!("name={name}"), format!("uri={uri}")];
                if let Some(title) = title {
                    parts.push(format!("title={title}"));
                }
                if let Some(description) = description {
                    parts.push(format!("description={description}"));
                }
                if let Some(mime_type) = mime_type {
                    parts.push(format!("mime={mime_type}"));
                }
                format!("[resource_link {}]", parts.join(" "))
            }
            Self::EmbeddedTextResource {
                uri,
                mime_type,
                text,
            } => match mime_type {
                Some(mime_type) => {
                    format!("[embedded_text_resource uri={uri} mime={mime_type}] {text}")
                }
                None => format!("[embedded_text_resource uri={uri}] {text}"),
            },
            Self::EmbeddedBlobResource {
                uri,
                mime_type,
                byte_len,
            } => match mime_type {
                Some(mime_type) => {
                    format!("[embedded_blob_resource uri={uri} mime={mime_type} bytes={byte_len}]")
                }
                None => format!("[embedded_blob_resource uri={uri} bytes={byte_len}]"),
            },
            Self::Unsupported { description } => format!("[unsupported_content {description}]"),
        }
    }
}

fn merge_session_update_into_log(entry: &mut AcpSessionUpdateLog, update: &acp::SessionUpdate) {
    match update {
        acp::SessionUpdate::UserMessageChunk(_) => {}
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            entry
                .answer
                .push_str(&render_content_block(&chunk.content).rendered_text());
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            entry
                .reasoning
                .push_str(&render_content_block(&chunk.content).rendered_text());
        }
        acp::SessionUpdate::ToolCall(tool_call) => {
            entry.tool_updates.push(format_tool_call_summary(tool_call));
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            entry
                .tool_updates
                .push(format_tool_call_update_summary(update));
        }
        acp::SessionUpdate::Plan(plan) => {
            entry.tool_updates.push(format_plan_summary(plan));
        }
        acp::SessionUpdate::AvailableCommandsUpdate(update) => {
            entry.available_commands = update
                .available_commands
                .iter()
                .map(map_available_command)
                .collect();
        }
        acp::SessionUpdate::CurrentModeUpdate(update) => {
            entry.current_mode_id = Some(update.current_mode_id.to_string());
        }
        acp::SessionUpdate::ConfigOptionUpdate(update) => {
            entry.config_options = update
                .config_options
                .iter()
                .map(map_config_option)
                .collect();
        }
        acp::SessionUpdate::SessionInfoUpdate(update) => {
            if let Some(title) = maybe_undefined_to_option(&update.title) {
                entry.session_title = title;
            }
            if let Some(updated_at) = maybe_undefined_to_option(&update.updated_at) {
                entry.session_updated_at = updated_at;
            }
        }
        _ => {}
    }
}

fn map_session_event(session_id: &str, update: &acp::SessionUpdate) -> AcpSessionEvent {
    let (summary, mapped_update) = match update {
        acp::SessionUpdate::UserMessageChunk(chunk) => {
            let content = render_content_block(&chunk.content);
            let summary = format!("user: {}", content.rendered_text());
            (summary, AcpSessionEventKind::UserMessageChunk { content })
        }
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            let content = render_content_block(&chunk.content);
            let summary = format!("assistant: {}", content.rendered_text());
            (summary, AcpSessionEventKind::AgentMessageChunk { content })
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            let content = render_content_block(&chunk.content);
            let summary = format!("thought: {}", content.rendered_text());
            (summary, AcpSessionEventKind::AgentThoughtChunk { content })
        }
        acp::SessionUpdate::ToolCall(tool_call) => {
            let event = map_tool_call(tool_call);
            let summary = format_tool_call_summary(tool_call);
            (summary, AcpSessionEventKind::ToolCall(event))
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            let event = map_tool_call_update(update);
            let summary = format_tool_call_update_summary(update);
            (summary, AcpSessionEventKind::ToolCallUpdate(event))
        }
        acp::SessionUpdate::Plan(plan) => {
            let event = map_plan(plan);
            let summary = format_plan_summary(plan);
            (summary, AcpSessionEventKind::Plan(event))
        }
        acp::SessionUpdate::AvailableCommandsUpdate(update) => {
            let commands = update
                .available_commands
                .iter()
                .map(map_available_command)
                .collect::<Vec<_>>();
            let summary = if commands.is_empty() {
                "available commands updated (none)".to_string()
            } else {
                format!(
                    "available commands updated: {}",
                    commands
                        .iter()
                        .map(|command| command.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            (
                summary,
                AcpSessionEventKind::AvailableCommandsUpdate { commands },
            )
        }
        acp::SessionUpdate::CurrentModeUpdate(update) => {
            let current_mode_id = update.current_mode_id.to_string();
            (
                format!("current mode updated: {current_mode_id}"),
                AcpSessionEventKind::CurrentModeUpdate { current_mode_id },
            )
        }
        acp::SessionUpdate::ConfigOptionUpdate(update) => {
            let config_options = update
                .config_options
                .iter()
                .map(map_config_option)
                .collect::<Vec<_>>();
            let summary = if config_options.is_empty() {
                "config options updated (none)".to_string()
            } else {
                format!(
                    "config options updated: {}",
                    config_options
                        .iter()
                        .map(|option| format!("{}={}", option.id, option.current_value))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            (
                summary,
                AcpSessionEventKind::ConfigOptionUpdate { config_options },
            )
        }
        acp::SessionUpdate::SessionInfoUpdate(update) => {
            let title = maybe_undefined_to_option(&update.title);
            let updated_at = maybe_undefined_to_option(&update.updated_at);
            let mut parts = Vec::new();
            if let Some(title) = &title {
                parts.push(match title {
                    Some(value) => format!("title={value}"),
                    None => "title=<cleared>".to_string(),
                });
            }
            if let Some(updated_at) = &updated_at {
                parts.push(match updated_at {
                    Some(value) => format!("updated_at={value}"),
                    None => "updated_at=<cleared>".to_string(),
                });
            }
            let summary = if parts.is_empty() {
                "session info updated".to_string()
            } else {
                format!("session info updated: {}", parts.join(", "))
            };
            (
                summary,
                AcpSessionEventKind::SessionInfoUpdate { title, updated_at },
            )
        }
        _ => (
            format!("other session update: {update:?}"),
            AcpSessionEventKind::Other {
                description: format!("{update:?}"),
            },
        ),
    };
    AcpSessionEvent {
        session_id: session_id.to_string(),
        summary,
        update: mapped_update,
    }
}

fn map_tool_call(tool_call: &acp::ToolCall) -> AcpToolCallEvent {
    AcpToolCallEvent {
        tool_call_id: tool_call.tool_call_id.to_string(),
        title: tool_call.title.clone(),
        kind: format_tool_kind(tool_call.kind),
        status: format_tool_call_status(tool_call.status),
        content: tool_call
            .content
            .iter()
            .map(format_tool_call_content)
            .collect(),
        locations: tool_call
            .locations
            .iter()
            .map(format_tool_call_location)
            .collect(),
        raw_input: tool_call.raw_input.as_ref().map(format_json_value),
        raw_output: tool_call.raw_output.as_ref().map(format_json_value),
    }
}

fn map_tool_call_update(update: &acp::ToolCallUpdate) -> AcpToolCallUpdateEvent {
    AcpToolCallUpdateEvent {
        tool_call_id: update.tool_call_id.to_string(),
        title: update.fields.title.clone(),
        kind: update.fields.kind.map(format_tool_kind),
        status: update.fields.status.map(format_tool_call_status),
        content: update
            .fields
            .content
            .as_ref()
            .map(|content| content.iter().map(format_tool_call_content).collect()),
        locations: update
            .fields
            .locations
            .as_ref()
            .map(|locations| locations.iter().map(format_tool_call_location).collect()),
        raw_input: update.fields.raw_input.as_ref().map(format_json_value),
        raw_output: update.fields.raw_output.as_ref().map(format_json_value),
    }
}

fn map_plan(plan: &acp::Plan) -> AcpPlanEvent {
    AcpPlanEvent {
        entries: plan
            .entries
            .iter()
            .map(|entry| AcpPlanEntry {
                content: entry.content.clone(),
                priority: format!("{:?}", entry.priority),
                status: format!("{:?}", entry.status),
            })
            .collect(),
    }
}

fn map_available_command(command: &acp::AvailableCommand) -> AcpAvailableCommand {
    AcpAvailableCommand {
        name: command.name.clone(),
        description: command.description.clone(),
        input_hint: command.input.as_ref().map(|input| match input {
            acp::AvailableCommandInput::Unstructured(input) => input.hint.clone(),
            _ => format!("{input:?}"),
        }),
    }
}

fn map_config_option(option: &acp::SessionConfigOption) -> AcpConfigOption {
    let (current_value, values) = match &option.kind {
        acp::SessionConfigKind::Select(select) => (
            select.current_value.to_string(),
            format_select_options(&select.options),
        ),
        _ => (format!("{:?}", option.kind), Vec::new()),
    };
    AcpConfigOption {
        id: option.id.to_string(),
        name: option.name.clone(),
        description: option.description.clone(),
        category: option.category.as_ref().map(format_config_option_category),
        current_value,
        values,
    }
}

fn map_permission_request(args: &acp::RequestPermissionRequest) -> AcpPermissionRequest {
    AcpPermissionRequest {
        session_id: args.session_id.to_string(),
        tool_call_id: args.tool_call.tool_call_id.to_string(),
        title: args.tool_call.fields.title.clone(),
        kind: args.tool_call.fields.kind.map(format_tool_kind),
        status: args.tool_call.fields.status.map(format_tool_call_status),
        raw_input: args
            .tool_call
            .fields
            .raw_input
            .as_ref()
            .map(format_json_value),
        raw_output: args
            .tool_call
            .fields
            .raw_output
            .as_ref()
            .map(format_json_value),
        options: args
            .options
            .iter()
            .map(|option| AcpPermissionOption {
                option_id: option.option_id.to_string(),
                label: option.name.clone(),
                kind: format_permission_option_kind(option.kind),
            })
            .collect(),
    }
}

fn default_permission_decision(args: &acp::RequestPermissionRequest) -> AcpPermissionDecision {
    args.options
        .first()
        .map(|option| AcpPermissionDecision::SelectOption {
            option_id: option.option_id.to_string(),
        })
        .unwrap_or(AcpPermissionDecision::Cancelled)
}

fn format_tool_call_summary(tool_call: &acp::ToolCall) -> String {
    format!(
        "tool call [{}] {} ({}, {})",
        tool_call.tool_call_id,
        tool_call.title,
        format_tool_kind(tool_call.kind),
        format_tool_call_status(tool_call.status)
    )
}

fn format_tool_call_update_summary(update: &acp::ToolCallUpdate) -> String {
    let mut changes = Vec::new();
    if let Some(title) = &update.fields.title {
        changes.push(format!("title={title}"));
    }
    if let Some(kind) = update.fields.kind {
        changes.push(format!("kind={}", format_tool_kind(kind)));
    }
    if let Some(status) = update.fields.status {
        changes.push(format!("status={}", format_tool_call_status(status)));
    }
    if update.fields.content.is_some() {
        changes.push("content=updated".to_string());
    }
    if update.fields.locations.is_some() {
        changes.push("locations=updated".to_string());
    }
    if update.fields.raw_input.is_some() {
        changes.push("raw_input=updated".to_string());
    }
    if update.fields.raw_output.is_some() {
        changes.push("raw_output=updated".to_string());
    }
    if changes.is_empty() {
        format!("tool update [{}]", update.tool_call_id)
    } else {
        format!(
            "tool update [{}]: {}",
            update.tool_call_id,
            changes.join(", ")
        )
    }
}

fn format_plan_summary(plan: &acp::Plan) -> String {
    if plan.entries.is_empty() {
        return "plan updated (empty)".to_string();
    }
    format!(
        "plan updated: {}",
        plan.entries
            .iter()
            .map(|entry| format!(
                "{} [{:?}/{:?}]",
                entry.content, entry.priority, entry.status
            ))
            .collect::<Vec<_>>()
            .join(" | ")
    )
}

fn format_tool_kind(kind: acp::ToolKind) -> String {
    match kind {
        acp::ToolKind::Read => "read".to_string(),
        acp::ToolKind::Edit => "edit".to_string(),
        acp::ToolKind::Delete => "delete".to_string(),
        acp::ToolKind::Move => "move".to_string(),
        acp::ToolKind::Search => "search".to_string(),
        acp::ToolKind::Execute => "execute".to_string(),
        acp::ToolKind::Think => "think".to_string(),
        acp::ToolKind::Fetch => "fetch".to_string(),
        acp::ToolKind::SwitchMode => "switch_mode".to_string(),
        acp::ToolKind::Other => "other".to_string(),
        _ => format!("{kind:?}"),
    }
}

fn format_tool_call_status(status: acp::ToolCallStatus) -> String {
    match status {
        acp::ToolCallStatus::Pending => "pending".to_string(),
        acp::ToolCallStatus::InProgress => "in_progress".to_string(),
        acp::ToolCallStatus::Completed => "completed".to_string(),
        acp::ToolCallStatus::Failed => "failed".to_string(),
        _ => format!("{status:?}"),
    }
}

fn format_tool_call_content(content: &acp::ToolCallContent) -> String {
    match content {
        acp::ToolCallContent::Content(content) => {
            render_content_block(&content.content).rendered_text()
        }
        acp::ToolCallContent::Diff(diff) => format!(
            "[diff path={} old_len={} new_len={}]",
            diff.path.display(),
            diff.old_text.as_ref().map_or(0, String::len),
            diff.new_text.len()
        ),
        acp::ToolCallContent::Terminal(terminal) => {
            format!("[terminal id={}]", terminal.terminal_id)
        }
        _ => format!("{content:?}"),
    }
}

fn format_tool_call_location(location: &acp::ToolCallLocation) -> String {
    match location.line {
        Some(line) => format!("{}:{}", location.path.display(), line),
        None => location.path.display().to_string(),
    }
}

fn format_select_options(options: &acp::SessionConfigSelectOptions) -> Vec<String> {
    match options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .map(|option| format!("{} ({})", option.name, option.value))
            .collect(),
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| {
                group.options.iter().map(move |option| {
                    format!("{} / {} ({})", group.name, option.name, option.value)
                })
            })
            .collect(),
        _ => vec![format!("{options:?}")],
    }
}

fn format_config_option_category(category: &acp::SessionConfigOptionCategory) -> String {
    match category {
        acp::SessionConfigOptionCategory::Mode => "mode".to_string(),
        acp::SessionConfigOptionCategory::Model => "model".to_string(),
        acp::SessionConfigOptionCategory::ThoughtLevel => "thought_level".to_string(),
        acp::SessionConfigOptionCategory::Other(value) => value.clone(),
        _ => format!("{category:?}"),
    }
}

fn maybe_undefined_to_option(value: &acp::MaybeUndefined<String>) -> Option<Option<String>> {
    match value {
        acp::MaybeUndefined::Undefined => None,
        acp::MaybeUndefined::Null => Some(None),
        acp::MaybeUndefined::Value(value) => Some(Some(value.clone())),
    }
}

fn format_permission_option_kind(kind: acp::PermissionOptionKind) -> String {
    match kind {
        acp::PermissionOptionKind::AllowOnce => "allow_once".to_string(),
        acp::PermissionOptionKind::AllowAlways => "allow_always".to_string(),
        acp::PermissionOptionKind::RejectOnce => "reject_once".to_string(),
        acp::PermissionOptionKind::RejectAlways => "reject_always".to_string(),
        _ => format!("{kind:?}"),
    }
}

fn format_json_value(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
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
    use agent_client_protocol::Client as _;

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
            events: Vec::new(),
            available_commands: Vec::new(),
            current_mode_id: None,
            config_options: Vec::new(),
            session_title: None,
            session_updated_at: None,
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

    #[test]
    fn render_content_block_summarizes_resources() {
        let link = render_content_block(&acp::ContentBlock::ResourceLink(acp::ResourceLink::new(
            "README",
            "file:///workspace/README.md",
        )));
        assert_eq!(
            link.rendered_text(),
            "[resource_link name=README uri=file:///workspace/README.md]"
        );

        let resource = render_content_block(&acp::ContentBlock::Resource(
            acp::EmbeddedResource::new(acp::EmbeddedResourceResource::TextResourceContents(
                acp::TextResourceContents::new("# Heading", "file:///workspace/notes.md")
                    .mime_type("text/markdown"),
            )),
        ));
        assert_eq!(
            resource.rendered_text(),
            "[embedded_text_resource uri=file:///workspace/notes.md mime=text/markdown] # Heading"
        );
    }

    #[test]
    fn permission_request_mapping_preserves_tool_context() {
        let request = acp::RequestPermissionRequest::new(
            "session-1",
            acp::ToolCallUpdate::new(
                "tool-1",
                acp::ToolCallUpdateFields::new()
                    .title("Write output.txt")
                    .kind(acp::ToolKind::Edit)
                    .status(acp::ToolCallStatus::Pending),
            ),
            vec![acp::PermissionOption::new(
                "allow",
                "Allow once",
                acp::PermissionOptionKind::AllowOnce,
            )],
        );

        let mapped = map_permission_request(&request);
        assert_eq!(mapped.session_id, "session-1");
        assert_eq!(mapped.tool_call_id, "tool-1");
        assert_eq!(mapped.title.as_deref(), Some("Write output.txt"));
        assert_eq!(mapped.kind.as_deref(), Some("edit"));
        assert_eq!(mapped.status.as_deref(), Some("pending"));
        assert_eq!(mapped.options.len(), 1);
        assert!(matches!(
            default_permission_decision(&request),
            AcpPermissionDecision::SelectOption { ref option_id } if option_id == "allow"
        ));
    }

    #[tokio::test]
    async fn session_notification_tracks_structured_session_state() {
        let root = temp_dir();
        let client = KlawAcpClient::new(root.clone());
        let session_id = "session-1";

        client
            .session_notification(acp::SessionNotification::new(
                session_id,
                acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new("build")),
            ))
            .await
            .expect("current mode update should succeed");
        client
            .session_notification(acp::SessionNotification::new(
                session_id,
                acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(
                    vec![acp::AvailableCommand::new("create_plan", "Create a plan")],
                )),
            ))
            .await
            .expect("available commands update should succeed");
        client
            .session_notification(acp::SessionNotification::new(
                session_id,
                acp::SessionUpdate::ConfigOptionUpdate(acp::ConfigOptionUpdate::new(vec![
                    acp::SessionConfigOption::select(
                        "mode",
                        "Mode",
                        "build",
                        vec![acp::SessionConfigSelectOption::new("build", "Build")],
                    ),
                ])),
            ))
            .await
            .expect("config option update should succeed");
        client
            .session_notification(acp::SessionNotification::new(
                session_id,
                acp::SessionUpdate::SessionInfoUpdate(
                    acp::SessionInfoUpdate::new()
                        .title("ACP session".to_string())
                        .updated_at("2026-04-02T00:00:00Z".to_string()),
                ),
            ))
            .await
            .expect("session info update should succeed");
        client
            .session_notification(acp::SessionNotification::new(
                session_id,
                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                    acp::ContentBlock::ResourceLink(acp::ResourceLink::new(
                        "README",
                        "file:///workspace/README.md",
                    )),
                )),
            ))
            .await
            .expect("message chunk should succeed");

        let log = client
            .session_log(session_id)
            .await
            .expect("session log should exist");
        assert_eq!(log.current_mode_id.as_deref(), Some("build"));
        assert_eq!(log.available_commands.len(), 1);
        assert_eq!(log.config_options.len(), 1);
        assert_eq!(log.session_title.as_deref(), Some("ACP session"));
        assert_eq!(
            log.session_updated_at.as_deref(),
            Some("2026-04-02T00:00:00Z")
        );
        assert!(
            log.answer
                .contains("[resource_link name=README uri=file:///workspace/README.md]")
        );
        assert!(log.events.len() >= 5);

        let _ = std::fs::remove_dir_all(root);
    }
}
