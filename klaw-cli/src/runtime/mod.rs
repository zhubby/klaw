pub mod gateway_manager;
pub mod service_loop;
pub mod webhook;

use crate::env_check;
use klaw_agent::{
    build_compression_prompt, build_provider_from_config, merge_or_reset_summary,
    parse_conversation_summary, AgentExecutionStreamEvent, ConversationMessage,
    ConversationSummary,
};
use klaw_approval::{
    ApprovalManager, ApprovalResolveDecision, ApprovalStatus, SqliteApprovalManager,
};
use klaw_channel::{
    ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, ChannelStreamEvent,
    ChannelStreamWriter,
};
use klaw_config::{AppConfig, ConfigStore, ToolEnabled};
use klaw_core::{
    compose_runtime_prompt, ensure_workspace_prompt_templates, AgentLoop, AgentRuntimeError,
    AgentTelemetry, CircuitBreakerPolicy, DeadLetterMessage, DeadLetterPolicy, Envelope,
    EnvelopeHeader, ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker,
    InMemoryIdempotencyStore, InMemoryTransport, InboundMessage, MediaReference, OutboundMessage,
    QueueStrategy, RunLimits, RuntimePromptInput, SessionSchedulingPolicy, SkillPromptEntry,
    Subscription, TransportError,
};
use klaw_gateway::GatewayWebhookRequest;
use klaw_heartbeat::{
    should_suppress_output, specs_from_config, CronHeartbeatScheduler, HeartbeatScheduler,
};
use klaw_llm::{ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolDefinition};
use klaw_mcp::{McpBootstrapHandle, McpBootstrapSummary, McpManager};
use klaw_observability::{
    init_observability, ObservabilityConfig, ObservabilityHandle, OtelAgentTelemetry,
};
use klaw_session::{
    ChatRecord, LlmAuditStatus, LlmUsageSource, NewLlmAuditRecord, NewLlmUsageRecord,
    SessionCompressionState, SessionManager, SqliteSessionManager,
};
use klaw_skill::{
    open_default_skills_manager, InstalledSkill, RegistrySource, SkillSourceKind, SkillsManager,
};
use klaw_storage::{open_default_store, CronStorage, DefaultSessionStore};
use klaw_tool::{
    ApplyPatchTool, ApprovalTool, ArchiveTool, CronManagerTool, LocalSearchTool, MemoryTool,
    ShellTool, SkillsManagerTool, SkillsRegistryTool, SubAgentTool, TerminalMultiplexerTool,
    ToolContext, ToolRegistry, WebFetchTool, WebSearchTool,
};
use klaw_util::EnvironmentCheckReport;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;
use tokio::{sync::Mutex, time::timeout};
use tracing::{info, warn};
use uuid::Uuid;

const LLM_AUDIT_QUEUE_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Default)]
pub struct StartupReport {
    pub skill_names: Vec<String>,
    pub tool_names: Vec<String>,
    pub mcp_summary: Option<McpBootstrapSummary>,
}

pub struct RuntimeBundle {
    pub runtime: AgentLoop,
    pub default_provider_id: String,
    pub provider_default_models: BTreeMap<String, String>,
    pub runtime_provider_override: Arc<RwLock<Option<String>>>,
    pub disable_session_commands_for: BTreeSet<String>,
    pub inbound_transport: InMemoryTransport<InboundMessage>,
    pub outbound_transport: InMemoryTransport<OutboundMessage>,
    pub deadletter_transport: InMemoryTransport<DeadLetterMessage>,
    pub idempotency: InMemoryIdempotencyStore,
    pub retry_policy: ExponentialBackoffRetryPolicy,
    pub deadletter_policy: DeadLetterPolicy,
    pub circuit_breaker: InMemoryCircuitBreaker,
    pub subscription: Subscription,
    pub session_store: DefaultSessionStore,
    pub mcp_bootstrap: Option<Mutex<McpBootstrapHandle>>,
    pub startup_report: StartupReport,
    pub observability: Option<ObservabilityHandle>,
    pub conversation_history_limit: usize,
    pub llm_audit_tx: std::sync::mpsc::SyncSender<NewLlmAuditRecord>,
    pub env_check: EnvironmentCheckReport,
}

pub struct SharedChannelRuntime {
    runtime: Arc<RuntimeBundle>,
    background: Arc<service_loop::BackgroundServices>,
}

#[derive(Debug)]
struct UnavailableProvider {
    name: String,
    default_model: String,
    wire_api: Option<String>,
    reason: String,
}

#[async_trait::async_trait]
impl LlmProvider for UnavailableProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn default_model(&self) -> &str {
        &self.default_model
    }

    fn wire_api(&self) -> Option<&str> {
        self.wire_api.as_deref()
    }

    async fn chat(
        &self,
        _messages: Vec<LlmMessage>,
        _tools: Vec<ToolDefinition>,
        _model: Option<&str>,
        _options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        Err(LlmError::provider_unavailable(self.reason.clone()))
    }
}

impl SharedChannelRuntime {
    pub fn new(
        runtime: Arc<RuntimeBundle>,
        background: Arc<service_loop::BackgroundServices>,
    ) -> Self {
        Self {
            runtime,
            background,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ChannelRuntime for SharedChannelRuntime {
    async fn submit(&self, request: ChannelRequest) -> ChannelResult<Option<ChannelResponse>> {
        if !is_channel_commands_disabled(self.runtime.as_ref(), &request.channel)
            && request.input.trim_start().starts_with('/')
        {
            if let Some(response) = handle_im_command(
                self.runtime.as_ref(),
                request.channel.clone(),
                request.session_key.clone(),
                request.chat_id.clone(),
                request.input.clone(),
            )
            .await?
            {
                return Ok(Some(response));
            }
        }

        let route = resolve_session_route(
            self.runtime.as_ref(),
            &request.channel,
            &request.session_key,
            &request.chat_id,
        )
        .await?;
        let maybe_output = submit_and_get_output(
            self.runtime.as_ref(),
            request.channel,
            request.input,
            route.active_session_key,
            request.chat_id,
            "local-user".to_string(),
            route.model_provider,
            route.model,
            request.media_references,
            request.metadata,
        )
        .await?;

        Ok(maybe_output.map(|output| ChannelResponse {
            content: output.content,
            reasoning: output.reasoning,
            metadata: output.metadata,
        }))
    }

    async fn submit_streaming(
        &self,
        request: ChannelRequest,
        writer: &mut dyn ChannelStreamWriter,
    ) -> ChannelResult<Option<ChannelResponse>> {
        if !is_channel_commands_disabled(self.runtime.as_ref(), &request.channel)
            && request.input.trim_start().starts_with('/')
        {
            if let Some(response) = handle_im_command(
                self.runtime.as_ref(),
                request.channel.clone(),
                request.session_key.clone(),
                request.chat_id.clone(),
                request.input.clone(),
            )
            .await?
            {
                writer
                    .write(ChannelStreamEvent::Snapshot(response.clone()))
                    .await?;
                return Ok(Some(response));
            }
        }

        let route = resolve_session_route(
            self.runtime.as_ref(),
            &request.channel,
            &request.session_key,
            &request.chat_id,
        )
        .await?;
        let maybe_output = submit_and_stream_output(
            self.runtime.as_ref(),
            request.channel,
            request.input,
            route.active_session_key,
            request.chat_id,
            route.model_provider,
            route.model,
            request.media_references,
            request.metadata,
            writer,
        )
        .await?;

        Ok(maybe_output.map(|output| ChannelResponse {
            content: output.content,
            reasoning: output.reasoning,
            metadata: output.metadata,
        }))
    }

    fn cron_tick_interval(&self) -> Duration {
        self.background.cron_tick_interval()
    }

    fn runtime_tick_interval(&self) -> Duration {
        self.background.runtime_tick_interval()
    }

    async fn on_cron_tick(&self) {
        self.background.on_cron_tick().await;
    }

    async fn on_runtime_tick(&self) {
        self.background.on_runtime_tick(self.runtime.as_ref()).await;
    }
}

#[derive(Debug, Clone)]
pub struct AssistantOutput {
    pub content: String,
    pub reasoning: Option<String>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";
const META_PROVIDER_KEY: &str = "agent.provider_id";
const META_MODEL_KEY: &str = "agent.model";
const WORKSPACE_PROMPT_DOC_FILES: [&str; 7] = [
    "AGENTS.md",
    "BOOTSTRAP.md",
    "HEARTBEAT.md",
    "IDENTITY.md",
    "SOUL.md",
    "TOOLS.md",
    "USER.md",
];

const RUNTIME_PROMPT_RULES: &str = "Prefer lazy-loading context from files and skills instead of relying on embedded long prompt bodies. Read only what is needed for the current user request.";
const RUNTIME_PROMPT_EXTRA_INSTRUCTIONS: &str = "When local workspace docs are relevant, read them from disk on demand before acting. Do not assume their content without reading the files.\nWhen a task requires remembering or recalling prior context, use the memory tool. Do not rely on ad-hoc markdown memory files.\nFiles under archives/ are read-only source material. Never edit, move, or delete them in place. If you need to transform or modify an archived file, use the archive tool to copy it into workspace first, then operate on the copied file.";

#[derive(Debug, Clone)]
struct SessionRoute {
    active_session_key: String,
    model_provider: String,
    model: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PersistedLlmUsageMetadata {
    request_seq: i64,
    provider: String,
    model: String,
    wire_api: String,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    #[serde(default)]
    cached_input_tokens: Option<i64>,
    #[serde(default)]
    reasoning_tokens: Option<i64>,
    source: String,
    #[serde(default)]
    provider_request_id: Option<String>,
    #[serde(default)]
    provider_response_id: Option<String>,
}

fn is_channel_commands_disabled(runtime: &RuntimeBundle, channel: &str) -> bool {
    runtime.disable_session_commands_for.contains(channel)
}

fn parse_im_command(input: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let rest = trimmed.trim_start_matches('/').trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let command = parts.next()?.trim();
    let arg = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some((command, arg))
}

fn first_arg_token(arg: Option<&str>) -> Option<&str> {
    arg.and_then(|raw| raw.split_whitespace().next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn extract_llm_usage_records(
    metadata: &BTreeMap<String, serde_json::Value>,
) -> Vec<PersistedLlmUsageMetadata> {
    metadata
        .get("llm.usage.records")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

async fn persist_llm_usage_records(
    runtime: &RuntimeBundle,
    session_key: &str,
    chat_id: &str,
    turn_index: i64,
    message_id: &uuid::Uuid,
    metadata: &BTreeMap<String, serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let records = extract_llm_usage_records(metadata);
    if records.is_empty() {
        return Ok(());
    }

    let sessions = session_manager(runtime);
    for record in records {
        let source = LlmUsageSource::parse(record.source.as_str()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid llm usage source: {}", record.source),
            )
        })?;
        sessions
            .append_llm_usage(&NewLlmUsageRecord {
                id: format!("{message_id}:{}", record.request_seq),
                session_key: session_key.to_string(),
                chat_id: chat_id.to_string(),
                turn_index,
                request_seq: record.request_seq,
                provider: record.provider,
                model: record.model,
                wire_api: record.wire_api,
                input_tokens: record.input_tokens,
                output_tokens: record.output_tokens,
                total_tokens: record.total_tokens,
                cached_input_tokens: record.cached_input_tokens,
                reasoning_tokens: record.reasoning_tokens,
                source,
                provider_request_id: record.provider_request_id,
                provider_response_id: record.provider_response_id,
            })
            .await?;
    }
    Ok(())
}

async fn persist_assistant_response_state(
    runtime: &RuntimeBundle,
    session_key: &str,
    chat_id: &str,
    channel: &str,
    turn_index: i64,
    message_id: &uuid::Uuid,
    metadata: &BTreeMap<String, serde_json::Value>,
    agent_record: &ChatRecord,
) {
    if let Err(err) = persist_llm_usage_records(
        runtime,
        session_key,
        chat_id,
        turn_index,
        message_id,
        metadata,
    )
    .await
    {
        warn!(
            error = %err,
            session_key,
            message_id = %message_id,
            "failed to persist llm usage records after response; continuing"
        );
    }

    let sessions = session_manager(runtime);
    if let Err(err) = sessions.append_chat_record(session_key, agent_record).await {
        warn!(
            error = %err,
            session_key,
            message_id = %message_id,
            "failed to append assistant chat record after response; continuing"
        );
    }

    if let Err(err) = sessions.complete_turn(session_key, chat_id, channel).await {
        warn!(
            error = %err,
            session_key,
            message_id = %message_id,
            "failed to complete session turn after response; continuing"
        );
    }
}

fn enqueue_llm_audit_records_from_outcome(
    runtime: &RuntimeBundle,
    turn_index: i64,
    outcome: &klaw_core::ProcessOutcome,
) {
    let (Some(message_id), Some(session_key), Some(chat_id)) = (
        outcome.audit_message_id,
        outcome.audit_session_key.as_deref(),
        outcome.audit_chat_id.as_deref(),
    ) else {
        return;
    };
    for (index, record) in outcome.llm_audits.iter().enumerate() {
        let payload = NewLlmAuditRecord {
            id: format!("{message_id}:audit:{}", index + 1),
            session_key: session_key.to_string(),
            chat_id: chat_id.to_string(),
            turn_index,
            request_seq: (index as i64) + 1,
            provider: record.provider.clone(),
            model: record.model.clone(),
            wire_api: record.wire_api.clone(),
            status: match record.status {
                klaw_llm::LlmAuditStatus::Success => LlmAuditStatus::Success,
                klaw_llm::LlmAuditStatus::Failed => LlmAuditStatus::Failed,
            },
            error_code: record.error_code.clone(),
            error_message: record.error_message.clone(),
            provider_request_id: record.provider_request_id.clone(),
            provider_response_id: record.provider_response_id.clone(),
            request_body_json: serialize_json_string(&record.request_body),
            response_body_json: record.response_body.as_ref().map(serialize_json_string),
            requested_at_ms: record.requested_at_ms,
            responded_at_ms: record.responded_at_ms,
        };
        if let Err(err) = runtime.llm_audit_tx.try_send(payload) {
            warn!(error = %err, session_key, "llm audit queue full or disconnected; dropping record");
        }
    }
}

fn serialize_json_string(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

async fn execute_approved_shell(
    runtime: &RuntimeBundle,
    approval_id: &str,
    session_key: &str,
    command_text: &str,
) -> Result<String, Box<dyn Error>> {
    let Some(shell_tool) = runtime.runtime.tools.get("shell") else {
        return Ok(
            "⚠️ shell tool unavailable; approval has been recorded but command was not executed."
                .to_string(),
        );
    };
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "shell.approval_id".to_string(),
        serde_json::Value::String(approval_id.to_string()),
    );
    let output = shell_tool
        .execute(
            json!({ "command": command_text }),
            &ToolContext {
                session_key: session_key.to_string(),
                metadata,
            },
        )
        .await;
    match output {
        Ok(output) => Ok(output
            .content_for_user
            .unwrap_or_else(|| output.content_for_model)),
        Err(err) => Ok(format!("tool `shell` failed: {err}")),
    }
}

async fn resolve_session_route(
    runtime: &RuntimeBundle,
    channel: &str,
    base_session_key: &str,
    chat_id: &str,
) -> Result<SessionRoute, Box<dyn Error>> {
    let sessions = session_manager(runtime);
    let default_provider_id = runtime_default_provider_id(runtime);
    let base = sessions
        .get_or_create_session_state(
            base_session_key,
            chat_id,
            channel,
            &default_provider_id,
            default_model_for_provider(runtime, &default_provider_id),
        )
        .await?;
    let active_session_key = base
        .active_session_key
        .clone()
        .unwrap_or_else(|| base_session_key.to_string());
    let provider = base.model_provider.clone().unwrap_or(default_provider_id);
    let model_default = default_model_for_provider(runtime, &provider).to_string();
    let active = sessions
        .get_or_create_session_state(
            &active_session_key,
            chat_id,
            channel,
            &provider,
            &model_default,
        )
        .await?;
    let model_provider = active.model_provider.unwrap_or(provider);
    let model = active.model.unwrap_or(model_default);
    Ok(SessionRoute {
        active_session_key,
        model_provider,
        model,
    })
}

fn resolve_new_session_target(runtime: &RuntimeBundle) -> (String, String) {
    if let Some(provider_id) = runtime_provider_override(runtime) {
        return (
            provider_id.clone(),
            default_model_for_provider(runtime, &provider_id).to_string(),
        );
    }

    ConfigStore::open(None)
        .and_then(|store| store.reload())
        .map(|snapshot| resolve_new_session_target_from_config(&snapshot.config))
        .unwrap_or_else(|_| {
            (
                runtime.default_provider_id.clone(),
                default_model_for_provider(runtime, &runtime.default_provider_id).to_string(),
            )
        })
}

fn runtime_provider_override(runtime: &RuntimeBundle) -> Option<String> {
    runtime
        .runtime_provider_override
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

fn runtime_default_provider_id(runtime: &RuntimeBundle) -> String {
    runtime_provider_override(runtime).unwrap_or_else(|| runtime.default_provider_id.clone())
}

fn resolve_new_session_target_from_config(config: &AppConfig) -> (String, String) {
    let provider_id = config.model_provider.clone();
    let model = configured_default_model(config, &provider_id);
    (provider_id, model)
}

pub fn set_runtime_provider_override(
    runtime: &RuntimeBundle,
    provider_id: Option<&str>,
) -> Result<(String, String), Box<dyn Error>> {
    let (next, active_provider) = normalize_runtime_provider_override(
        &runtime.provider_default_models,
        &runtime.default_provider_id,
        provider_id,
    )?;

    let mut guard = runtime
        .runtime_provider_override
        .write()
        .unwrap_or_else(|err| err.into_inner());
    *guard = next;
    drop(guard);

    let active_model = default_model_for_provider(runtime, &active_provider).to_string();
    Ok((active_provider, active_model))
}

fn normalize_runtime_provider_override(
    provider_default_models: &BTreeMap<String, String>,
    default_provider_id: &str,
    provider_id: Option<&str>,
) -> Result<(Option<String>, String), Box<dyn Error>> {
    let next = match provider_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(provider_id) => {
            if !provider_default_models.contains_key(provider_id) {
                let all = provider_default_models
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(config_err(format!(
                    "unknown runtime provider '{provider_id}', available: {all}"
                )));
            }
            Some(provider_id.to_string())
        }
        None => None,
    };
    let active_provider = next
        .clone()
        .unwrap_or_else(|| default_provider_id.to_string());
    Ok((next, active_provider))
}

fn default_model_for_provider<'a>(runtime: &'a RuntimeBundle, provider_id: &str) -> &'a str {
    runtime
        .provider_default_models
        .get(provider_id)
        .map(String::as_str)
        .unwrap_or(runtime.runtime.active_model.as_str())
}

fn render_help_text(runtime: &RuntimeBundle) -> String {
    let mut lines = vec![
        "📘 **Command Center**".to_string(),
        String::new(),
        "```text".to_string(),
    ];
    lines.push(format!("{:<24}{}", "/new", "Start a new session context"));
    lines.push(format!("{:<24}{}", "/help", "Show this help"));
    if runtime.provider_default_models.len() > 1 {
        lines.push(format!(
            "{:<24}{}",
            "/model_provider", "List available providers"
        ));
        lines.push(format!(
            "{:<24}{}",
            "/model_provider <id>", "Switch provider for current session"
        ));
    }
    lines.push(format!("{:<24}{}", "/model", "Show current model"));
    lines.push(format!(
        "{:<24}{}",
        "/model <model_name>", "Update current model for current session"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/approve <approval_id>", "Approve a pending tool action"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/reject <approval_id>", "Reject a pending tool action"
    ));
    lines.push("```".to_string());
    if runtime.provider_default_models.len() > 1 {
        let providers = runtime
            .provider_default_models
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(String::new());
        lines.push(format!("🧩 Providers: {providers}"));
    }
    lines.join("\n")
}

fn approval_manager(runtime: &RuntimeBundle) -> SqliteApprovalManager {
    SqliteApprovalManager::from_store(runtime.session_store.clone())
}

fn session_manager(runtime: &RuntimeBundle) -> SqliteSessionManager {
    SqliteSessionManager::from_store(runtime.session_store.clone())
}

async fn handle_im_command(
    runtime: &RuntimeBundle,
    channel: String,
    base_session_key: String,
    chat_id: String,
    input: String,
) -> Result<Option<ChannelResponse>, Box<dyn Error>> {
    let Some((command, arg)) = parse_im_command(&input) else {
        return Ok(None);
    };
    let route = resolve_session_route(runtime, &channel, &base_session_key, &chat_id).await?;
    let response = match command {
        "help" | "start" => ChannelResponse {
            content: render_help_text(runtime),
            reasoning: None,
            metadata: BTreeMap::new(),
        },
        "new" => {
            let new_session_key = format!("{base_session_key}:{}", Uuid::new_v4().simple());
            let (new_session_provider, new_session_model) = resolve_new_session_target(runtime);
            let sessions = session_manager(runtime);
            sessions
                .get_or_create_session_state(
                    &new_session_key,
                    &chat_id,
                    &channel,
                    &new_session_provider,
                    &new_session_model,
                )
                .await?;
            sessions
                .set_active_session(&base_session_key, &chat_id, &channel, &new_session_key)
                .await?;
            ChannelResponse {
                content: format!(
                    "🆕 **New session started**\n\n🧵 Session: `{new_session_key}`\n🧩 Provider: `{}`\n🤖 Model: `{}`",
                    new_session_provider, new_session_model
                ),
                reasoning: None,
                metadata: BTreeMap::new(),
            }
        }
        "model_provider" => {
            if runtime.provider_default_models.len() <= 1 && arg.is_none() {
                return Ok(Some(ChannelResponse {
                    content: "ℹ️ Only one provider is configured, so switching is not required."
                        .to_string(),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }));
            }
            if let Some(provider_id) = first_arg_token(arg) {
                let Some(default_model) = runtime.provider_default_models.get(provider_id) else {
                    let all = runtime
                        .provider_default_models
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Ok(Some(ChannelResponse {
                        content: format!(
                            "❌ Unknown provider: `{provider_id}`\n🧩 Available: {all}"
                        ),
                        reasoning: None,
                        metadata: BTreeMap::new(),
                    }));
                };
                let sessions = session_manager(runtime);
                sessions
                    .set_model_provider(
                        &route.active_session_key,
                        &chat_id,
                        &channel,
                        provider_id,
                        default_model,
                    )
                    .await?;
                sessions
                    .set_model_provider(
                        &base_session_key,
                        &chat_id,
                        &channel,
                        provider_id,
                        default_model,
                    )
                    .await?;
                ChannelResponse {
                    content: format!(
                        "✅ **Provider switched**\n\n🧩 Provider: `{provider_id}`\n🤖 Model: `{default_model}`"
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }
            } else {
                let lines = runtime
                    .provider_default_models
                    .iter()
                    .map(|(id, model)| {
                        if id == &route.model_provider {
                            format!("• `{id}`  ← current (default: `{model}`)")
                        } else {
                            format!("• `{id}`  (default: `{model}`)")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                ChannelResponse {
                    content: format!("🧩 **Providers**\n\n{lines}"),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }
            }
        }
        "model" => {
            if let Some(model) = first_arg_token(arg) {
                if model.trim().is_empty() {
                    return Ok(Some(ChannelResponse {
                        content: "❌ Model name cannot be empty.".to_string(),
                        reasoning: None,
                        metadata: BTreeMap::new(),
                    }));
                }
                let sessions = session_manager(runtime);
                sessions
                    .set_model(&route.active_session_key, &chat_id, &channel, model)
                    .await?;
                sessions
                    .set_model(&base_session_key, &chat_id, &channel, model)
                    .await?;
                ChannelResponse {
                    content: format!(
                        "✅ **Model updated**\n\n🧩 Provider: `{}`\n🤖 Model: `{model}`",
                        route.model_provider
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }
            } else {
                ChannelResponse {
                    content: format!(
                        "🤖 **Current model**\n\n🧩 Provider: `{}`\n🤖 Model: `{}`",
                        route.model_provider, route.model
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }
            }
        }
        "approve" => {
            let Some(approval_id) = first_arg_token(arg) else {
                return Ok(Some(ChannelResponse {
                    content: "❌ Usage: `/approve <approval_id>`".to_string(),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }));
            };
            let manager = approval_manager(runtime);
            let approval = match manager.get_approval(approval_id).await {
                Ok(approval) => approval,
                Err(_) => {
                    return Ok(Some(ChannelResponse {
                        content: format!("❌ Approval not found: `{approval_id}`"),
                        reasoning: None,
                        metadata: BTreeMap::new(),
                    }));
                }
            };
            if approval.session_key != route.active_session_key
                && approval.session_key != base_session_key
            {
                return Ok(Some(ChannelResponse {
                    content: format!(
                        "❌ Approval `{approval_id}` does not belong to current session."
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }));
            }
            match approval.status {
                ApprovalStatus::Pending => {
                    if approval.expires_at_ms < now_ms() {
                        let _ = manager
                            .resolve_approval(
                                approval_id,
                                ApprovalResolveDecision::Approve,
                                Some("channel-user"),
                                now_ms(),
                            )
                            .await?;
                        return Ok(Some(ChannelResponse {
                            content: format!("⌛ Approval expired: `{approval_id}`"),
                            reasoning: None,
                            metadata: BTreeMap::new(),
                        }));
                    }
                    let approved = manager
                        .resolve_approval(
                            approval_id,
                            ApprovalResolveDecision::Approve,
                            Some("channel-user"),
                            now_ms(),
                        )
                        .await?
                        .approval;
                    if approved.tool_name != "shell" {
                        return Ok(Some(ChannelResponse {
                            content: format!(
                                "✅ Approval granted: `{}` (`{}`).\n\n请重试之前触发审批的操作。",
                                approved.id, approved.tool_name
                            ),
                            reasoning: None,
                            metadata: BTreeMap::new(),
                        }));
                    }
                    let execution_result = execute_approved_shell(
                        runtime,
                        &approved.id,
                        &approved.session_key,
                        &approved.command_text,
                    )
                    .await?;
                    let model_followup_input = format!(
                        "审批已通过并已执行命令。请基于以下执行结果给出最终回复。\n\
                        要求：\n\
                        1) 先说明成功/失败\n\
                        2) 如果失败，指出最关键原因和下一步建议\n\
                        3) 不要再调用任何工具\n\n\
                        approval_id: {}\n\
                        command: {}\n\
                        shell_result:\n{}",
                        approved.id, approved.command_preview, execution_result
                    );
                    let maybe_output = submit_and_get_output(
                        runtime,
                        channel.clone(),
                        model_followup_input,
                        approved.session_key.clone(),
                        chat_id.clone(),
                        "local-user".to_string(),
                        route.model_provider.clone(),
                        route.model.clone(),
                        Vec::new(),
                        BTreeMap::new(),
                    )
                    .await?;
                    match maybe_output {
                        Some(output) => ChannelResponse {
                            content: output.content,
                            reasoning: output.reasoning,
                            metadata: output.metadata,
                        },
                        None => ChannelResponse {
                            content: format!(
                                "✅ **Approval granted and command executed**\n\n{}",
                                execution_result
                            ),
                            reasoning: None,
                            metadata: BTreeMap::new(),
                        },
                    }
                }
                other => ChannelResponse {
                    content: format_approve_already_handled_message(approval_id, other),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                },
            }
        }
        "reject" => {
            let Some(approval_id) = first_arg_token(arg) else {
                return Ok(Some(ChannelResponse {
                    content: "❌ Usage: `/reject <approval_id>`".to_string(),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }));
            };
            let manager = approval_manager(runtime);
            let approval = match manager.get_approval(approval_id).await {
                Ok(approval) => approval,
                Err(_) => {
                    return Ok(Some(ChannelResponse {
                        content: format!("❌ Approval not found: `{approval_id}`"),
                        reasoning: None,
                        metadata: BTreeMap::new(),
                    }));
                }
            };
            if approval.session_key != route.active_session_key
                && approval.session_key != base_session_key
            {
                return Ok(Some(ChannelResponse {
                    content: format!(
                        "❌ Approval `{approval_id}` does not belong to current session."
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                }));
            }
            match approval.status {
                ApprovalStatus::Pending => {
                    if approval.expires_at_ms < now_ms() {
                        let _ = manager
                            .resolve_approval(
                                approval_id,
                                ApprovalResolveDecision::Reject,
                                Some("channel-user"),
                                now_ms(),
                            )
                            .await?;
                        return Ok(Some(ChannelResponse {
                            content: format!("⌛ Approval expired: `{approval_id}`"),
                            reasoning: None,
                            metadata: BTreeMap::new(),
                        }));
                    }
                    manager
                        .resolve_approval(
                            approval_id,
                            ApprovalResolveDecision::Reject,
                            Some("channel-user"),
                            now_ms(),
                        )
                        .await?;
                    ChannelResponse {
                        content: format!(
                            "🛑 Approval rejected: `{approval_id}` (`{}`).",
                            approval.tool_name
                        ),
                        reasoning: None,
                        metadata: BTreeMap::new(),
                    }
                }
                other => ChannelResponse {
                    content: format!(
                        "ℹ️ Approval `{approval_id}` is already `{}`.",
                        other.as_str()
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                },
            }
        }
        other => {
            let help = render_help_text(runtime);
            ChannelResponse {
                content: format!("❌ Unknown command: `/{other}`\n\n{help}"),
                reasoning: None,
                metadata: BTreeMap::new(),
            }
        }
    };
    Ok(Some(response))
}

pub async fn build_runtime_bundle(config: &AppConfig) -> Result<RuntimeBundle, Box<dyn Error>> {
    info!(
        provider = %config.model_provider,
        "building runtime bundle"
    );
    let mut provider_registry = BTreeMap::new();
    let mut provider_default_models = BTreeMap::new();
    for provider_id in config.model_providers.keys() {
        let default_model = configured_default_model(config, provider_id);
        match build_provider_from_config(config, provider_id) {
            Ok(instance) => {
                provider_default_models.insert(provider_id.clone(), instance.default_model.clone());
                provider_registry.insert(provider_id.clone(), instance.provider);
            }
            Err(err) => {
                warn!(
                    provider_id = provider_id.as_str(),
                    error = %err,
                    "provider is configured but unavailable at startup; using unavailable placeholder"
                );
                provider_default_models.insert(provider_id.clone(), default_model.clone());
                provider_registry.insert(
                    provider_id.clone(),
                    Arc::new(build_unavailable_provider(
                        config,
                        provider_id,
                        default_model,
                        err.to_string(),
                    )),
                );
            }
        }
    }
    let default_provider = provider_registry
        .get(&config.model_provider)
        .cloned()
        .ok_or_else(|| {
            config_err(format!(
                "default provider '{}' is missing",
                config.model_provider
            ))
        })?;
    let default_model = provider_default_models
        .get(&config.model_provider)
        .cloned()
        .ok_or_else(|| {
            config_err(format!(
                "default model for provider '{}' is missing",
                config.model_provider
            ))
        })?;
    let session_store = open_default_store().await?;
    reconcile_heartbeats(config, &session_store)
        .await
        .map_err(|err| config_err(format!("heartbeat reconcile failed: {err}")))?;
    let mut tools = ToolRegistry::default();
    if config.tools.archive.enabled() {
        tools.register(ArchiveTool::open_default(config).await?);
    }
    if config.tools.apply_patch.enabled() {
        tools.register(ApplyPatchTool::new(config));
    }
    if config.tools.shell.enabled() {
        tools.register(ShellTool::with_store(config, session_store.clone()));
    }
    if config.tools.approval.enabled() {
        tools.register(ApprovalTool::with_manager(
            SqliteApprovalManager::from_store(session_store.clone()),
        ));
    }
    if config.tools.local_search.enabled() {
        tools.register(LocalSearchTool::new());
    }
    if config.tools.terminal_multiplexers.enabled() {
        tools.register(TerminalMultiplexerTool::new());
    }
    if config.tools.cron_manager.enabled() {
        tools.register(CronManagerTool::with_store(session_store.clone()));
    }
    if config.tools.skills_registry.enabled() && !config.skills.registries.is_empty() {
        info!(
            sources = config.skills.registries.len(),
            source_names = ?config.skills.registries.keys().cloned().collect::<Vec<_>>(),
            "registering skills registry tool"
        );
        match SkillsRegistryTool::open_default(config) {
            Ok(tool) => tools.register(tool),
            Err(err) => {
                warn!("skills registry tool disabled: {err}");
            }
        }
    } else if !config.tools.skills_registry.enabled() {
        info!("skills registry tool disabled by config");
    } else {
        info!("skills registry tool disabled: no configured sources");
    }
    if config.tools.skills_manager.enabled() {
        match SkillsManagerTool::open_default(config) {
            Ok(tool) => tools.register(tool),
            Err(err) => {
                warn!("skills manager tool disabled: {err}");
            }
        }
    } else {
        info!("skills manager tool disabled by config");
    }
    if config.tools.memory.enabled() {
        tools.register(MemoryTool::open_default(config).await?);
    }
    if config.tools.web_fetch.enabled() {
        tools.register(WebFetchTool::new(config));
    }
    if config.tools.web_search.enabled() {
        tools.register(WebSearchTool::new(config)?);
    }

    let configured_mcp_servers = config
        .mcp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .count();
    info!(
        enabled = config.mcp.enabled,
        configured_servers = configured_mcp_servers,
        startup_timeout_seconds = config.mcp.startup_timeout_seconds,
        "bootstrapping mcp servers"
    );
    if config.tools.sub_agent.enabled() {
        let parent_tools = tools.clone();
        tools.register(SubAgentTool::new(Arc::new(config.clone()), parent_tools));
    }

    let mcp_bootstrap = if config.mcp.enabled && configured_mcp_servers > 0 {
        Some(Mutex::new(McpManager::spawn_bootstrap(
            config.mcp.clone(),
            tools.clone(),
        )))
    } else {
        None
    };

    ensure_workspace_prompt_templates_if_possible().await;
    let loaded_skills = load_skills_system_prompt(config).await;
    let skill_names = loaded_skills.skill_names.clone();
    let system_prompt = compose_runtime_prompt(RuntimePromptInput {
        runtime_metadata: None,
        rules: Some(RUNTIME_PROMPT_RULES.to_string()),
        local_docs: Some(format_workspace_docs_for_prompt()),
        additional_instructions: Some(RUNTIME_PROMPT_EXTRA_INSTRUCTIONS.to_string()),
        skills: loaded_skills.skill_entries,
    });

    let observability = init_observability_from_config(config).await;
    let llm_audit_tx = spawn_llm_audit_writer(session_store.clone());
    let telemetry: Option<Arc<dyn AgentTelemetry>> = observability.as_ref().map(|handle| {
        Arc::new(OtelAgentTelemetry::from_handle(handle, "klaw")) as Arc<dyn AgentTelemetry>
    });

    let mut runtime = AgentLoop::new_with_identity(
        RunLimits {
            max_tool_iterations: 0,
            max_tool_calls: 0,
            token_budget: 0,
            agent_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(8),
        },
        SessionSchedulingPolicy {
            strategy: QueueStrategy::Collect,
            max_queue_depth: 32,
            lock_ttl: Duration::from_secs(15),
        },
        default_provider,
        config.model_provider.clone(),
        default_model,
        tools,
    )
    .with_provider_registry(provider_registry)
    .with_system_prompt(system_prompt);

    if let Some(ref tel) = telemetry {
        runtime = runtime.with_telemetry(Arc::clone(tel));
    }

    let env_check = env_check::check_environment();

    info!(
        tool_count = runtime.tools.list().len(),
        observability_enabled = observability.is_some(),
        "runtime bundle ready"
    );

    Ok(RuntimeBundle {
        default_provider_id: config.model_provider.clone(),
        provider_default_models,
        runtime_provider_override: Arc::new(RwLock::new(None)),
        disable_session_commands_for: config
            .channels
            .disable_session_commands_for
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect(),
        startup_report: StartupReport {
            skill_names,
            tool_names: runtime.tools.list(),
            mcp_summary: None,
        },
        runtime,
        inbound_transport: InMemoryTransport::new(),
        outbound_transport: InMemoryTransport::new(),
        deadletter_transport: InMemoryTransport::new(),
        idempotency: InMemoryIdempotencyStore::default(),
        retry_policy: ExponentialBackoffRetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(150),
            max_delay: Duration::from_secs(2),
            jitter_ratio: 0.0,
        },
        deadletter_policy: DeadLetterPolicy {
            topic: "agent.dlq",
            max_payload_bytes: 1024 * 1024,
            include_error_stack: false,
        },
        circuit_breaker: InMemoryCircuitBreaker::new(CircuitBreakerPolicy {
            failure_threshold: 5,
            open_interval: Duration::from_secs(3),
            half_open_max_requests: 1,
        }),
        subscription: Subscription {
            topic: "agent.inbound",
            consumer_group: "stdio".to_string(),
            visibility_timeout: Duration::from_secs(10),
        },
        session_store,
        mcp_bootstrap,
        observability,
        conversation_history_limit: config.conversation_history_limit,
        llm_audit_tx,
        env_check,
    })
}

fn spawn_llm_audit_writer(
    session_store: DefaultSessionStore,
) -> std::sync::mpsc::SyncSender<NewLlmAuditRecord> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<NewLlmAuditRecord>(LLM_AUDIT_QUEUE_CAPACITY);
    std::thread::Builder::new()
        .name("klaw-llm-audit-writer".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    warn!(error = %err, "failed to start llm audit writer runtime");
                    return;
                }
            };
            let manager = SqliteSessionManager::from_store(session_store);
            for record in rx {
                if let Err(err) = runtime.block_on(manager.append_llm_audit(&record)) {
                    warn!(error = %err, audit_id = record.id.as_str(), "failed to persist llm audit record");
                }
            }
        })
        .expect("llm audit writer should start");
    tx
}

pub async fn shutdown_runtime_bundle(runtime: &RuntimeBundle) -> Result<(), Box<dyn Error>> {
    if let Some(handle) = &runtime.mcp_bootstrap {
        info!("shutting down runtime mcp servers");
        let mut guard = handle.lock().await;
        let shutdown_deadline = Duration::from_secs(2);
        match timeout(shutdown_deadline, guard.shutdown()).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                return Err(config_err(format!("mcp shutdown failed: {err}")));
            }
            Err(_) => {
                warn!(
                    timeout_seconds = shutdown_deadline.as_secs(),
                    "mcp shutdown timed out; continuing process exit"
                );
            }
        }
    }

    if let Some(handle) = &runtime.observability {
        info!("shutting down observability");
        handle.shutdown().await;
    }

    Ok(())
}

pub async fn reload_runtime_skills_prompt(
    runtime: &RuntimeBundle,
) -> Result<Vec<String>, Box<dyn Error>> {
    let store = ConfigStore::open(None)?;
    let snapshot = store.snapshot();
    ensure_workspace_prompt_templates_if_possible().await;
    let loaded_skills = load_skills_system_prompt(&snapshot.config).await;
    let skill_names = loaded_skills.skill_names.clone();
    let system_prompt = compose_runtime_prompt(RuntimePromptInput {
        runtime_metadata: None,
        rules: Some(RUNTIME_PROMPT_RULES.to_string()),
        local_docs: Some(format_workspace_docs_for_prompt()),
        additional_instructions: Some(RUNTIME_PROMPT_EXTRA_INSTRUCTIONS.to_string()),
        skills: loaded_skills.skill_entries,
    });
    runtime.runtime.set_system_prompt(system_prompt);
    info!(skills = ?skill_names, "reloaded runtime skills prompt");
    Ok(skill_names)
}

#[derive(Debug, Clone, Default)]
struct LoadedSkillsPrompt {
    skill_entries: Vec<SkillPromptEntry>,
    skill_names: Vec<String>,
}

async fn load_skills_system_prompt(config: &AppConfig) -> LoadedSkillsPrompt {
    info!("loading local skills for system prompt");
    let store = match open_default_skills_manager() {
        Ok(store) => store,
        Err(err) => {
            warn!("failed to open default skill store: {err}");
            return LoadedSkillsPrompt::default();
        }
    };

    let sources: Vec<RegistrySource> = config
        .skills
        .registries
        .iter()
        .map(|(name, registry)| RegistrySource {
            name: name.clone(),
            address: registry.address.clone(),
        })
        .collect();
    let installed: Vec<InstalledSkill> = config
        .skills
        .registries
        .iter()
        .flat_map(|(registry_name, registry)| {
            registry.installed.iter().map(|skill_name| InstalledSkill {
                registry: registry_name.clone(),
                name: skill_name.clone(),
            })
        })
        .collect();
    match store
        .sync_registry_installed_skills(&sources, &installed, config.skills.sync_timeout)
        .await
    {
        Ok(report) => {
            info!(
                synced_registries = ?report.synced_registries,
                installed_skills = ?report.installed_skills,
                removed_skills = ?report.removed_skills,
                "registry skills sync completed"
            );
        }
        Err(err) => {
            warn!("failed to sync registry-installed skills: {err}");
        }
    }

    let skills = match store.load_all_installed_skill_markdowns().await {
        Ok(items) => items,
        Err(err) => {
            warn!("failed to load local skills: {err}");
            return LoadedSkillsPrompt::default();
        }
    };

    let skill_names: Vec<String> = skills.iter().map(|skill| skill.name.clone()).collect();
    info!(
        count = skill_names.len(),
        names = ?skill_names,
        "loaded local skills for runtime prompt shortlist"
    );

    let skill_entries = skills
        .into_iter()
        .map(|skill| SkillPromptEntry {
            name: skill.name,
            path: skill.local_path.display().to_string(),
            description: extract_skill_short_description(&skill.content),
            source: format_skill_source(&skill.source_kind, skill.registry.as_deref(), skill.stale),
        })
        .collect();

    LoadedSkillsPrompt {
        skill_entries,
        skill_names,
    }
}

fn extract_skill_short_description(markdown: &str) -> String {
    let line = markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_default();

    if line.is_empty() {
        return "no description".to_string();
    }

    const MAX_LEN: usize = 180;
    if line.chars().count() <= MAX_LEN {
        return line.to_string();
    }

    let mut trimmed = line.chars().take(MAX_LEN).collect::<String>();
    trimmed.push_str("...");
    trimmed
}

fn format_skill_source(
    source_kind: &SkillSourceKind,
    registry: Option<&str>,
    stale: Option<bool>,
) -> String {
    let mut source = match source_kind {
        SkillSourceKind::Local => "workspace/local".to_string(),
        SkillSourceKind::Registry => format!(
            "managed/{}",
            registry
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("registry")
        ),
    };
    if stale.unwrap_or(false) {
        source.push_str(" (stale)");
    }
    source
}

async fn ensure_workspace_prompt_templates_if_possible() {
    if let Err(err) = ensure_workspace_prompt_templates().await {
        warn!("failed to initialize workspace prompt templates: {err}");
    }
}

fn format_workspace_docs_for_prompt() -> String {
    let base = std::env::var("HOME")
        .map(|home| format!("{home}/.klaw/workspace"))
        .unwrap_or_else(|_| "~/.klaw/workspace".to_string());
    let docs = WORKSPACE_PROMPT_DOC_FILES
        .iter()
        .map(|name| format!("- {base}/{name}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Read these workspace docs on demand when relevant:\n{docs}\n\
\n\
Recommended usage:\n\
- Start with AGENTS.md and USER.md for baseline behavior and user preferences.\n\
- Read TOOLS.md before tool-heavy tasks or environment-specific operations.\n\
- Read HEARTBEAT.md only for heartbeat/autonomous polling turns.\n\
- Read BOOTSTRAP.md only during first-run initialization or cold-start setup.\n\
- Use the memory tool for durable memory; do not use markdown files as memory storage."
    )
}

fn trim_conversation_history(
    mut conversation_history: Vec<ChatRecord>,
    limit: usize,
) -> Vec<ChatRecord> {
    if limit == 0 || conversation_history.len() <= limit {
        return conversation_history;
    }
    let keep_from = conversation_history.len() - limit;
    conversation_history.split_off(keep_from)
}

fn compression_trigger_interval(limit: usize) -> usize {
    (limit / 2).max(1)
}

fn should_trigger_compression(
    last_compressed_len: usize,
    history_len: usize,
    limit: usize,
) -> bool {
    if limit == 0 {
        return false;
    }
    history_len.saturating_sub(last_compressed_len) >= compression_trigger_interval(limit)
}

fn to_conversation_messages(records: &[ChatRecord]) -> Vec<ConversationMessage> {
    records
        .iter()
        .map(|record| ConversationMessage {
            role: record.role.clone(),
            content: record.content.clone(),
        })
        .collect()
}

fn build_history_for_model(
    full_history: Vec<ChatRecord>,
    limit: usize,
    summary: Option<&ConversationSummary>,
) -> Vec<ChatRecord> {
    let mut trimmed = trim_conversation_history(full_history, limit);
    let Some(summary) = summary else {
        return trimmed;
    };

    if limit > 0 && trimmed.len() >= limit {
        trimmed = trim_conversation_history(trimmed, limit.saturating_sub(1));
    }

    let summary_json = serde_json::to_string(summary).unwrap_or_else(|_| "{}".to_string());
    let mut merged = Vec::with_capacity(trimmed.len() + 1);
    merged.push(ChatRecord::new(
        "system",
        format!("Conversation Summary (JSON): {summary_json}"),
        None,
    ));
    merged.extend(trimmed);
    merged
}

async fn run_structured_compression(
    provider: Arc<dyn LlmProvider>,
    model: &str,
    old_summary: Option<&ConversationSummary>,
    new_messages: &[ConversationMessage],
) -> Option<String> {
    let prompt = build_compression_prompt(old_summary, new_messages);
    let request = vec![LlmMessage {
        role: "user".to_string(),
        content: prompt,
        media: Vec::new(),
        tool_calls: None,
        tool_call_id: None,
    }];
    let options = ChatOptions {
        temperature: 0.0,
        max_tokens: None,
        max_output_tokens: None,
        previous_response_id: None,
        instructions: None,
        metadata: None,
        include: None,
        store: None,
        parallel_tool_calls: None,
        tool_choice: None,
        text: None,
        reasoning: None,
        truncation: None,
        user: None,
        service_tier: None,
    };
    match provider
        .chat(request, Vec::new(), Some(model), options)
        .await
    {
        Ok(response) => Some(response.content),
        Err(err) => {
            warn!(error = %err, "structured compression call failed");
            None
        }
    }
}

async fn maybe_refresh_summary(
    runtime: &RuntimeBundle,
    session_key: &str,
    model_provider: &str,
    model: &str,
    full_history: &[ChatRecord],
) -> Option<ConversationSummary> {
    if runtime.conversation_history_limit == 0 || full_history.is_empty() {
        return None;
    }

    let sessions = session_manager(runtime);
    let persisted = sessions
        .get_session_compression_state(session_key)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let last_compressed_len = persisted.last_compressed_len.max(0) as usize;
    let latest_summary = persisted
        .summary_json
        .as_deref()
        .and_then(parse_conversation_summary);

    if !should_trigger_compression(
        last_compressed_len,
        full_history.len(),
        runtime.conversation_history_limit,
    ) {
        return latest_summary;
    }

    let start = last_compressed_len.min(full_history.len());
    let new_messages = to_conversation_messages(&full_history[start..]);
    if new_messages.is_empty() {
        return latest_summary;
    }

    let provider = runtime
        .runtime
        .provider_registry
        .get(model_provider)
        .cloned()
        .unwrap_or_else(|| Arc::clone(&runtime.runtime.provider));
    let model_output =
        run_structured_compression(provider, model, latest_summary.as_ref(), &new_messages).await;
    let next_summary = model_output
        .as_deref()
        .map(|output| merge_or_reset_summary(latest_summary.as_ref(), output))
        .or(latest_summary);

    let persisted_next = SessionCompressionState {
        last_compressed_len: full_history.len() as i64,
        summary_json: next_summary
            .as_ref()
            .and_then(|summary| serde_json::to_string(summary).ok()),
    };
    if let Err(err) = sessions
        .set_session_compression_state(session_key, &persisted_next)
        .await
    {
        warn!(error = %err, "failed to persist session compression state");
    }
    next_summary
}

pub async fn submit_and_get_output(
    runtime: &RuntimeBundle,
    channel: String,
    input: String,
    session_key: String,
    chat_id: String,
    sender_id: String,
    model_provider: String,
    model: String,
    media_references: Vec<MediaReference>,
    request_metadata: BTreeMap<String, serde_json::Value>,
) -> Result<Option<AssistantOutput>, Box<dyn std::error::Error>> {
    let sessions = session_manager(runtime);
    let full_history = sessions.read_chat_records(&session_key).await?;
    let summary = maybe_refresh_summary(
        runtime,
        &session_key,
        &model_provider,
        &model,
        &full_history,
    )
    .await;
    let conversation_history = build_history_for_model(
        full_history,
        runtime.conversation_history_limit,
        summary.as_ref(),
    );
    let header = EnvelopeHeader::new(session_key.clone());
    let user_record = ChatRecord::new("user", input.clone(), Some(header.message_id.to_string()));
    sessions
        .append_chat_record(&session_key, &user_record)
        .await?;
    let session_state = sessions
        .touch_session(&session_key, &chat_id, &channel)
        .await?;

    let inbound_payload = InboundMessage {
        channel: channel.to_string(),
        sender_id,
        chat_id: chat_id.clone(),
        session_key,
        content: input,
        media_references,
        metadata: {
            let mut metadata = request_metadata;
            metadata.insert(
                META_CONVERSATION_HISTORY_KEY.to_string(),
                json!(conversation_history
                    .into_iter()
                    .map(|record| {
                        json!({
                            "role": record.role,
                            "content": record.content,
                        })
                    })
                    .collect::<Vec<_>>()),
            );
            metadata.insert(
                META_PROVIDER_KEY.to_string(),
                serde_json::Value::String(model_provider),
            );
            metadata.insert(META_MODEL_KEY.to_string(), serde_json::Value::String(model));
            metadata
        },
    };
    info!(inbound = ?inbound_payload, "channel inbound normalized");

    runtime
        .inbound_transport
        .enqueue(Envelope {
            header,
            metadata: BTreeMap::new(),
            payload: inbound_payload,
        })
        .await;

    let outcome = run_runtime_once(runtime).await?;
    enqueue_llm_audit_records_from_outcome(runtime, session_state.turn_count, &outcome);
    match outcome.final_response {
        Some(msg) if should_emit_outbound(&msg) => {
            let agent_record = ChatRecord::new("assistant", msg.payload.content.clone(), None);
            persist_assistant_response_state(
                runtime,
                &msg.header.session_key,
                &msg.payload.chat_id,
                &msg.payload.channel,
                session_state.turn_count,
                &msg.header.message_id,
                &msg.payload.metadata,
                &agent_record,
            )
            .await;
            let reasoning = msg
                .payload
                .metadata
                .get("reasoning")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .filter(|value| !value.trim().is_empty());
            Ok(Some(AssistantOutput {
                content: msg.payload.content.clone(),
                reasoning,
                metadata: msg.payload.metadata.clone(),
            }))
        }
        Some(_) | None => {
            warn!("no outbound response produced");
            Ok(None)
        }
    }
}

pub async fn submit_webhook_event(
    runtime: &RuntimeBundle,
    request: &GatewayWebhookRequest,
) -> Result<Option<AssistantOutput>, String> {
    let route = resolve_session_route(runtime, "webhook", &request.session_key, &request.chat_id)
        .await
        .map_err(|err| err.to_string())?;
    submit_and_get_output(
        runtime,
        "webhook".to_string(),
        request.content.clone(),
        route.active_session_key,
        request.chat_id.clone(),
        request.sender_id.clone(),
        route.model_provider,
        route.model,
        Vec::new(),
        request.metadata.clone(),
    )
    .await
    .map_err(|err| err.to_string())
}

#[allow(clippy::too_many_arguments)]
pub async fn submit_and_stream_output(
    runtime: &RuntimeBundle,
    channel: String,
    input: String,
    session_key: String,
    chat_id: String,
    model_provider: String,
    model: String,
    media_references: Vec<MediaReference>,
    request_metadata: BTreeMap<String, serde_json::Value>,
    writer: &mut dyn ChannelStreamWriter,
) -> Result<Option<AssistantOutput>, Box<dyn std::error::Error>> {
    let sessions = session_manager(runtime);
    let full_history = sessions.read_chat_records(&session_key).await?;
    let summary = maybe_refresh_summary(
        runtime,
        &session_key,
        &model_provider,
        &model,
        &full_history,
    )
    .await;
    let conversation_history = build_history_for_model(
        full_history,
        runtime.conversation_history_limit,
        summary.as_ref(),
    );
    let header = EnvelopeHeader::new(session_key.clone());
    let user_record = ChatRecord::new("user", input.clone(), Some(header.message_id.to_string()));
    sessions
        .append_chat_record(&session_key, &user_record)
        .await?;
    let session_state = sessions
        .touch_session(&session_key, &chat_id, &channel)
        .await?;

    let inbound_payload = InboundMessage {
        channel: channel.to_string(),
        sender_id: "local-user".to_string(),
        chat_id: chat_id.clone(),
        session_key,
        content: input,
        media_references,
        metadata: {
            let mut metadata = request_metadata;
            metadata.insert(
                META_CONVERSATION_HISTORY_KEY.to_string(),
                json!(conversation_history
                    .into_iter()
                    .map(|record| {
                        json!({
                            "role": record.role,
                            "content": record.content,
                        })
                    })
                    .collect::<Vec<_>>()),
            );
            metadata.insert(
                META_PROVIDER_KEY.to_string(),
                serde_json::Value::String(model_provider),
            );
            metadata.insert(META_MODEL_KEY.to_string(), serde_json::Value::String(model));
            metadata
        },
    };
    info!(inbound = ?inbound_payload, "channel inbound normalized (streaming)");

    let envelope = Envelope {
        header,
        metadata: BTreeMap::new(),
        payload: inbound_payload,
    };
    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<AgentExecutionStreamEvent>();
    let process = runtime
        .runtime
        .process_message_streaming(envelope, stream_tx);
    tokio::pin!(process);
    let mut stream_open = true;

    let outcome = loop {
        tokio::select! {
            maybe_event = stream_rx.recv(), if stream_open => {
                let Some(event) = maybe_event else {
                    stream_open = false;
                    continue;
                };
                match event {
                    AgentExecutionStreamEvent::Snapshot { content, reasoning } => {
                        writer.write(ChannelStreamEvent::Snapshot(ChannelResponse {
                            content,
                            reasoning,
                            metadata: BTreeMap::new(),
                        })).await?;
                    }
                    AgentExecutionStreamEvent::Clear => {
                        writer.write(ChannelStreamEvent::Clear).await?;
                    }
                }
            }
            outcome = &mut process => break outcome,
        }
    };
    enqueue_llm_audit_records_from_outcome(runtime, session_state.turn_count, &outcome);

    match outcome.final_response {
        Some(msg) if should_emit_outbound(&msg) => {
            let agent_record = ChatRecord::new("assistant", msg.payload.content.clone(), None);
            persist_assistant_response_state(
                runtime,
                &msg.header.session_key,
                &msg.payload.chat_id,
                &msg.payload.channel,
                session_state.turn_count,
                &msg.header.message_id,
                &msg.payload.metadata,
                &agent_record,
            )
            .await;
            let reasoning = msg
                .payload
                .metadata
                .get("reasoning")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .filter(|value| !value.trim().is_empty());
            let output = AssistantOutput {
                content: msg.payload.content.clone(),
                reasoning,
                metadata: msg.payload.metadata.clone(),
            };
            writer
                .write(ChannelStreamEvent::Snapshot(ChannelResponse {
                    content: output.content.clone(),
                    reasoning: output.reasoning.clone(),
                    metadata: output.metadata.clone(),
                }))
                .await?;
            Ok(Some(output))
        }
        Some(_) | None => {
            enqueue_llm_audit_records_from_outcome(runtime, session_state.turn_count, &outcome);
            warn!("no outbound response produced");
            Ok(None)
        }
    }
}

pub async fn drain_runtime_queue(
    runtime: &RuntimeBundle,
    max_iterations: usize,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut drained = 0usize;
    for _ in 0..max_iterations.max(1) {
        let outcome = run_runtime_once(runtime).await?;
        let Some(ref msg) = outcome.final_response else {
            let turn_index = if let Some(session_key) = outcome.audit_session_key.as_deref() {
                session_manager(runtime)
                    .get_session(session_key)
                    .await
                    .map(|session| session.turn_count)
                    .unwrap_or(0)
            } else {
                0
            };
            enqueue_llm_audit_records_from_outcome(runtime, turn_index, &outcome);
            break;
        };
        let agent_record = ChatRecord::new("assistant", msg.payload.content.clone(), None);
        let sessions = session_manager(runtime);
        let turn_index = sessions
            .get_session(&msg.header.session_key)
            .await
            .map(|session| session.turn_count)
            .unwrap_or(0);
        enqueue_llm_audit_records_from_outcome(runtime, turn_index, &outcome);
        persist_assistant_response_state(
            runtime,
            &msg.header.session_key,
            &msg.payload.chat_id,
            &msg.payload.channel,
            turn_index,
            &msg.header.message_id,
            &msg.payload.metadata,
            &agent_record,
        )
        .await;
        drained += 1;
    }
    Ok(drained)
}

async fn run_runtime_once(
    runtime: &RuntimeBundle,
) -> Result<klaw_core::ProcessOutcome, Box<dyn std::error::Error>> {
    let result = runtime
        .runtime
        .run_once_reliable(
            &runtime.inbound_transport,
            &runtime.outbound_transport,
            &runtime.deadletter_transport,
            &runtime.subscription,
            &runtime.idempotency,
            &runtime.retry_policy,
            &runtime.deadletter_policy,
            &runtime.circuit_breaker,
        )
        .await;

    match result {
        Ok(outcome) => Ok(outcome),
        Err(err) if is_queue_empty_error(&err) => Ok(klaw_core::ProcessOutcome {
            final_response: None,
            error_code: None,
            final_state: klaw_core::AgentRunState::Completed,
            llm_audits: Vec::new(),
            audit_message_id: None,
            audit_session_key: None,
            audit_chat_id: None,
        }),
        Err(err) => Err(Box::new(err)),
    }
}

fn is_queue_empty_error(err: &AgentRuntimeError) -> bool {
    match err {
        AgentRuntimeError::Transport(TransportError::ConsumeFailed(message)) => {
            message.contains("queue empty")
        }
        _ => false,
    }
}

fn format_approve_already_handled_message(approval_id: &str, status: ApprovalStatus) -> String {
    match status {
        ApprovalStatus::Consumed => {
            format!("ℹ️ Approval `{approval_id}` was already approved and executed.")
        }
        other => format!(
            "ℹ️ Approval `{approval_id}` is already `{}`.",
            other.as_str()
        ),
    }
}

fn config_err(message: String) -> Box<dyn Error> {
    Box::new(io::Error::other(message))
}

fn configured_default_model(config: &AppConfig, provider_id: &str) -> String {
    config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            config
                .model_providers
                .get(provider_id)
                .map(|provider| provider.default_model.clone())
        })
        .unwrap_or_else(|| "default".to_string())
}

fn build_unavailable_provider(
    config: &AppConfig,
    provider_id: &str,
    default_model: String,
    reason: String,
) -> UnavailableProvider {
    let provider = config.model_providers.get(provider_id);
    let provider_name = provider
        .and_then(|provider| provider.name.clone())
        .unwrap_or_else(|| provider_id.to_string());
    let setup_hint = provider
        .and_then(|provider| provider.env_key.as_ref())
        .map(|env_key| format!(" Set `{env_key}` or configure `api_key`."))
        .unwrap_or_default();

    UnavailableProvider {
        name: provider_name,
        default_model,
        wire_api: provider.map(|provider| provider.wire_api.clone()),
        reason: format!("provider `{provider_id}` is unavailable: {reason}.{setup_hint}"),
    }
}

async fn init_observability_from_config(config: &AppConfig) -> Option<ObservabilityHandle> {
    if !config.observability.enabled {
        return None;
    }
    let obs = &config.observability;
    let obs_config = ObservabilityConfig {
        enabled: obs.enabled,
        service_name: obs.service_name.clone(),
        service_version: obs.service_version.clone(),
        metrics: klaw_observability::config::MetricsConfig {
            enabled: obs.metrics.enabled,
            export_interval_seconds: obs.metrics.export_interval_seconds,
        },
        traces: klaw_observability::config::TracesConfig {
            enabled: obs.traces.enabled,
            sample_rate: obs.traces.sample_rate,
        },
        otlp: klaw_observability::config::OtlpConfig {
            enabled: obs.otlp.enabled,
            endpoint: obs.otlp.endpoint.clone(),
            headers: obs.otlp.headers.clone(),
        },
        prometheus: klaw_observability::config::PrometheusConfig {
            enabled: obs.prometheus.enabled,
            listen_port: obs.prometheus.listen_port,
            path: obs.prometheus.path.clone(),
        },
        audit: klaw_observability::config::AuditConfig {
            enabled: obs.audit.enabled,
            output_path: obs.audit.output_path.clone(),
        },
        local_store: klaw_observability::config::LocalStoreConfig {
            enabled: obs.local_store.enabled,
            retention_days: obs.local_store.retention_days,
            flush_interval_seconds: obs.local_store.flush_interval_seconds,
        },
    };
    let data_root = config.storage.root_dir.as_ref().map(PathBuf::from);
    match init_observability(&obs_config, data_root).await {
        Ok(handle) => {
            info!(
                service_name = %obs.service_name,
                otlp_enabled = obs.otlp.enabled,
                prometheus_enabled = obs.prometheus.enabled,
                local_store_enabled = obs.local_store.enabled,
                "observability initialized"
            );
            Some(handle)
        }
        Err(err) => {
            warn!(error = %err, "failed to initialize observability; continuing without telemetry");
            None
        }
    }
}

pub async fn finalize_startup_report(
    runtime: &mut RuntimeBundle,
) -> Result<StartupReport, Box<dyn Error>> {
    let mcp_summary = match &runtime.mcp_bootstrap {
        Some(handle) => {
            let mut guard = handle.lock().await;
            if guard.is_ready() {
                Some(
                    guard
                        .wait_until_ready()
                        .await
                        .map_err(|err| config_err(format!("mcp bootstrap failed: {err}")))?,
                )
            } else {
                None
            }
        }
        None => None,
    };

    runtime.startup_report.tool_names = runtime.runtime.tools.list();
    runtime.startup_report.mcp_summary = mcp_summary;
    Ok(runtime.startup_report.clone())
}

async fn reconcile_heartbeats<S>(config: &AppConfig, storage: &S) -> Result<(), Box<dyn Error>>
where
    S: CronStorage + Send + Sync + Clone + 'static,
{
    let specs = specs_from_config(config)?;
    if specs.is_empty() {
        return Ok(());
    }

    let scheduler = CronHeartbeatScheduler::new(Arc::new(storage.clone()));
    scheduler.reconcile(&specs).await?;
    Ok(())
}

fn should_emit_outbound(msg: &Envelope<OutboundMessage>) -> bool {
    !should_suppress_output(&msg.payload.content, &msg.payload.metadata)
}

#[cfg(test)]
mod tests {
    use super::{
        build_history_for_model, build_unavailable_provider, compression_trigger_interval,
        configured_default_model, first_arg_token, format_approve_already_handled_message,
        format_workspace_docs_for_prompt, normalize_runtime_provider_override, parse_im_command,
        resolve_new_session_target_from_config, should_emit_outbound, should_trigger_compression,
        trim_conversation_history,
    };
    use klaw_agent::ConversationSummary;
    use klaw_config::{AppConfig, ModelProviderConfig};
    use klaw_core::{Envelope, EnvelopeHeader, OutboundMessage};
    use klaw_llm::{ChatOptions, LlmError, LlmProvider};
    use klaw_session::ChatRecord;
    use klaw_storage::ApprovalStatus;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn silent_heartbeat_ack_is_filtered() {
        let msg = Envelope {
            header: EnvelopeHeader::new("stdio:test"),
            metadata: BTreeMap::new(),
            payload: OutboundMessage {
                channel: "stdio".to_string(),
                chat_id: "test".to_string(),
                content: " HEARTBEAT_OK ".to_string(),
                reply_to: None,
                metadata: BTreeMap::from([
                    ("trigger.kind".to_string(), json!("heartbeat")),
                    (
                        "heartbeat.silent_ack_token".to_string(),
                        json!("HEARTBEAT_OK"),
                    ),
                ]),
            },
        };

        assert!(!should_emit_outbound(&msg));
    }

    #[test]
    fn normal_messages_are_not_filtered() {
        let msg = Envelope {
            header: EnvelopeHeader::new("stdio:test"),
            metadata: BTreeMap::new(),
            payload: OutboundMessage {
                channel: "stdio".to_string(),
                chat_id: "test".to_string(),
                content: "Need action".to_string(),
                reply_to: None,
                metadata: BTreeMap::from([("trigger.kind".to_string(), json!("heartbeat"))]),
            },
        };

        assert!(should_emit_outbound(&msg));
    }

    #[test]
    fn approve_already_handled_message_hides_consumed_status_wording() {
        let content =
            format_approve_already_handled_message("approval-1", ApprovalStatus::Consumed);
        assert!(content.contains("already approved and executed"));
        assert!(!content.contains("consumed"));
    }

    #[test]
    fn parse_im_command_supports_name_and_optional_arg() {
        assert_eq!(parse_im_command("/help"), Some(("help", None)));
        assert_eq!(
            parse_im_command("/model_provider openai"),
            Some(("model_provider", Some("openai")))
        );
        assert_eq!(
            parse_im_command("/model qwen-plus /help"),
            Some(("model", Some("qwen-plus /help")))
        );
        assert_eq!(parse_im_command("hello"), None);
    }

    #[test]
    fn first_arg_token_uses_first_token_only() {
        assert_eq!(first_arg_token(Some("openai /help")), Some("openai"));
        assert_eq!(
            first_arg_token(Some("qwen-plus extra words")),
            Some("qwen-plus")
        );
        assert_eq!(first_arg_token(Some("   ")), None);
    }

    #[test]
    fn resolve_new_session_target_uses_global_active_provider_and_model() {
        let mut config = AppConfig::default();
        config.model_provider = "anthropic".to_string();
        config.model = None;
        config.model_providers.insert(
            "anthropic".to_string(),
            ModelProviderConfig {
                default_model: "claude-sonnet-4-5".to_string(),
                ..ModelProviderConfig::default()
            },
        );

        let (provider_id, model) = resolve_new_session_target_from_config(&config);
        assert_eq!(provider_id, "anthropic");
        assert_eq!(model, "claude-sonnet-4-5");
    }

    #[test]
    fn resolve_new_session_target_prefers_root_model_override() {
        let mut config = AppConfig::default();
        config.model_provider = "anthropic".to_string();
        config.model = Some("claude-opus-4-1".to_string());
        config.model_providers.insert(
            "anthropic".to_string(),
            ModelProviderConfig {
                default_model: "claude-sonnet-4-5".to_string(),
                ..ModelProviderConfig::default()
            },
        );

        let (provider_id, model) = resolve_new_session_target_from_config(&config);
        assert_eq!(provider_id, "anthropic");
        assert_eq!(model, "claude-opus-4-1");
    }

    #[test]
    fn normalize_runtime_provider_override_accepts_known_override_and_reset() {
        let providers = BTreeMap::from([
            ("openai".to_string(), "gpt-4.1-mini".to_string()),
            ("anthropic".to_string(), "claude-sonnet-4-5".to_string()),
        ]);

        let (override_value, active_provider) =
            normalize_runtime_provider_override(&providers, "openai", Some("anthropic"))
                .expect("override should set");
        assert_eq!(override_value.as_deref(), Some("anthropic"));
        assert_eq!(active_provider, "anthropic");

        let (override_value, active_provider) =
            normalize_runtime_provider_override(&providers, "openai", None)
                .expect("override should reset");
        assert_eq!(override_value, None);
        assert_eq!(active_provider, "openai");
    }

    #[test]
    fn normalize_runtime_provider_override_rejects_unknown_provider() {
        let providers = BTreeMap::from([("openai".to_string(), "gpt-4.1-mini".to_string())]);

        let err = normalize_runtime_provider_override(&providers, "openai", Some("missing"))
            .expect_err("unknown provider should fail");
        assert!(err.to_string().contains("unknown runtime provider"));
    }

    #[test]
    fn workspace_docs_prompt_contains_routing_and_memory_tool_rule() {
        let docs_prompt = format_workspace_docs_for_prompt();

        assert!(docs_prompt.contains("Read these workspace docs on demand when relevant:"));
        assert!(docs_prompt.contains("Recommended usage:"));
        assert!(docs_prompt.contains("AGENTS.md"));
        assert!(docs_prompt.contains("USER.md"));
        assert!(docs_prompt.contains("TOOLS.md"));
        assert!(docs_prompt.contains("Use the memory tool for durable memory"));
    }

    #[test]
    fn trim_conversation_history_keeps_recent_items_when_over_limit() {
        let history = vec![
            ChatRecord::new("user", "m1", None),
            ChatRecord::new("assistant", "m2", None),
            ChatRecord::new("user", "m3", None),
        ];

        let trimmed = trim_conversation_history(history, 2);
        let contents: Vec<&str> = trimmed
            .iter()
            .map(|record| record.content.as_str())
            .collect();
        assert_eq!(contents, vec!["m2", "m3"]);
    }

    #[test]
    fn trim_conversation_history_returns_full_history_when_limit_is_zero() {
        let history = vec![
            ChatRecord::new("user", "m1", None),
            ChatRecord::new("assistant", "m2", None),
        ];
        let trimmed = trim_conversation_history(history, 0);
        assert_eq!(trimmed.len(), 2);
    }

    #[test]
    fn trim_conversation_history_returns_full_history_when_limit_covers_all() {
        let history = vec![
            ChatRecord::new("user", "m1", None),
            ChatRecord::new("assistant", "m2", None),
        ];
        let trimmed = trim_conversation_history(history, 5);
        assert_eq!(trimmed.len(), 2);
    }

    #[test]
    fn compression_trigger_interval_uses_half_limit_with_minimum_one() {
        assert_eq!(compression_trigger_interval(20), 10);
        assert_eq!(compression_trigger_interval(1), 1);
    }

    #[test]
    fn should_trigger_compression_fires_after_half_window_growth() {
        assert!(should_trigger_compression(0, 10, 20));
        assert!(!should_trigger_compression(0, 9, 20));
        assert!(!should_trigger_compression(0, 100, 0));
    }

    #[test]
    fn build_history_for_model_injects_summary_and_keeps_recent_messages() {
        let full_history = vec![
            ChatRecord::new("user", "a", None),
            ChatRecord::new("assistant", "b", None),
            ChatRecord::new("user", "c", None),
        ];
        let summary = ConversationSummary {
            goal: "g".to_string(),
            progress: vec![],
            pending: vec![],
            decisions: vec![],
            notes: vec![],
        };

        let model_history = build_history_for_model(full_history, 2, Some(&summary));
        assert_eq!(model_history.len(), 2);
        assert_eq!(model_history[0].role, "system");
        assert!(model_history[0]
            .content
            .contains("Conversation Summary (JSON):"));
        assert_eq!(model_history[1].content, "c");
    }

    #[test]
    fn configured_default_model_prefers_global_override() {
        let mut config = AppConfig::default();
        config.model = Some("gpt-4.1".to_string());

        assert_eq!(configured_default_model(&config, "openai"), "gpt-4.1");
    }

    #[tokio::test]
    async fn unavailable_provider_reports_setup_hint() {
        let config = AppConfig::default();
        let provider = build_unavailable_provider(
            &config,
            "openai",
            configured_default_model(&config, "openai"),
            "provider `openai` requires api_key or env_key".to_string(),
        );

        let err = provider
            .chat(
                Vec::new(),
                Vec::new(),
                None,
                ChatOptions {
                    temperature: 0.0,
                    max_tokens: None,
                    max_output_tokens: None,
                    previous_response_id: None,
                    instructions: None,
                    metadata: None,
                    include: None,
                    store: None,
                    parallel_tool_calls: None,
                    tool_choice: None,
                    text: None,
                    reasoning: None,
                    truncation: None,
                    user: None,
                    service_tier: None,
                },
            )
            .await
            .expect_err("unavailable provider should fail");

        match err {
            LlmError::ProviderUnavailable { message, .. } => {
                assert!(message.contains("provider `openai` is unavailable"));
                assert!(message.contains("OPENAI_API_KEY"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
