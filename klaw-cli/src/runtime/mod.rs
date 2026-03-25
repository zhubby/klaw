pub mod gateway_manager;
pub mod service_loop;
pub mod webhook;

use crate::env_check;
use klaw_agent::{
    AgentExecutionStreamEvent, ConversationMessage, ConversationSummary, build_compression_prompt,
    build_provider_from_config, merge_or_reset_summary, parse_conversation_summary,
};
use klaw_approval::{
    ApprovalManager, ApprovalResolveDecision, ApprovalStatus, SqliteApprovalManager,
};
use klaw_channel::{
    ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, ChannelStreamEvent,
    ChannelStreamWriter, DefaultChannelDriverFactory,
};
use klaw_config::{AppConfig, ConfigStore, ToolEnabled};
use klaw_core::{
    AgentLoop, AgentRuntimeError, AgentTelemetry, CircuitBreakerPolicy, DeadLetterMessage,
    DeadLetterPolicy, Envelope, EnvelopeHeader, ExponentialBackoffRetryPolicy,
    InMemoryCircuitBreaker, InMemoryIdempotencyStore, InMemoryTransport, InboundMessage,
    MediaReference, OutboundMessage, QueueStrategy, RunLimits, SessionSchedulingPolicy,
    SkillPromptEntry, Subscription, TransportError, build_runtime_system_prompt,
    ensure_workspace_prompt_templates,
};
use klaw_gateway::{GatewayWebhookAgentRequest, GatewayWebhookRequest};
use klaw_heartbeat::should_suppress_output;
use klaw_llm::{ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolDefinition};
use klaw_mcp::{McpConfigSnapshot, McpInitHandle, McpManager, McpSyncResult};
use klaw_observability::{
    ObservabilityConfig, ObservabilityHandle, OtelAgentTelemetry, init_observability,
};
use klaw_session::{
    ChatRecord, LlmAuditStatus, LlmUsageSource, NewLlmAuditRecord, NewLlmUsageRecord,
    SessionCompressionState, SessionIndex, SessionManager, SqliteSessionManager,
};
use klaw_skill::{
    InstalledSkill, RegistrySource, SkillSourceKind, SkillsManager, open_default_skills_manager,
};
use klaw_storage::{DefaultSessionStore, open_default_store};
use klaw_tool::{
    ApplyPatchTool, ApprovalTool, ArchiveTool, CronManagerTool, HeartbeatManagerTool,
    LocalSearchTool, MemoryTool, ShellTool, SkillsManagerTool, SkillsRegistryTool, SubAgentTool,
    TerminalMultiplexerTool, ToolContext, ToolRegistry, VoiceTool, WebFetchTool, WebSearchTool,
};
use klaw_util::EnvironmentCheckReport;
use serde_json::{Value, json};
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
const NEW_SESSION_BOOTSTRAP_USER_MESSAGE: &str = "You just woke up. Time to figure out who you are.\nThis is a brand new conversation. If `BOOTSTRAP.md` exists, use the available workspace tools to read it and follow it before anything else. If it does not exist, start with a short, natural greeting.\nGuide the user through initializing the agent's identity, vibe, and context. When you learn durable bootstrap details, use tools to update `IDENTITY.md` and `USER.md`, and delete `BOOTSTRAP.md` once bootstrap is truly complete. Do not claim files were updated unless you actually changed them with tools. Do not mention that this message was auto-generated.";

#[derive(Debug, Clone, Default)]
pub struct StartupReport {
    pub skill_names: Vec<String>,
    pub tool_names: Vec<String>,
    pub mcp_summary: Option<McpSyncResult>,
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
    pub mcp_init: Option<Mutex<McpInitHandle>>,
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

pub fn build_channel_driver_factory(
    _config: &AppConfig,
) -> Result<DefaultChannelDriverFactory, Box<dyn Error>> {
    Ok(DefaultChannelDriverFactory::default())
}

const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";
const META_PROVIDER_KEY: &str = "agent.provider_id";
const META_MODEL_KEY: &str = "agent.model";
const META_TOOL_CHOICE_KEY: &str = "agent.tool_choice";

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
        Err(err) if err.code() == "approval_required" => Ok(err.message().to_string()),
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
    let (default_provider_id, default_model) = runtime_default_route(runtime);
    let base = sessions
        .get_or_create_session_state(
            base_session_key,
            chat_id,
            channel,
            &default_provider_id,
            &default_model,
        )
        .await?;
    let base = normalize_session_route_state(
        runtime,
        base,
        chat_id,
        channel,
        &default_provider_id,
        &default_model,
    )
    .await?;
    let active_session_key = base
        .active_session_key
        .clone()
        .unwrap_or_else(|| base_session_key.to_string());
    let active = sessions
        .get_or_create_session_state(
            &active_session_key,
            chat_id,
            channel,
            &default_provider_id,
            &default_model,
        )
        .await?;
    let active = normalize_session_route_state(
        runtime,
        active,
        chat_id,
        channel,
        &default_provider_id,
        &default_model,
    )
    .await?;
    let model_provider = active
        .model_provider
        .unwrap_or_else(|| default_provider_id.clone());
    let model = active.model.unwrap_or_else(|| {
        if model_provider == default_provider_id {
            default_model.clone()
        } else {
            default_model_for_provider(runtime, &model_provider).to_string()
        }
    });
    Ok(SessionRoute {
        active_session_key,
        model_provider,
        model,
    })
}

fn resolve_new_session_target(runtime: &RuntimeBundle) -> (String, String) {
    runtime_default_route(runtime)
}

fn build_new_session_bootstrap_user_message() -> String {
    NEW_SESSION_BOOTSTRAP_USER_MESSAGE.to_string()
}

fn build_new_session_bootstrap_request_metadata() -> BTreeMap<String, Value> {
    BTreeMap::from([(
        META_TOOL_CHOICE_KEY.to_string(),
        Value::String("required".to_string()),
    )])
}

fn format_new_session_started_message(
    session_key: &str,
    provider: &str,
    model: &str,
    assistant_reply: Option<&str>,
) -> String {
    let summary = format!(
        "🆕 **New session started**\n\n🧵 Session: `{session_key}`\n🧩 Provider: `{provider}`\n🤖 Model: `{model}`"
    );
    match assistant_reply
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(reply) => format!("{summary}\n\n{reply}"),
        None => summary,
    }
}

fn runtime_default_route(runtime: &RuntimeBundle) -> (String, String) {
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

async fn normalize_session_route_state(
    runtime: &RuntimeBundle,
    session: SessionIndex,
    chat_id: &str,
    channel: &str,
    global_provider: &str,
    global_model: &str,
) -> Result<SessionIndex, Box<dyn Error>> {
    let sessions = session_manager(runtime);
    let self_routed = session
        .active_session_key
        .as_deref()
        .map(|value| value == session.session_key)
        .unwrap_or(true);

    if !self_routed && (session.model_provider.is_some() || session.model.is_some()) {
        return Ok(sessions
            .clear_model_routing_override(&session.session_key, chat_id, channel)
            .await?);
    }

    if session.model_provider.is_none() {
        if let Some(model) = session.model.as_deref() {
            if model == global_model {
                return Ok(sessions
                    .clear_model_routing_override(&session.session_key, chat_id, channel)
                    .await?);
            }
            return Ok(sessions
                .set_model_provider(
                    &session.session_key,
                    chat_id,
                    channel,
                    global_provider,
                    model,
                )
                .await?);
        }
        return Ok(session);
    }

    if session.model_provider.as_deref() == Some(global_provider) {
        let resolved_model = session.model.as_deref().unwrap_or(global_model);
        if resolved_model == global_model {
            return Ok(sessions
                .clear_model_routing_override(&session.session_key, chat_id, channel)
                .await?);
        }
    }

    Ok(session)
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
    lines.push(format!(
        "{:<24}{}",
        "/stop", "Stop the current turn without calling the agent"
    ));
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

fn stopped_turn_metadata(reason: &str, source: &str) -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([
        ("turn.stopped".to_string(), serde_json::Value::Bool(true)),
        (
            "turn.stop_signal".to_string(),
            serde_json::json!({
                "reason": reason,
                "source": source,
            }),
        ),
        (
            "tool.signals".to_string(),
            serde_json::json!([
                {
                    "kind": "stop",
                    "payload": {
                        "reason": reason,
                        "source": source,
                    }
                }
            ]),
        ),
    ])
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
        "stop" => ChannelResponse {
            content: "Current turn stopped manually. No further tool calls were made.".to_string(),
            reasoning: None,
            metadata: stopped_turn_metadata("manual stop command", "im_command"),
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
            let bootstrap_input = build_new_session_bootstrap_user_message();
            match submit_and_get_output(
                runtime,
                channel.clone(),
                bootstrap_input,
                new_session_key.clone(),
                chat_id.clone(),
                "local-user".to_string(),
                new_session_provider.clone(),
                new_session_model.clone(),
                Vec::new(),
                build_new_session_bootstrap_request_metadata(),
            )
            .await
            {
                Ok(Some(output)) => ChannelResponse {
                    content: format_new_session_started_message(
                        &new_session_key,
                        &new_session_provider,
                        &new_session_model,
                        Some(&output.content),
                    ),
                    reasoning: output.reasoning,
                    metadata: output.metadata,
                },
                Ok(None) => ChannelResponse {
                    content: format_new_session_started_message(
                        &new_session_key,
                        &new_session_provider,
                        &new_session_model,
                        None,
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                },
                Err(err) => ChannelResponse {
                    content: format!(
                        "{}\n\n⚠️ Session bootstrap reply failed: {}",
                        format_new_session_started_message(
                            &new_session_key,
                            &new_session_provider,
                            &new_session_model,
                            None,
                        ),
                        err
                    ),
                    reasoning: None,
                    metadata: BTreeMap::new(),
                },
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
                let (global_provider, global_model) = runtime_default_route(runtime);
                if provider_id == global_provider && default_model == &global_model {
                    sessions
                        .clear_model_routing_override(&route.active_session_key, &chat_id, &channel)
                        .await?;
                } else {
                    sessions
                        .set_model_provider(
                            &route.active_session_key,
                            &chat_id,
                            &channel,
                            provider_id,
                            default_model,
                        )
                        .await?;
                }
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
                let (global_provider, global_model) = runtime_default_route(runtime);
                if route.model_provider == global_provider && model == global_model {
                    sessions
                        .clear_model_routing_override(&route.active_session_key, &chat_id, &channel)
                        .await?;
                } else {
                    sessions
                        .set_model_provider(
                            &route.active_session_key,
                            &chat_id,
                            &channel,
                            &route.model_provider,
                            model,
                        )
                        .await?;
                }
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
    let mut tools = ToolRegistry::default();
    if config.tools.archive.enabled() {
        tools.register(ArchiveTool::open_default(config).await?);
    }
    if config.tools.voice.enabled() {
        if config.voice.enabled {
            tools.register(VoiceTool::open_default(config).await?);
        } else {
            warn!(
                "voice tool enabled but voice service is disabled in config; skipping registration"
            );
        }
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
    if config.tools.heartbeat_manager.enabled() {
        tools.register(HeartbeatManagerTool::with_store(session_store.clone()));
    }
    if config.tools.skills_registry.enabled() {
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
    } else {
        info!("skills registry tool disabled by config");
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

    let mcp_init = if config.mcp.enabled && configured_mcp_servers > 0 {
        let snapshot = McpConfigSnapshot::from_mcp_config(&config.mcp);
        Some(Mutex::new(McpManager::spawn_init(tools.clone(), snapshot)))
    } else {
        None
    };

    ensure_workspace_prompt_templates_if_possible().await;
    let loaded_skills = load_skills_system_prompt(config).await;
    let skill_names = loaded_skills.skill_names.clone();
    let system_prompt = build_runtime_system_prompt(loaded_skills.skill_entries);

    let observability = init_observability_from_config(config).await;
    let llm_audit_tx = spawn_llm_audit_writer(session_store.clone());
    let telemetry: Option<Arc<dyn AgentTelemetry>> = observability.as_ref().map(|handle| {
        Arc::new(OtelAgentTelemetry::from_handle(handle, "klaw")) as Arc<dyn AgentTelemetry>
    });

    let mut runtime = AgentLoop::new_with_identity(
        RunLimits {
            max_tool_iterations: 32,
            max_tool_calls: 64,
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
        mcp_init,
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
    if let Some(handle) = &runtime.mcp_init {
        info!("shutting down runtime mcp servers");
        let guard = handle.lock().await;
        let manager = guard.manager();
        let mut manager_guard = manager.lock().await;
        let shutdown_deadline = Duration::from_secs(2);
        match timeout(shutdown_deadline, manager_guard.shutdown_all()).await {
            Ok(()) => {}
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
    let system_prompt = build_runtime_system_prompt(loaded_skills.skill_entries);
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
    const MAX_LEN: usize = 180;
    extract_skill_frontmatter_description(markdown)
        .or_else(|| extract_skill_body_description(markdown))
        .map(|description| truncate_skill_description(&description, MAX_LEN))
        .unwrap_or_else(|| "no description".to_string())
}

fn extract_skill_frontmatter_description(markdown: &str) -> Option<String> {
    frontmatter_lines(markdown)?
        .find_map(|line| line.trim().strip_prefix("description:").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(trim_matching_quotes)
        .map(str::to_string)
}

fn extract_skill_body_description(markdown: &str) -> Option<String> {
    let body = strip_frontmatter(markdown).unwrap_or(markdown);
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && *line != "---")
        .map(str::to_string)
}

fn strip_frontmatter(markdown: &str) -> Option<&str> {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }

    let mut offset = markdown.find('\n')? + 1;
    for line in lines {
        let line_end = offset + line.len();
        let next_offset = if markdown.as_bytes().get(line_end) == Some(&b'\n') {
            line_end + 1
        } else {
            line_end
        };
        if line.trim() == "---" {
            return Some(&markdown[next_offset..]);
        }
        offset = next_offset;
    }

    None
}

fn frontmatter_lines(markdown: &str) -> Option<impl Iterator<Item = &str>> {
    let frontmatter = markdown
        .strip_prefix("---\n")
        .or_else(|| markdown.strip_prefix("---\r\n"))?;
    let (frontmatter, _) = frontmatter
        .split_once("\n---\n")
        .or_else(|| frontmatter.split_once("\r\n---\r\n"))?;
    Some(frontmatter.lines())
}

fn trim_matching_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }

    value
}

fn truncate_skill_description(description: &str, max_len: usize) -> String {
    if description.chars().count() <= max_len {
        return description.to_string();
    }

    let mut trimmed = description.chars().take(max_len).collect::<String>();
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
                json!(
                    conversation_history
                        .into_iter()
                        .map(|record| {
                            json!({
                                "role": record.role,
                                "content": record.content,
                            })
                        })
                        .collect::<Vec<_>>()
                ),
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

pub async fn submit_webhook_agent(
    runtime: &RuntimeBundle,
    request: &GatewayWebhookAgentRequest,
    content: String,
) -> Result<Option<AssistantOutput>, String> {
    let route = resolve_session_route(runtime, "webhook", &request.session_key, &request.chat_id)
        .await
        .map_err(|err| err.to_string())?;
    let (model_provider, model) = resolve_webhook_agent_model(
        runtime,
        &route.model_provider,
        &route.model,
        request.provider.as_deref(),
        request.model.as_deref(),
    )?;
    let mut metadata = request.metadata.clone();
    metadata.insert(
        "webhook.agents.original_session_key".to_string(),
        Value::String(request.session_key.clone()),
    );
    metadata.insert(
        "webhook.agents.resolved_session_key".to_string(),
        Value::String(route.active_session_key.clone()),
    );
    metadata.insert(
        "webhook.agents.provider".to_string(),
        Value::String(model_provider.clone()),
    );
    metadata.insert(
        "webhook.agents.model".to_string(),
        Value::String(model.clone()),
    );
    submit_and_get_output(
        runtime,
        "webhook".to_string(),
        content,
        route.active_session_key,
        request.chat_id.clone(),
        request.sender_id.clone(),
        model_provider,
        model,
        Vec::new(),
        metadata,
    )
    .await
    .map_err(|err| err.to_string())
}

fn resolve_webhook_agent_model(
    runtime: &RuntimeBundle,
    route_provider: &str,
    route_model: &str,
    request_provider: Option<&str>,
    request_model: Option<&str>,
) -> Result<(String, String), String> {
    let requested_provider = request_provider
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let requested_model = request_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let provider = requested_provider
        .clone()
        .unwrap_or_else(|| route_provider.to_string());
    if !runtime.provider_default_models.contains_key(&provider) {
        let all = runtime
            .provider_default_models
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "unknown provider `{provider}`, available providers: {all}"
        ));
    }
    let model = requested_model.unwrap_or_else(|| {
        if requested_provider.is_some() {
            default_model_for_provider(runtime, &provider).to_string()
        } else {
            route_model.to_string()
        }
    });
    Ok((provider, model))
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
                json!(
                    conversation_history
                        .into_iter()
                        .map(|record| {
                            json!({
                                "role": record.role,
                                "content": record.content,
                            })
                        })
                        .collect::<Vec<_>>()
                ),
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
    let mcp_summary = match &runtime.mcp_init {
        Some(handle) => {
            let mut guard = handle.lock().await;
            if guard.is_ready() {
                Some(
                    guard
                        .wait_until_ready()
                        .await
                        .map_err(|err| config_err(format!("mcp init failed: {err}")))?,
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

fn should_emit_outbound(msg: &Envelope<OutboundMessage>) -> bool {
    !should_suppress_output(&msg.payload.content, &msg.payload.metadata)
}

#[cfg(test)]
mod tests {
    use super::{
        RuntimeBundle, StartupReport, build_history_for_model,
        build_new_session_bootstrap_user_message, build_unavailable_provider,
        compression_trigger_interval, configured_default_model, extract_skill_short_description,
        first_arg_token, format_approve_already_handled_message,
        format_new_session_started_message, handle_im_command, normalize_runtime_provider_override,
        parse_im_command, resolve_new_session_target_from_config, resolve_session_route,
        resolve_webhook_agent_model, should_emit_outbound, should_trigger_compression,
        spawn_llm_audit_writer, trim_conversation_history,
    };
    use klaw_agent::ConversationSummary;
    use klaw_config::{AppConfig, ModelProviderConfig};
    use klaw_core::{
        CircuitBreakerPolicy, DeadLetterPolicy, Envelope, EnvelopeHeader,
        ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker, InMemoryIdempotencyStore,
        InMemoryTransport, OutboundMessage, QueueStrategy, RunLimits, SessionSchedulingPolicy,
        Subscription,
    };
    use klaw_llm::{ChatOptions, LlmError, LlmProvider};
    use klaw_session::{ChatRecord, SessionManager, SqliteSessionManager};
    use klaw_storage::ApprovalStatus;
    use klaw_storage::{DefaultSessionStore, StoragePaths};
    use klaw_tool::ToolRegistry;
    use klaw_util::EnvironmentCheckReport;
    use serde_json::{Value, json};
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{Arc, Mutex, RwLock},
        time::Duration,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[derive(Default, Clone)]
    struct BootstrapCaptureProvider {
        last_user_message: Arc<Mutex<Option<String>>>,
        last_tool_choice: Arc<Mutex<Option<Value>>>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for BootstrapCaptureProvider {
        fn name(&self) -> &str {
            "test-provider"
        }

        fn default_model(&self) -> &str {
            "test-model"
        }

        async fn chat(
            &self,
            messages: Vec<klaw_llm::LlmMessage>,
            _tools: Vec<klaw_llm::ToolDefinition>,
            _model: Option<&str>,
            options: ChatOptions,
        ) -> Result<klaw_llm::LlmResponse, LlmError> {
            let last_user_message = messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .map(|message| message.content.clone());
            *self
                .last_user_message
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = last_user_message;
            *self
                .last_tool_choice
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = options.tool_choice;
            Ok(klaw_llm::LlmResponse {
                content: "bootstrap reply".to_string(),
                reasoning: None,
                tool_calls: Vec::new(),
                usage: None,
                usage_source: None,
                audit: None,
            })
        }
    }

    async fn create_test_store() -> DefaultSessionStore {
        let root = std::env::temp_dir().join(format!("klaw-runtime-test-{}", Uuid::new_v4()));
        DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("test session store should open")
    }

    async fn build_test_runtime(provider: Arc<dyn LlmProvider>) -> RuntimeBundle {
        let session_store = create_test_store().await;
        let runtime = klaw_core::AgentLoop::new_with_identity(
            RunLimits {
                max_tool_iterations: 0,
                max_tool_calls: 0,
                token_budget: 0,
                agent_timeout: Duration::from_secs(5),
                tool_timeout: Duration::from_secs(5),
            },
            SessionSchedulingPolicy {
                strategy: QueueStrategy::Collect,
                max_queue_depth: 8,
                lock_ttl: Duration::from_secs(5),
            },
            provider.clone(),
            "test-provider".to_string(),
            "test-model".to_string(),
            ToolRegistry::default(),
        )
        .with_provider_registry(BTreeMap::from([("test-provider".to_string(), provider)]));
        RuntimeBundle {
            runtime,
            default_provider_id: "test-provider".to_string(),
            provider_default_models: BTreeMap::from([(
                "test-provider".to_string(),
                "test-model".to_string(),
            )]),
            runtime_provider_override: Arc::new(RwLock::new(None)),
            disable_session_commands_for: BTreeSet::new(),
            inbound_transport: InMemoryTransport::new(),
            outbound_transport: InMemoryTransport::new(),
            deadletter_transport: InMemoryTransport::new(),
            idempotency: InMemoryIdempotencyStore::default(),
            retry_policy: ExponentialBackoffRetryPolicy {
                max_attempts: 3,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
                jitter_ratio: 0.0,
            },
            deadletter_policy: DeadLetterPolicy {
                topic: "agent.dlq",
                max_payload_bytes: 1024 * 1024,
                include_error_stack: false,
            },
            circuit_breaker: InMemoryCircuitBreaker::new(CircuitBreakerPolicy {
                failure_threshold: 5,
                open_interval: Duration::from_secs(1),
                half_open_max_requests: 1,
            }),
            subscription: Subscription {
                topic: "agent.inbound",
                consumer_group: "test".to_string(),
                visibility_timeout: Duration::from_secs(5),
            },
            session_store: session_store.clone(),
            mcp_init: None,
            startup_report: StartupReport::default(),
            observability: None,
            conversation_history_limit: 16,
            llm_audit_tx: spawn_llm_audit_writer(session_store),
            env_check: EnvironmentCheckReport {
                checks: Vec::new(),
                checked_at: OffsetDateTime::UNIX_EPOCH,
            },
        }
    }

    fn test_session_manager(runtime: &RuntimeBundle) -> SqliteSessionManager {
        SqliteSessionManager::from_store(runtime.session_store.clone())
    }

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
        assert_eq!(parse_im_command("/stop"), Some(("stop", None)));
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
    fn new_session_bootstrap_user_message_mentions_bootstrap_or_greeting() {
        let message = build_new_session_bootstrap_user_message();
        assert!(message.contains("You just woke up"));
        assert!(message.contains("If `BOOTSTRAP.md` exists, use the available workspace tools"));
        assert!(message.contains("start with a short, natural greeting"));
        assert!(message.contains(
            "Do not claim files were updated unless you actually changed them with tools"
        ));
    }

    #[test]
    fn skill_description_prefers_frontmatter_description() {
        let markdown = r#"---
name: pdf
description: "Use this skill whenever the user wants to do anything with PDF files."
---

# PDF Processing Guide

## Overview

This body line should not be used.
"#;

        assert_eq!(
            extract_skill_short_description(markdown),
            "Use this skill whenever the user wants to do anything with PDF files."
        );
    }

    #[test]
    fn skill_description_falls_back_to_body_when_frontmatter_has_no_description() {
        let markdown = r#"---
name: local-skill
license: Proprietary
---

# Local Skill

## Overview

Use this skill for local workflows.
"#;

        assert_eq!(
            extract_skill_short_description(markdown),
            "Use this skill for local workflows."
        );
    }

    #[test]
    fn skill_description_supports_body_only_skills() {
        let markdown = r#"# kubeease

## Overview

Kubeease 管理终端技能。
"#;

        assert_eq!(
            extract_skill_short_description(markdown),
            "Kubeease 管理终端技能。"
        );
    }

    #[test]
    fn skill_description_does_not_return_frontmatter_delimiter() {
        let markdown = r#"---
name: docx
license: Proprietary
---

# DOCX creation, editing, and analysis

## Overview

A .docx file is a ZIP archive containing XML files.
"#;

        assert_eq!(
            extract_skill_short_description(markdown),
            "A .docx file is a ZIP archive containing XML files."
        );
    }

    #[test]
    fn new_session_started_message_appends_assistant_reply_when_present() {
        let content = format_new_session_started_message(
            "telegram:main:child",
            "openai",
            "gpt-4.1",
            Some("hello there"),
        );
        assert!(content.contains("New session started"));
        assert!(content.contains("telegram:main:child"));
        assert!(content.contains("hello there"));
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
        assert!(
            model_history[0]
                .content
                .contains("Conversation Summary (JSON):")
        );
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

    #[tokio::test(flavor = "current_thread")]
    async fn new_command_writes_bootstrap_user_message_into_new_session() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider.clone()).await;
        let channel = "telegram".to_string();
        let base_session_key = "telegram:chat-1".to_string();
        let chat_id = "chat-1".to_string();
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                &base_session_key,
                &chat_id,
                &channel,
                "test-provider",
                "test-model",
            )
            .await
            .expect("base session should exist");

        let response = handle_im_command(
            &runtime,
            channel.clone(),
            base_session_key.clone(),
            chat_id.clone(),
            "/new".to_string(),
        )
        .await
        .expect("new command should succeed")
        .expect("new command should return a response");

        assert!(response.content.contains("New session started"));
        assert!(response.content.contains("bootstrap reply"));

        let route = resolve_session_route(&runtime, &channel, &base_session_key, &chat_id)
            .await
            .expect("new session route should resolve");
        assert_ne!(route.active_session_key, base_session_key);

        let new_history = sessions
            .read_chat_records(&route.active_session_key)
            .await
            .expect("new session history should load");
        assert_eq!(new_history.len(), 2);
        assert_eq!(new_history[0].role, "user");
        assert_eq!(
            new_history[0].content,
            build_new_session_bootstrap_user_message()
        );
        assert_eq!(new_history[1].role, "assistant");
        assert_eq!(new_history[1].content, "bootstrap reply");

        let base_history = sessions
            .read_chat_records(&base_session_key)
            .await
            .expect("base session history should load");
        assert!(base_history.is_empty());

        let captured = provider
            .last_user_message
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert_eq!(captured.as_deref(), Some(new_history[0].content.as_str()));
        let tool_choice = provider
            .last_tool_choice
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert_eq!(tool_choice, Some(Value::String("required".to_string())));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stop_command_returns_stopped_response_without_running_agent() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider.clone()).await;
        let channel = "telegram".to_string();
        let base_session_key = "telegram:chat-stop".to_string();
        let chat_id = "chat-stop".to_string();

        let response = handle_im_command(
            &runtime,
            channel,
            base_session_key,
            chat_id,
            "/stop".to_string(),
        )
        .await
        .expect("stop command should succeed")
        .expect("stop command should return a response");

        assert!(response.content.contains("Current turn stopped manually"));
        assert_eq!(
            response
                .metadata
                .get("turn.stopped")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            response.metadata.get("turn.stop_signal"),
            Some(&json!({
                "reason": "manual stop command",
                "source": "im_command"
            }))
        );
        assert_eq!(
            provider
                .last_user_message
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone(),
            None
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn webhook_agent_model_override_uses_requested_provider_default_model() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let mut runtime = build_test_runtime(provider).await;
        runtime
            .provider_default_models
            .insert("alt-provider".to_string(), "alt-default-model".to_string());

        let (resolved_provider, resolved_model) = resolve_webhook_agent_model(
            &runtime,
            "test-provider",
            "test-model",
            Some("alt-provider"),
            None,
        )
        .expect("provider override should resolve");

        assert_eq!(resolved_provider, "alt-provider");
        assert_eq!(resolved_model, "alt-default-model");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn webhook_agent_model_override_keeps_route_provider_when_only_model_is_requested() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;

        let (resolved_provider, resolved_model) = resolve_webhook_agent_model(
            &runtime,
            "test-provider",
            "test-model",
            None,
            Some("one-off-model"),
        )
        .expect("model override should resolve");

        assert_eq!(resolved_provider, "test-provider");
        assert_eq!(resolved_model, "one-off-model");
    }
}
