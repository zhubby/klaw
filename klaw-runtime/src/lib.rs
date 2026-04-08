pub mod env_check;
pub mod gateway_manager;
mod im_commands;
pub mod service_loop;
pub mod webhook;

use klaw_acp::{AcpConfigSnapshot, AcpInitHandle, AcpManager};
use klaw_agent::{
    AgentExecutionOutput, AgentExecutionStreamEvent, ConversationMessage, ConversationSummary,
    build_compression_prompt, build_provider_from_config, merge_or_reset_summary,
    parse_conversation_summary,
};
use klaw_approval::{
    ApprovalManager, ApprovalResolveDecision, ApprovalStatus, SqliteApprovalManager,
};
use klaw_channel::{
    ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, ChannelStreamEvent,
    ChannelStreamWriter, DefaultChannelDriverFactory, OutboundAttachment, OutboundAttachmentSource,
};
use klaw_config::{AppConfig, ConfigStore, McpConfig, TailscaleMode, ToolEnabled};
use klaw_core::{
    AgentLoop, AgentRuntimeError, AgentTelemetry, CircuitBreakerPolicy, DeadLetterMessage,
    DeadLetterPolicy, Envelope, EnvelopeHeader, ExponentialBackoffRetryPolicy,
    InMemoryCircuitBreaker, InMemoryIdempotencyStore, InMemoryTransport, InboundMessage,
    MediaReference, MessageTopic, MessageTransport, OutboundMessage, ProviderRuntimeSnapshot,
    QueueStrategy, RunLimits, SessionSchedulingPolicy, SkillPromptEntry, Subscription,
    TransportError, build_runtime_system_prompt, ensure_workspace_prompt_templates,
};
use klaw_gateway::{
    GatewayRuntimeInfo, GatewayWebhookAgentRequest, GatewayWebhookRequest, TailscaleHostInfo,
};
use klaw_heartbeat::{HeartbeatManager, should_suppress_output};
use klaw_llm::{ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolDefinition};
use klaw_mcp::{McpConfigSnapshot, McpInitHandle, McpManager, McpSyncResult};
use klaw_memory::{
    LongTermMemoryPromptOptions, SqliteMemoryService, SqliteMemoryStatsService,
    build_embedding_provider_from_config, render_long_term_memory_section,
};
use klaw_observability::{
    ObservabilityConfig, ObservabilityHandle, OtelAgentTelemetry, init_observability,
};
use klaw_session::{
    ChatRecord, LlmAuditStatus, LlmUsageSource, NewLlmAuditRecord, NewLlmUsageRecord,
    NewToolAuditRecord, SessionCompressionState, SessionIndex, SessionManager,
    SqliteSessionManager, ToolAuditStatus,
};
use klaw_skill::{
    InstalledSkill, RegistrySource, SkillSourceKind, SkillsManager, open_default_skills_manager,
};
use klaw_storage::{DefaultSessionStore, MemoryDb, open_default_memory_db, open_default_store};
use klaw_tool::{
    ApplyPatchTool, ApprovalTool, ArchiveTool, AskQuestionTool, ChannelAttachmentTool,
    CronManagerTool, GeoTool, HeartbeatManagerTool, LocalSearchTool, MemoryTool, ShellTool,
    SkillsManagerTool, SkillsRegistryTool, SqliteAskQuestionManager, SubAgentAuditSink,
    SubAgentTool, TerminalMultiplexerTool, ToolContext, ToolRegistry, VoiceTool, WebFetchTool,
    WebSearchTool,
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
};
use tokio::sync::mpsc;
use tokio::{sync::Mutex, time::timeout};
use tracing::{info, trace, warn};
use uuid::Uuid;

const LLM_AUDIT_QUEUE_CAPACITY: usize = 1024;
const TOOL_AUDIT_QUEUE_CAPACITY: usize = 1024;
const NEW_SESSION_BOOTSTRAP_USER_MESSAGE: &str = "You just woke up. Time to figure out who you are.\nThis is a brand new conversation. If `BOOTSTRAP.md` exists, use the available workspace tools to read it and follow it before anything else. If it does not exist, start with a short, natural greeting and do not recreate or restore `BOOTSTRAP.md`.\nGuide the user through initializing the agent's identity, vibe, and context. When you learn durable bootstrap details, use tools to update `IDENTITY.md` and `USER.md`, and delete `BOOTSTRAP.md` once bootstrap is truly complete. If `BOOTSTRAP.md` is already absent, leave it absent. Do not claim files were updated unless you actually changed them with tools. Do not mention that this message was auto-generated.";
const META_OUTBOUND_ATTACHMENTS_KEY: &str = "channel.attachments";

#[derive(Debug, Clone, Default)]
pub struct GatewayStatusSnapshot {
    pub configured_enabled: bool,
    pub running: bool,
    pub transitioning: bool,
    pub info: Option<GatewayRuntimeInfo>,
    pub tailscale_host: TailscaleHostInfo,
    pub last_error: Option<String>,
    pub auth_configured: bool,
    pub tailscale_mode: TailscaleMode,
}

#[derive(Debug, Clone, Default)]
pub struct StartupReport {
    pub skill_names: Vec<String>,
    pub tool_names: Vec<String>,
    pub mcp_summary: Option<McpSyncResult>,
}

pub struct RuntimeBundle {
    pub runtime: AgentLoop,
    pub base_system_prompt: Arc<RwLock<Option<String>>>,
    pub memory_db: Option<Arc<dyn MemoryDb>>,
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
    pub mcp_init: Mutex<McpInitHandle>,
    pub acp_init: Mutex<AcpInitHandle>,
    pub startup_report: StartupReport,
    pub observability: Option<ObservabilityHandle>,
    pub conversation_history_limit: usize,
    pub llm_audit_tx: std::sync::mpsc::SyncSender<NewLlmAuditRecord>,
    pub tool_audit_tx: std::sync::mpsc::SyncSender<NewToolAuditRecord>,
    pub env_check: EnvironmentCheckReport,
}

pub struct SharedChannelRuntime {
    runtime: Arc<RuntimeBundle>,
    background: Arc<service_loop::BackgroundServices>,
}

#[derive(Clone)]
pub struct HostedRuntime {
    pub runtime: Arc<RuntimeBundle>,
    pub background: Arc<service_loop::BackgroundServices>,
    pub adapter: Arc<SharedChannelRuntime>,
    pub startup_report: StartupReport,
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

pub async fn build_hosted_runtime(config: &AppConfig) -> Result<HostedRuntime, Box<dyn Error>> {
    let mut runtime = build_runtime_bundle(config).await?;
    let startup_report = finalize_startup_report(&mut runtime).await?;
    let runtime = Arc::new(runtime);
    let background = Arc::new(service_loop::BackgroundServices::new(
        runtime.as_ref(),
        service_loop::BackgroundServiceConfig::from_app_config(config),
    ));
    let adapter = Arc::new(SharedChannelRuntime::new(
        Arc::clone(&runtime),
        Arc::clone(&background),
    ));
    Ok(HostedRuntime {
        runtime,
        background,
        adapter,
        startup_report,
    })
}

#[async_trait::async_trait(?Send)]
impl ChannelRuntime for SharedChannelRuntime {
    async fn submit(&self, request: ChannelRequest) -> ChannelResult<Option<ChannelResponse>> {
        if !is_channel_commands_disabled(self.runtime.as_ref(), &request.channel)
            && request.input.trim_start().starts_with('/')
        {
            if let Some(response) = im_commands::try_handle(
                self.runtime.as_ref(),
                request.channel.clone(),
                request.session_key.clone(),
                request.chat_id.clone(),
                request.input.clone(),
                request.metadata.clone(),
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

        Ok(maybe_output
            .map(|output| channel_response(output.content, output.reasoning, output.metadata)))
    }

    async fn submit_streaming(
        &self,
        request: ChannelRequest,
        writer: &mut dyn ChannelStreamWriter,
    ) -> ChannelResult<Option<ChannelResponse>> {
        if !is_channel_commands_disabled(self.runtime.as_ref(), &request.channel)
            && request.input.trim_start().starts_with('/')
        {
            if let Some(response) = im_commands::try_handle(
                self.runtime.as_ref(),
                request.channel.clone(),
                request.session_key.clone(),
                request.chat_id.clone(),
                request.input.clone(),
                request.metadata.clone(),
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

        Ok(maybe_output
            .map(|output| channel_response(output.content, output.reasoning, output.metadata)))
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

fn parse_outbound_attachments(
    metadata: &BTreeMap<String, serde_json::Value>,
) -> Vec<OutboundAttachment> {
    metadata
        .get(META_OUTBOUND_ATTACHMENTS_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<OutboundAttachment>>(value).ok())
        .map(|attachments| {
            attachments
                .into_iter()
                .filter_map(|mut attachment| {
                    attachment.source = match attachment.source {
                        OutboundAttachmentSource::ArchiveId { archive_id } => {
                            let archive_id = archive_id.trim().to_string();
                            if archive_id.is_empty() {
                                return None;
                            }
                            OutboundAttachmentSource::ArchiveId { archive_id }
                        }
                        OutboundAttachmentSource::LocalPath { path } => {
                            let path = path.trim().to_string();
                            if path.is_empty() {
                                return None;
                            }
                            OutboundAttachmentSource::LocalPath { path }
                        }
                    };
                    attachment.filename = attachment.filename.and_then(|value| {
                        let trimmed = value.trim().to_string();
                        (!trimmed.is_empty()).then_some(trimmed)
                    });
                    attachment.caption = attachment.caption.and_then(|value| {
                        let trimmed = value.trim().to_string();
                        (!trimmed.is_empty()).then_some(trimmed)
                    });
                    Some(attachment)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn channel_response(
    content: String,
    reasoning: Option<String>,
    metadata: BTreeMap<String, serde_json::Value>,
) -> ChannelResponse {
    let attachments = parse_outbound_attachments(&metadata);
    ChannelResponse {
        content,
        reasoning,
        metadata,
        attachments,
    }
}

pub fn build_channel_driver_factory(
    config: &AppConfig,
) -> Result<DefaultChannelDriverFactory, Box<dyn Error>> {
    Ok(DefaultChannelDriverFactory::new(
        config.tools.channel_attachment.local_attachments.clone(),
    ))
}

const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";
const META_PROVIDER_KEY: &str = "agent.provider_id";
const META_MODEL_KEY: &str = "agent.model";
const META_TOOL_CHOICE_KEY: &str = "agent.tool_choice";
const META_MAX_TOOL_ITERATIONS_KEY: &str = "agent.max_tool_iterations";
const META_MAX_TOOL_CALLS_KEY: &str = "agent.max_tool_calls";

#[derive(Debug, Clone)]
struct SessionRoute {
    active_session_key: String,
    model_provider: String,
    model: String,
}

#[derive(Debug, Clone)]
struct WebhookExecutionRoute {
    session_key: String,
    chat_id: String,
    model_provider: String,
    model: String,
}

#[derive(Debug, Clone)]
struct WebhookDeliveryRoute {
    session_key: String,
    chat_id: String,
    channel: String,
    model_provider: String,
    model: String,
    metadata: BTreeMap<String, Value>,
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
    enqueue_llm_audit_records(
        &runtime.llm_audit_tx,
        session_key,
        chat_id,
        turn_index,
        &message_id.to_string(),
        &outcome.llm_audits,
        None,
    );
    enqueue_tool_audit_records(
        &runtime.tool_audit_tx,
        session_key,
        chat_id,
        turn_index,
        &message_id.to_string(),
        &outcome.tool_audits,
        None,
    );
}

fn enqueue_llm_audit_records(
    llm_audit_tx: &std::sync::mpsc::SyncSender<NewLlmAuditRecord>,
    session_key: &str,
    chat_id: &str,
    turn_index: i64,
    id_prefix: &str,
    audits: &[klaw_llm::LlmAuditPayload],
    metadata_json: Option<String>,
) {
    for (index, record) in audits.iter().enumerate() {
        let payload = NewLlmAuditRecord {
            id: format!("{id_prefix}:audit:{}", index + 1),
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
            metadata_json: metadata_json.clone(),
            requested_at_ms: record.requested_at_ms,
            responded_at_ms: record.responded_at_ms,
        };
        if let Err(err) = llm_audit_tx.try_send(payload) {
            warn!(error = %err, session_key, "llm audit queue full or disconnected; dropping record");
        }
    }
}

fn serialize_json_string(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn enqueue_tool_audit_records(
    tool_audit_tx: &std::sync::mpsc::SyncSender<NewToolAuditRecord>,
    session_key: &str,
    chat_id: &str,
    turn_index: i64,
    id_prefix: &str,
    audits: &[klaw_agent::AgentToolAudit],
    metadata_json: Option<String>,
) {
    for audit in audits {
        let payload = NewToolAuditRecord {
            id: format!(
                "{id_prefix}:tool:{}:{}",
                audit.request_seq, audit.tool_call_seq
            ),
            session_key: session_key.to_string(),
            chat_id: chat_id.to_string(),
            turn_index,
            request_seq: audit.request_seq,
            tool_call_seq: audit.tool_call_seq,
            tool_name: audit.tool_name.clone(),
            status: if audit.result.ok {
                ToolAuditStatus::Success
            } else {
                ToolAuditStatus::Failed
            },
            error_code: audit.result.error_code.clone(),
            error_message: audit.result.error_code.as_ref().map(|code| {
                if code == "approval_required" {
                    "approval requested".to_string()
                } else {
                    audit.result.content_for_model.clone()
                }
            }),
            retryable: audit.result.retryable,
            approval_required: audit
                .result
                .signals
                .iter()
                .any(|signal| signal.kind == "approval_required"),
            arguments_json: serialize_json_string(&audit.arguments),
            result_content: audit.result.content_for_model.clone(),
            error_details_json: audit
                .result
                .error_details
                .as_ref()
                .map(serialize_json_string),
            signals_json: (!audit.result.signals.is_empty()).then(|| {
                serde_json::to_string(&audit.result.signals).unwrap_or_else(|_| "[]".to_string())
            }),
            metadata_json: merge_tool_audit_metadata(metadata_json.clone(), &audit.tool_call_id),
            started_at_ms: audit.started_at_ms,
            finished_at_ms: audit.finished_at_ms,
        };
        if let Err(err) = tool_audit_tx.try_send(payload) {
            warn!(error = %err, session_key, "tool audit queue full or disconnected; dropping record");
        }
    }
}

fn merge_tool_audit_metadata(
    metadata_json: Option<String>,
    tool_call_id: &Option<String>,
) -> Option<String> {
    if metadata_json.is_none() && tool_call_id.is_none() {
        return None;
    }

    let mut metadata = metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| json!({}));
    if let Some(tool_call_id) = tool_call_id.as_ref() {
        metadata["tool_call_id"] = Value::String(tool_call_id.clone());
    }
    serde_json::to_string(&metadata).ok()
}

#[derive(Clone)]
struct RuntimeSubAgentAuditSink {
    session_store: DefaultSessionStore,
    llm_audit_tx: std::sync::mpsc::SyncSender<NewLlmAuditRecord>,
    tool_audit_tx: std::sync::mpsc::SyncSender<NewToolAuditRecord>,
}

impl RuntimeSubAgentAuditSink {
    fn new(
        session_store: DefaultSessionStore,
        llm_audit_tx: std::sync::mpsc::SyncSender<NewLlmAuditRecord>,
        tool_audit_tx: std::sync::mpsc::SyncSender<NewToolAuditRecord>,
    ) -> Self {
        Self {
            session_store,
            llm_audit_tx,
            tool_audit_tx,
        }
    }
}

#[async_trait::async_trait]
impl SubAgentAuditSink for RuntimeSubAgentAuditSink {
    async fn persist_sub_agent_audits(
        &self,
        parent_session_key: &str,
        child_session_key: &str,
        output: &AgentExecutionOutput,
    ) -> Result<(), String> {
        if output.request_audits.is_empty() && output.tool_audits.is_empty() {
            return Ok(());
        }

        let sessions = SqliteSessionManager::from_store(self.session_store.clone());
        let session = sessions
            .get_session(parent_session_key)
            .await
            .map_err(|err| format!("load parent session failed: {err}"))?;
        let metadata_json = serde_json::to_string(&json!({
            "sub_agent": true,
            "sub_agent.parent_session_key": parent_session_key,
            "sub_agent.child_session_key": child_session_key,
            "sub_agent.source_tool": "sub_agent",
        }))
        .map_err(|err| format!("serialize sub-agent audit metadata failed: {err}"))?;
        let audits = output
            .request_audits
            .iter()
            .map(|record| record.payload.clone())
            .collect::<Vec<_>>();
        enqueue_llm_audit_records(
            &self.llm_audit_tx,
            parent_session_key,
            &session.chat_id,
            session.turn_count,
            child_session_key,
            &audits,
            Some(metadata_json.clone()),
        );
        enqueue_tool_audit_records(
            &self.tool_audit_tx,
            parent_session_key,
            &session.chat_id,
            session.turn_count,
            child_session_key,
            &output.tool_audits,
            Some(metadata_json),
        );
        Ok(())
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
    sync_base_session_heartbeat(runtime, &base).await?;
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
            default_model_for_provider(runtime, &model_provider)
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

fn inherited_channel_runtime_metadata(
    metadata: &BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    metadata
        .iter()
        .filter(|(key, _)| key.starts_with("channel."))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn build_new_session_bootstrap_request_metadata(
    request_metadata: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut metadata = inherited_channel_runtime_metadata(request_metadata);
    metadata.insert(
        META_TOOL_CHOICE_KEY.to_string(),
        Value::String("required".to_string()),
    );
    metadata
}

fn build_approved_shell_followup_request_metadata(
    request_metadata: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut metadata = inherited_channel_runtime_metadata(request_metadata);
    metadata.insert(META_MAX_TOOL_ITERATIONS_KEY.to_string(), Value::from(2_u64));
    metadata.insert(META_MAX_TOOL_CALLS_KEY.to_string(), Value::from(1_u64));
    metadata
}

fn build_ask_question_followup_request_metadata(
    request_metadata: &BTreeMap<String, Value>,
    question_id: &str,
    question_text: &str,
    selected_option_id: &str,
    selected_option_label: &str,
) -> BTreeMap<String, Value> {
    let mut metadata = inherited_channel_runtime_metadata(request_metadata);
    metadata.insert(
        "ask_question.answer".to_string(),
        json!({
            "question_id": question_id,
            "question": question_text,
            "selected_option_id": selected_option_id,
            "selected_option_label": selected_option_label,
        }),
    );
    metadata
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

fn provider_runtime_snapshot(runtime: &RuntimeBundle) -> ProviderRuntimeSnapshot {
    runtime.runtime.provider_runtime_snapshot()
}

fn runtime_default_route(runtime: &RuntimeBundle) -> (String, String) {
    let snapshot = provider_runtime_snapshot(runtime);
    if let Some(provider_id) = runtime_provider_override(runtime) {
        let model = snapshot
            .provider_default_models
            .get(&provider_id)
            .cloned()
            .unwrap_or_else(|| snapshot.default_model.clone());
        return (provider_id, model);
    }

    (
        snapshot.default_provider_id.clone(),
        snapshot.default_model.clone(),
    )
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
    let provider_runtime = provider_runtime_snapshot(runtime);
    let has_route_override = session.model_provider.is_some() || session.model.is_some();
    let has_explicit_override = session.model_provider_explicit || session.model_explicit;
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

    if has_route_override && !has_explicit_override {
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

    if session.model_provider.as_deref().is_some_and(|provider| {
        !provider_runtime
            .provider_default_models
            .contains_key(provider)
    }) {
        return Ok(sessions
            .clear_model_routing_override(&session.session_key, chat_id, channel)
            .await?);
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

pub fn set_runtime_provider_override(
    runtime: &RuntimeBundle,
    provider_id: Option<&str>,
) -> Result<(String, String), Box<dyn Error>> {
    let snapshot = provider_runtime_snapshot(runtime);
    let (next, active_provider) = normalize_runtime_provider_override(
        &snapshot.provider_default_models,
        &snapshot.default_provider_id,
        provider_id,
    )?;

    let mut guard = runtime
        .runtime_provider_override
        .write()
        .unwrap_or_else(|err| err.into_inner());
    *guard = next;
    drop(guard);

    let active_model = default_model_for_provider(runtime, &active_provider);
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

fn spawn_mcp_init(tools: &ToolRegistry, config: &McpConfig) -> Mutex<McpInitHandle> {
    let snapshot = McpConfigSnapshot::from_mcp_config(config);
    Mutex::new(McpManager::spawn_init(tools.clone(), snapshot))
}

fn spawn_acp_init(tools: &ToolRegistry, config: &klaw_config::AcpConfig) -> Mutex<AcpInitHandle> {
    let snapshot = AcpConfigSnapshot::from_config(config);
    Mutex::new(AcpManager::spawn_init(tools.clone(), snapshot))
}

fn default_model_for_provider(runtime: &RuntimeBundle, provider_id: &str) -> String {
    let snapshot = provider_runtime_snapshot(runtime);
    snapshot
        .provider_default_models
        .get(provider_id)
        .cloned()
        .unwrap_or(snapshot.default_model)
}

fn build_provider_runtime_snapshot(
    config: &AppConfig,
) -> Result<ProviderRuntimeSnapshot, Box<dyn Error>> {
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

    Ok(ProviderRuntimeSnapshot {
        default_provider,
        provider_registry,
        default_provider_id: config.model_provider.clone(),
        default_model,
        provider_default_models,
    })
}

async fn register_configured_tools(
    tools: &mut ToolRegistry,
    config: &AppConfig,
    session_store: DefaultSessionStore,
    memory_db: Option<Arc<dyn MemoryDb>>,
    sub_agent_audit_sink: Option<Arc<dyn SubAgentAuditSink>>,
) -> Result<(), Box<dyn Error>> {
    if config.tools.archive.enabled() {
        tools.register(ArchiveTool::open_default(config).await?);
    }
    if config.tools.channel_attachment.enabled() {
        tools.register(ChannelAttachmentTool::open_default(config).await?);
    }
    if voice_tool_is_enabled(config) {
        tools.register(VoiceTool::open_default(config).await?);
    } else if config.tools.voice.enabled() && !config.voice.enabled {
        info!("voice tool disabled because voice.enabled=false");
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
    if config.tools.ask_question.enabled() {
        tools.register(AskQuestionTool::with_store(config, session_store.clone()));
    }
    if config.tools.geo.enabled() {
        tools.register(GeoTool::new());
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
        let Some(memory_db) = memory_db else {
            return Err("memory tool enabled but memory db is unavailable".into());
        };
        let embedding_provider = if config.memory.embedding.enabled {
            build_embedding_provider_from_config(config).ok()
        } else {
            None
        };
        let memory_service = SqliteMemoryService::new(memory_db, embedding_provider).await?;
        tools.register(MemoryTool::with_store(
            Arc::new(memory_service),
            session_store.clone(),
            config,
        ));
    }
    if config.tools.web_fetch.enabled() {
        tools.register(WebFetchTool::new(config));
    }
    if config.tools.web_search.enabled() {
        tools.register(WebSearchTool::new(config)?);
    }
    if config.tools.sub_agent.enabled() {
        let parent_tools = tools.clone();
        tools.register(SubAgentTool::with_audit_sink(
            Arc::new(config.clone()),
            parent_tools,
            sub_agent_audit_sink,
        ));
    }
    Ok(())
}

fn voice_tool_is_enabled(config: &AppConfig) -> bool {
    config.tools.voice.enabled() && config.voice.enabled
}

pub fn sync_runtime_providers(
    runtime: &RuntimeBundle,
    config: &AppConfig,
) -> Result<(String, String), Box<dyn Error>> {
    let provider_runtime = build_provider_runtime_snapshot(config)?;
    runtime
        .runtime
        .set_provider_runtime_snapshot(provider_runtime.clone());

    let next_override = runtime_provider_override(runtime).and_then(|provider_id| {
        provider_runtime
            .provider_default_models
            .contains_key(&provider_id)
            .then_some(provider_id)
    });
    let active_provider = next_override
        .clone()
        .unwrap_or_else(|| provider_runtime.default_provider_id.clone());
    let mut guard = runtime
        .runtime_provider_override
        .write()
        .unwrap_or_else(|err| err.into_inner());
    *guard = next_override;
    drop(guard);

    let active_model = provider_runtime
        .provider_default_models
        .get(&active_provider)
        .cloned()
        .unwrap_or_else(|| provider_runtime.default_model.clone());
    Ok((active_provider, active_model))
}

pub async fn sync_runtime_tools(
    runtime: &RuntimeBundle,
    config: &AppConfig,
) -> Result<Vec<String>, Box<dyn Error>> {
    let mut next_tools = ToolRegistry::default();
    let sub_agent_audit_sink: Arc<dyn SubAgentAuditSink> = Arc::new(RuntimeSubAgentAuditSink::new(
        runtime.session_store.clone(),
        runtime.llm_audit_tx.clone(),
        runtime.tool_audit_tx.clone(),
    ));
    register_configured_tools(
        &mut next_tools,
        config,
        runtime.session_store.clone(),
        runtime.memory_db.clone(),
        Some(sub_agent_audit_sink),
    )
    .await?;
    let builtin_tool_names = builtin_tool_names(config);
    let builtin_tool_name_refs: Vec<&str> = builtin_tool_names.iter().map(String::as_str).collect();

    let mut live_tools = runtime.runtime.tools.clone();
    live_tools.unregister_many(&builtin_tool_name_refs);
    for name in next_tools.list() {
        if let Some(tool) = next_tools.get(&name) {
            live_tools.register_shared(tool);
        }
    }

    Ok(live_tools.list())
}

fn builtin_tool_names(config: &AppConfig) -> Vec<String> {
    serde_json::to_value(&config.tools)
        .ok()
        .and_then(|value| {
            value.as_object().map(|map| {
                map.keys()
                    .map(String::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default()
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

fn ask_question_manager(runtime: &RuntimeBundle) -> SqliteAskQuestionManager {
    SqliteAskQuestionManager::from_store(runtime.session_store.clone())
}

fn session_manager(runtime: &RuntimeBundle) -> SqliteSessionManager {
    SqliteSessionManager::from_store(runtime.session_store.clone())
}

fn supports_channel_heartbeat(channel: &str) -> bool {
    matches!(channel, "telegram" | "dingtalk")
}

async fn sync_base_session_heartbeat(
    runtime: &RuntimeBundle,
    session: &SessionIndex,
) -> Result<(), Box<dyn Error>> {
    if !supports_channel_heartbeat(&session.channel) {
        return Ok(());
    }
    let manager = HeartbeatManager::new(Arc::new(runtime.session_store.clone()));
    manager
        .sync_job_to_session(&session.session_key, &session.channel, &session.chat_id)
        .await?;
    Ok(())
}

fn persistable_session_delivery_metadata_json(
    metadata: &BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    let delivery_metadata = metadata
        .iter()
        .filter_map(|(key, value)| match key.as_str() {
            "channel.dingtalk.session_webhook" | "channel.dingtalk.bot_title" => {
                Some((key.clone(), value.clone()))
            }
            _ => None,
        })
        .collect::<serde_json::Map<String, serde_json::Value>>();
    if delivery_metadata.is_empty() {
        return None;
    }
    serde_json::to_string(&delivery_metadata).ok()
}

async fn persist_session_delivery_metadata(
    sessions: &SqliteSessionManager,
    session_key: &str,
    chat_id: &str,
    channel: &str,
    request_metadata: &BTreeMap<String, serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let delivery_metadata_json = persistable_session_delivery_metadata_json(request_metadata);
    if delivery_metadata_json.is_none() {
        return Ok(());
    }
    sessions
        .set_delivery_metadata(
            session_key,
            chat_id,
            channel,
            delivery_metadata_json.as_deref(),
        )
        .await?;
    Ok(())
}

fn parse_delivery_metadata_json(raw: &str) -> Option<BTreeMap<String, Value>> {
    serde_json::from_str::<serde_json::Map<String, Value>>(raw)
        .ok()
        .map(|metadata| metadata.into_iter().collect())
}

fn validate_webhook_delivery_target(route: &WebhookDeliveryRoute) -> Result<(), String> {
    match route.channel.as_str() {
        "telegram" => Ok(()),
        "dingtalk" => {
            let has_session_webhook = route
                .metadata
                .get("channel.dingtalk.session_webhook")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty());
            if has_session_webhook {
                Ok(())
            } else {
                Err(format!(
                    "delivery target '{}' is missing dingtalk session webhook metadata",
                    route.session_key
                ))
            }
        }
        channel => Err(format!(
            "delivery target channel '{channel}' does not support background webhook delivery"
        )),
    }
}

async fn resolve_webhook_delivery_route(
    runtime: &RuntimeBundle,
    base_session_key: Option<&str>,
) -> Result<Option<WebhookDeliveryRoute>, String> {
    let Some(base_session_key) = base_session_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let sessions = session_manager(runtime);
    let (default_provider_id, default_model) = runtime_default_route(runtime);
    let base = sessions
        .get_session(base_session_key)
        .await
        .map_err(|err| format!("base delivery session '{base_session_key}' not found: {err}"))?;
    if base.channel == "webhook" {
        return Err(format!(
            "base_session_key '{}' points to a webhook execution session; choose the originating IM base session instead",
            base.session_key
        ));
    }
    let base_chat_id = base.chat_id.clone();
    let base_channel = base.channel.clone();
    let base = normalize_session_route_state(
        runtime,
        base,
        &base_chat_id,
        &base_channel,
        &default_provider_id,
        &default_model,
    )
    .await
    .map_err(|err| err.to_string())?;
    let target_session_key = base
        .active_session_key
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| base.session_key.clone());
    let target = if target_session_key == base.session_key {
        base
    } else {
        let target = sessions
            .get_session(&target_session_key)
            .await
            .map_err(|err| {
                format!("active delivery session '{target_session_key}' not found: {err}")
            })?;
        let target_chat_id = target.chat_id.clone();
        let target_channel = target.channel.clone();
        normalize_session_route_state(
            runtime,
            target,
            &target_chat_id,
            &target_channel,
            &default_provider_id,
            &default_model,
        )
        .await
        .map_err(|err| err.to_string())?
    };
    if target.channel == "webhook" {
        return Err(format!(
            "base_session_key '{}' currently routes to webhook execution session '{}'; choose the originating IM base session instead",
            base_session_key, target.session_key
        ));
    }
    let model_provider = target
        .model_provider
        .clone()
        .unwrap_or_else(|| default_provider_id.clone());
    let model = target.model.clone().unwrap_or_else(|| {
        if model_provider == default_provider_id {
            default_model.clone()
        } else {
            default_model_for_provider(runtime, &model_provider)
        }
    });
    let metadata = target
        .delivery_metadata_json
        .as_deref()
        .and_then(parse_delivery_metadata_json)
        .unwrap_or_default();
    let route = WebhookDeliveryRoute {
        session_key: target.session_key,
        chat_id: target.chat_id,
        channel: target.channel,
        model_provider,
        model,
        metadata,
    };
    validate_webhook_delivery_target(&route)?;
    Ok(Some(route))
}

fn build_webhook_execution_route(
    request_session_key: &str,
    request_chat_id: &str,
    default_provider: &str,
    default_model: &str,
) -> WebhookExecutionRoute {
    WebhookExecutionRoute {
        session_key: request_session_key.to_string(),
        chat_id: request_chat_id.to_string(),
        model_provider: default_provider.to_string(),
        model: default_model.to_string(),
    }
}

pub async fn build_runtime_bundle(config: &AppConfig) -> Result<RuntimeBundle, Box<dyn Error>> {
    info!(
        provider = %config.model_provider,
        "building runtime bundle"
    );
    let provider_runtime = build_provider_runtime_snapshot(config)?;
    let default_provider = Arc::clone(&provider_runtime.default_provider);
    let default_model = provider_runtime.default_model.clone();
    let session_store = open_default_store().await?;
    let memory_db = if config.tools.memory.enabled() {
        Some(Arc::new(open_default_memory_db().await?) as Arc<dyn MemoryDb>)
    } else {
        None
    };
    let llm_audit_tx = spawn_llm_audit_writer(session_store.clone());
    let tool_audit_tx = spawn_tool_audit_writer(session_store.clone());
    let mut tools = ToolRegistry::default();
    let sub_agent_audit_sink: Arc<dyn SubAgentAuditSink> = Arc::new(RuntimeSubAgentAuditSink::new(
        session_store.clone(),
        llm_audit_tx.clone(),
        tool_audit_tx.clone(),
    ));
    register_configured_tools(
        &mut tools,
        config,
        session_store.clone(),
        memory_db.clone(),
        Some(sub_agent_audit_sink),
    )
    .await?;

    let configured_mcp_servers = config
        .mcp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .count();
    info!(
        configured_servers = configured_mcp_servers,
        startup_timeout_seconds = config.mcp.startup_timeout_seconds,
        "bootstrapping mcp servers"
    );
    let mcp_init = spawn_mcp_init(&tools, &config.mcp);
    let configured_acp_agents = config
        .acp
        .agents
        .iter()
        .filter(|agent| agent.enabled)
        .count();
    info!(
        configured_agents = configured_acp_agents,
        startup_timeout_seconds = config.acp.startup_timeout_seconds,
        "bootstrapping acp agents"
    );
    let acp_init = spawn_acp_init(&tools, &config.acp);

    ensure_workspace_prompt_templates_if_possible().await;
    let loaded_skills = load_skills_system_prompt(config).await;
    let skill_names = loaded_skills.skill_names.clone();
    let base_system_prompt = build_runtime_system_prompt(loaded_skills.skill_entries);
    let system_prompt = compose_system_prompt(
        base_system_prompt.clone(),
        match memory_db.clone() {
            Some(memory_db) => render_long_term_memory_prompt(memory_db).await?,
            None => None,
        },
    );

    let observability = init_observability_from_config(config).await;
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
        provider_runtime.default_provider_id.clone(),
        default_model,
        tools,
    )
    .with_provider_registry(provider_runtime.provider_registry.clone())
    .with_system_prompt(system_prompt);

    runtime.set_provider_runtime_snapshot(provider_runtime);

    if let Some(ref tel) = telemetry {
        runtime = runtime.with_telemetry(Arc::clone(tel));
    }

    let env_check = env_check::check_environment();
    let tool_names = runtime.tools.list();

    info!(
        tool_count = tool_names.len(),
        observability_enabled = observability.is_some(),
        "runtime bundle ready"
    );

    Ok(RuntimeBundle {
        runtime,
        base_system_prompt: Arc::new(RwLock::new(base_system_prompt)),
        memory_db,
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
            tool_names,
            mcp_summary: None,
        },
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
        acp_init,
        observability,
        conversation_history_limit: config.conversation_history_limit,
        llm_audit_tx,
        tool_audit_tx,
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

fn spawn_tool_audit_writer(
    session_store: DefaultSessionStore,
) -> std::sync::mpsc::SyncSender<NewToolAuditRecord> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<NewToolAuditRecord>(TOOL_AUDIT_QUEUE_CAPACITY);
    std::thread::Builder::new()
        .name("klaw-tool-audit-writer".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    warn!(error = %err, "failed to start tool audit writer runtime");
                    return;
                }
            };
            let manager = SqliteSessionManager::from_store(session_store);
            for record in rx {
                if let Err(err) = runtime.block_on(manager.append_tool_audit(&record)) {
                    warn!(error = %err, audit_id = record.id.as_str(), "failed to persist tool audit record");
                }
            }
        })
        .expect("tool audit writer should start");
    tx
}

pub async fn shutdown_runtime_bundle(runtime: &RuntimeBundle) -> Result<(), Box<dyn Error>> {
    let shutdown_deadline = Duration::from_secs(2);

    info!("shutting down runtime mcp servers");
    let mcp_manager = {
        let guard = runtime.mcp_init.lock().await;
        guard.manager()
    };
    match timeout(shutdown_deadline, mcp_manager.lock()).await {
        Ok(mut manager_guard) => {
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
        Err(_) => {
            warn!(
                timeout_seconds = shutdown_deadline.as_secs(),
                "mcp manager busy during shutdown; continuing process exit"
            );
        }
    }
    info!("shutting down runtime acp agents");
    let acp_manager = {
        let guard = runtime.acp_init.lock().await;
        guard.manager()
    };
    match timeout(shutdown_deadline, acp_manager.lock()).await {
        Ok(mut manager_guard) => {
            match timeout(shutdown_deadline, manager_guard.shutdown_all()).await {
                Ok(()) => {}
                Err(_) => {
                    warn!(
                        timeout_seconds = shutdown_deadline.as_secs(),
                        "acp shutdown timed out; continuing process exit"
                    );
                }
            }
        }
        Err(_) => {
            warn!(
                timeout_seconds = shutdown_deadline.as_secs(),
                "acp manager busy during shutdown; continuing process exit"
            );
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
    let base_system_prompt = build_runtime_system_prompt(loaded_skills.skill_entries);
    {
        let mut guard = runtime
            .base_system_prompt
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = base_system_prompt;
    }
    refresh_runtime_system_prompt(runtime).await;
    info!(skills = ?skill_names, "reloaded runtime skills prompt");
    Ok(skill_names)
}

async fn refresh_runtime_system_prompt(runtime: &RuntimeBundle) {
    let base_system_prompt = runtime
        .base_system_prompt
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let memory_section = match runtime.memory_db.clone() {
        Some(memory_db) => match render_long_term_memory_prompt(memory_db).await {
            Ok(section) => section,
            Err(err) => {
                warn!(error = %err, "failed to render long-term memory prompt section");
                None
            }
        },
        None => None,
    };
    runtime
        .runtime
        .set_system_prompt(compose_system_prompt(base_system_prompt, memory_section));
}

fn cli_long_term_memory_prompt_options() -> LongTermMemoryPromptOptions {
    LongTermMemoryPromptOptions {
        max_items: 12,
        max_chars: 1200,
        max_item_chars: 240,
    }
}

async fn render_long_term_memory_prompt(
    memory_db: Arc<dyn MemoryDb>,
) -> Result<Option<String>, Box<dyn Error>> {
    let stats = SqliteMemoryStatsService::new(memory_db);
    let records = stats.list_scope_records("long_term").await?;
    let section = render_long_term_memory_section(&records, &cli_long_term_memory_prompt_options());
    Ok(section.map(|content| format!("## Memory\n\n{content}")))
}

fn compose_system_prompt(
    base_system_prompt: Option<String>,
    memory_section: Option<String>,
) -> Option<String> {
    match (base_system_prompt, memory_section) {
        (Some(base), Some(memory)) => Some(format!(
            "{base}\n\n--------------------------------\n\n{memory}"
        )),
        (Some(base), None) => Some(base),
        (None, Some(memory)) => Some(memory),
        (None, None) => None,
    }
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
        .provider_runtime_snapshot()
        .provider_registry
        .get(model_provider)
        .cloned()
        .unwrap_or_else(|| provider_runtime_snapshot(runtime).default_provider);
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
    persist_session_delivery_metadata(
        &sessions,
        &session_key,
        &chat_id,
        &channel,
        &request_metadata,
    )
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
    info!(
        channel = %inbound_payload.channel,
        chat_id = %inbound_payload.chat_id,
        session_key = %inbound_payload.session_key,
        "channel inbound normalized"
    );
    trace!(inbound = ?inbound_payload, "channel inbound normalized");

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

async fn submit_webhook_isolated_turn(
    runtime: &RuntimeBundle,
    input: String,
    sender_id: String,
    execution: WebhookExecutionRoute,
    delivery_route: Option<WebhookDeliveryRoute>,
    base_session_key: Option<&str>,
    request_metadata: BTreeMap<String, Value>,
) -> Result<Option<AssistantOutput>, Box<dyn std::error::Error>> {
    let sessions = session_manager(runtime);
    let header = EnvelopeHeader::new(execution.session_key.clone());
    let user_record = ChatRecord::new("user", input.clone(), Some(header.message_id.to_string()));
    sessions
        .append_chat_record(&execution.session_key, &user_record)
        .await?;
    let session_state = sessions
        .touch_session(&execution.session_key, &execution.chat_id, "webhook")
        .await?;

    let mut inbound_metadata = request_metadata;
    inbound_metadata.insert(
        META_CONVERSATION_HISTORY_KEY.to_string(),
        Value::Array(Vec::new()),
    );
    inbound_metadata.insert(
        META_PROVIDER_KEY.to_string(),
        Value::String(execution.model_provider.clone()),
    );
    inbound_metadata.insert(
        META_MODEL_KEY.to_string(),
        Value::String(execution.model.clone()),
    );
    if let Some(base_session_key) = base_session_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        inbound_metadata.insert(
            "channel.base_session_key".to_string(),
            Value::String(base_session_key.to_string()),
        );
    }
    if let Some(route) = delivery_route.as_ref() {
        inbound_metadata.insert(
            "channel.delivery_session_key".to_string(),
            Value::String(route.session_key.clone()),
        );
        inbound_metadata.insert(
            "webhook.delivery_session_key".to_string(),
            Value::String(route.session_key.clone()),
        );
        inbound_metadata.insert(
            "webhook.delivery_channel".to_string(),
            Value::String(route.channel.clone()),
        );
        inbound_metadata.insert(
            "webhook.delivery_chat_id".to_string(),
            Value::String(route.chat_id.clone()),
        );
    }

    let envelope = Envelope {
        header,
        metadata: BTreeMap::new(),
        payload: InboundMessage {
            channel: "webhook".to_string(),
            sender_id,
            chat_id: execution.chat_id.clone(),
            session_key: execution.session_key.clone(),
            content: input,
            media_references: Vec::new(),
            metadata: inbound_metadata,
        },
    };
    refresh_runtime_system_prompt(runtime).await;
    let outcome = runtime.runtime.process_message(envelope, false).await;
    enqueue_llm_audit_records_from_outcome(runtime, session_state.turn_count, &outcome);

    let Some(mut msg) = outcome.final_response else {
        return Ok(None);
    };
    if !should_emit_outbound(&msg) {
        return Ok(None);
    }

    if let Some(route) = delivery_route.as_ref() {
        msg.payload.channel = route.channel.clone();
        msg.payload.chat_id = route.chat_id.clone();
        msg.payload.metadata.extend(route.metadata.clone());
        runtime
            .outbound_transport
            .publish(MessageTopic::Outbound.as_str(), msg.clone())
            .await?;
    }

    let agent_record = ChatRecord::new("assistant", msg.payload.content.clone(), None);
    persist_assistant_response_state(
        runtime,
        &msg.header.session_key,
        &execution.chat_id,
        "webhook",
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
        content: msg.payload.content,
        reasoning,
        metadata: msg.payload.metadata,
    }))
}

pub async fn submit_webhook_event(
    runtime: &RuntimeBundle,
    request: &GatewayWebhookRequest,
) -> Result<Option<AssistantOutput>, String> {
    let (default_provider_id, default_model) = runtime_default_route(runtime);
    let delivery_route =
        resolve_webhook_delivery_route(runtime, request.base_session_key.as_deref()).await?;
    let execution = if let Some(route) = delivery_route.as_ref() {
        build_webhook_execution_route(
            &request.session_key,
            &request.chat_id,
            &route.model_provider,
            &route.model,
        )
    } else {
        build_webhook_execution_route(
            &request.session_key,
            &request.chat_id,
            &default_provider_id,
            &default_model,
        )
    };
    submit_webhook_isolated_turn(
        runtime,
        request.content.clone(),
        request.sender_id.clone(),
        execution,
        delivery_route,
        request.base_session_key.as_deref(),
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
    let (default_provider_id, default_model) = runtime_default_route(runtime);
    let delivery_route =
        resolve_webhook_delivery_route(runtime, request.base_session_key.as_deref()).await?;
    let inherited_provider = delivery_route
        .as_ref()
        .map(|route| route.model_provider.as_str())
        .unwrap_or(default_provider_id.as_str());
    let inherited_model = delivery_route
        .as_ref()
        .map(|route| route.model.as_str())
        .unwrap_or(default_model.as_str());
    let (model_provider, model) = resolve_webhook_agent_model(
        runtime,
        inherited_provider,
        inherited_model,
        request.provider.as_deref(),
        request.model.as_deref(),
    )?;
    let execution = WebhookExecutionRoute {
        session_key: request.session_key.clone(),
        chat_id: request.chat_id.clone(),
        model_provider: model_provider.clone(),
        model: model.clone(),
    };
    let mut metadata = request.metadata.clone();
    metadata.insert(
        "webhook.agents.execution_session_key".to_string(),
        Value::String(request.session_key.clone()),
    );
    if let Some(base_session_key) = request.base_session_key.as_ref() {
        metadata.insert(
            "webhook.agents.base_session_key".to_string(),
            Value::String(base_session_key.clone()),
        );
    }
    if let Some(route) = delivery_route.as_ref() {
        metadata.insert(
            "webhook.agents.delivery_session_key".to_string(),
            Value::String(route.session_key.clone()),
        );
    }
    metadata.insert(
        "webhook.agents.provider".to_string(),
        Value::String(model_provider.clone()),
    );
    metadata.insert(
        "webhook.agents.model".to_string(),
        Value::String(model.clone()),
    );
    submit_webhook_isolated_turn(
        runtime,
        content,
        request.sender_id.clone(),
        execution,
        delivery_route,
        request.base_session_key.as_deref(),
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
    let provider_runtime = provider_runtime_snapshot(runtime);
    if !provider_runtime
        .provider_default_models
        .contains_key(&provider)
    {
        let all = provider_runtime
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
            default_model_for_provider(runtime, &provider)
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
    persist_session_delivery_metadata(
        &sessions,
        &session_key,
        &chat_id,
        &channel,
        &request_metadata,
    )
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
    info!(
        channel = %inbound_payload.channel,
        chat_id = %inbound_payload.chat_id,
        session_key = %inbound_payload.session_key,
        "channel inbound normalized (streaming)"
    );
    trace!(
        inbound = ?inbound_payload,
        "channel inbound normalized (streaming)"
    );

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
                        writer
                            .write(ChannelStreamEvent::Snapshot(channel_response(
                                content,
                                reasoning,
                                BTreeMap::new(),
                            )))
                            .await?;
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
                .write(ChannelStreamEvent::Snapshot(channel_response(
                    output.content.clone(),
                    output.reasoning.clone(),
                    output.metadata.clone(),
                )))
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
    refresh_runtime_system_prompt(runtime).await;
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
            tool_audits: Vec::new(),
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
        .model_providers
        .get(provider_id)
        .map(|provider| provider.default_model.clone())
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
    let mcp_summary = {
        let mut guard = runtime.mcp_init.lock().await;
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
    };
    {
        let mut guard = runtime.acp_init.lock().await;
        if guard.is_ready() {
            let _ = guard
                .wait_until_ready()
                .await
                .map_err(|err| config_err(format!("acp init failed: {err}")))?;
        }
    }

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
        RuntimeBundle, StartupReport, approval_manager, ask_question_manager,
        build_approved_shell_followup_request_metadata,
        build_ask_question_followup_request_metadata, build_history_for_model,
        build_new_session_bootstrap_user_message, build_unavailable_provider, builtin_tool_names,
        compression_trigger_interval, configured_default_model, extract_skill_short_description,
        format_approve_already_handled_message, format_new_session_started_message, im_commands,
        normalize_runtime_provider_override, parse_outbound_attachments, resolve_session_route,
        resolve_webhook_agent_model, should_emit_outbound, should_trigger_compression,
        shutdown_runtime_bundle, spawn_acp_init, spawn_llm_audit_writer, spawn_mcp_init,
        submit_and_get_output, submit_webhook_agent, submit_webhook_event, sync_runtime_providers,
        sync_runtime_tools, trim_conversation_history, voice_tool_is_enabled,
    };
    use klaw_agent::{AgentExecutionOutput, AgentRequestAudit, ConversationSummary};
    use klaw_approval::{ApprovalCreateInput, ApprovalManager};
    use klaw_channel::OutboundAttachmentSource;
    use klaw_config::{AppConfig, McpConfig, ModelProviderConfig};
    use klaw_core::{
        CircuitBreakerPolicy, DeadLetterPolicy, Envelope, EnvelopeHeader,
        ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker, InMemoryIdempotencyStore,
        InMemoryTransport, OutboundMessage, QueueStrategy, RunLimits, SessionSchedulingPolicy,
        Subscription,
    };
    use klaw_gateway::{GatewayWebhookAgentRequest, GatewayWebhookRequest};
    use klaw_llm::{ChatOptions, LlmAuditPayload, LlmError, LlmProvider};
    use klaw_session::{ChatRecord, SessionManager, SqliteSessionManager};
    use klaw_storage::{ApprovalStatus, HeartbeatStorage};
    use klaw_storage::{DefaultSessionStore, NewLlmUsageRecord, StoragePaths};
    use klaw_tool::{ShellTool, SubAgentAuditSink, ToolRegistry};
    use klaw_util::EnvironmentCheckReport;
    use serde_json::{Value, json};
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
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

    #[test]
    fn parse_outbound_attachments_reads_structured_metadata() {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "channel.attachments".to_string(),
            json!([{
                "archive_id": "arch-1",
                "kind": "image",
                "filename": "chart.png",
                "caption": "latest chart"
            }]),
        );

        let attachments = parse_outbound_attachments(&metadata);

        assert_eq!(attachments.len(), 1);
        assert!(matches!(
            attachments[0].source,
            OutboundAttachmentSource::ArchiveId { ref archive_id } if archive_id == "arch-1"
        ));
        assert_eq!(attachments[0].filename.as_deref(), Some("chart.png"));
    }

    #[test]
    fn parse_outbound_attachments_ignores_invalid_metadata() {
        let metadata = BTreeMap::from([(
            "channel.attachments".to_string(),
            json!([{ "archive_id": "" }]),
        )]);

        let attachments = parse_outbound_attachments(&metadata);

        assert!(attachments.is_empty());
    }

    #[test]
    fn parse_outbound_attachments_reads_local_path_metadata() {
        let metadata = BTreeMap::from([(
            "channel.attachments".to_string(),
            json!([{
                "path": "/tmp/report.pdf",
                "kind": "file",
                "filename": "report.pdf"
            }]),
        )]);

        let attachments = parse_outbound_attachments(&metadata);

        assert_eq!(attachments.len(), 1);
        assert!(matches!(
            attachments[0].source,
            OutboundAttachmentSource::LocalPath { ref path } if path == "/tmp/report.pdf"
        ));
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

    #[test]
    fn compose_system_prompt_appends_memory_section() {
        let composed = super::compose_system_prompt(
            Some("## Base\n\nrules".to_string()),
            Some("## Memory\n\n- fact".to_string()),
        )
        .expect("prompt should compose");

        assert!(composed.contains("## Base"));
        assert!(composed.contains("## Memory"));
        assert!(composed.contains("--------------------------------"));
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
            base_system_prompt: Arc::new(RwLock::new(None)),
            memory_db: None,
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
            mcp_init: spawn_mcp_init(&ToolRegistry::default(), &McpConfig::default()),
            acp_init: spawn_acp_init(&ToolRegistry::default(), &klaw_config::AcpConfig::default()),
            startup_report: StartupReport::default(),
            observability: None,
            conversation_history_limit: 16,
            llm_audit_tx: spawn_llm_audit_writer(session_store.clone()),
            tool_audit_tx: super::spawn_tool_audit_writer(session_store),
            env_check: EnvironmentCheckReport {
                checks: Vec::new(),
                checked_at: OffsetDateTime::UNIX_EPOCH,
            },
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_sub_agent_audit_sink_persists_audits_on_parent_session() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let sessions = super::session_manager(&runtime);
        sessions
            .touch_session("parent-session", "chat-parent", "stdio")
            .await
            .expect("parent session should exist");
        sessions
            .complete_turn("parent-session", "chat-parent", "stdio")
            .await
            .expect("turn count should increment");

        let sink = super::RuntimeSubAgentAuditSink::new(
            runtime.session_store.clone(),
            runtime.llm_audit_tx.clone(),
            runtime.tool_audit_tx.clone(),
        );
        let output = AgentExecutionOutput {
            content: "done".to_string(),
            reasoning: None,
            disposition: klaw_agent::AgentExecutionDisposition::FinalMessage,
            tool_signals: Vec::new(),
            request_usages: Vec::new(),
            request_audits: vec![AgentRequestAudit {
                request_seq: 1,
                payload: LlmAuditPayload {
                    provider: "openai".to_string(),
                    model: "gpt-4.1-mini".to_string(),
                    wire_api: "responses".to_string(),
                    status: klaw_llm::LlmAuditStatus::Success,
                    error_code: None,
                    error_message: None,
                    request_body: json!({"input":"delegate"}),
                    response_body: Some(json!({"output":"done"})),
                    requested_at_ms: 10,
                    responded_at_ms: Some(20),
                    provider_request_id: Some("req-sub".to_string()),
                    provider_response_id: Some("resp-sub".to_string()),
                },
            }],
            tool_audits: vec![klaw_agent::AgentToolAudit {
                request_seq: 1,
                tool_call_seq: 1,
                tool_call_id: Some("call_sub_1".to_string()),
                tool_name: "shell".to_string(),
                arguments: json!({"command":"pwd"}),
                result: klaw_agent::ToolInvocationResult::success("/tmp".to_string()),
                started_at_ms: 30,
                finished_at_ms: 45,
            }],
        };
        sink.persist_sub_agent_audits("parent-session", "parent-session:subagent:test", &output)
            .await
            .expect("sink should persist audits");

        let mut rows = Vec::new();
        for _ in 0..20 {
            rows = sessions
                .list_llm_audit(&klaw_storage::LlmAuditQuery {
                    session_key: Some("parent-session".to_string()),
                    provider: None,
                    requested_from_ms: None,
                    requested_to_ms: None,
                    limit: 10,
                    offset: 0,
                    sort_order: klaw_storage::LlmAuditSortOrder::RequestedAtDesc,
                })
                .await
                .expect("audit rows should list");
            if !rows.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_key, "parent-session");
        assert_eq!(rows[0].chat_id, "chat-parent");
        assert_eq!(rows[0].turn_index, 1);
        let metadata = rows[0]
            .metadata_json
            .as_ref()
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .expect("metadata json should parse");
        assert_eq!(metadata.get("sub_agent"), Some(&Value::Bool(true)));
        assert_eq!(
            metadata.get("sub_agent.parent_session_key"),
            Some(&Value::String("parent-session".to_string()))
        );
        assert_eq!(
            metadata.get("sub_agent.child_session_key"),
            Some(&Value::String("parent-session:subagent:test".to_string()))
        );

        let mut tool_rows = Vec::new();
        for _ in 0..20 {
            tool_rows = sessions
                .list_tool_audit(&klaw_storage::ToolAuditQuery {
                    session_key: Some("parent-session".to_string()),
                    tool_name: Some("shell".to_string()),
                    started_from_ms: None,
                    started_to_ms: None,
                    limit: 10,
                    offset: 0,
                    sort_order: klaw_storage::ToolAuditSortOrder::StartedAtDesc,
                })
                .await
                .expect("tool audit rows should list");
            if !tool_rows.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert_eq!(tool_rows.len(), 1);
        assert_eq!(tool_rows[0].tool_name, "shell");
        assert_eq!(tool_rows[0].request_seq, 1);
        assert_eq!(tool_rows[0].tool_call_seq, 1);
        let tool_metadata = tool_rows[0]
            .metadata_json
            .as_ref()
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .expect("tool metadata json should parse");
        assert_eq!(
            tool_metadata.get("tool_call_id"),
            Some(&Value::String("call_sub_1".to_string()))
        );
    }

    fn test_provider_config(default_model: &str) -> ModelProviderConfig {
        ModelProviderConfig {
            default_model: default_model.to_string(),
            api_key: Some("test-key".to_string()),
            env_key: None,
            ..ModelProviderConfig::default()
        }
    }

    fn disable_all_tools(config: &mut AppConfig) {
        config.tools.archive.enabled = false;
        config.tools.channel_attachment.enabled = false;
        config.tools.voice.enabled = false;
        config.tools.apply_patch.enabled = false;
        config.tools.shell.enabled = false;
        config.tools.approval.enabled = false;
        config.tools.geo.enabled = false;
        config.tools.local_search.enabled = false;
        config.tools.terminal_multiplexers.enabled = false;
        config.tools.cron_manager.enabled = false;
        config.tools.heartbeat_manager.enabled = false;
        config.tools.skills_registry.enabled = false;
        config.tools.skills_manager.enabled = false;
        config.tools.memory.enabled = false;
        config.tools.web_fetch.enabled = false;
        config.tools.web_search.enabled = false;
        config.tools.sub_agent.enabled = false;
    }

    #[tokio::test]
    async fn spawn_mcp_init_creates_manager_for_empty_config() {
        let handle = spawn_mcp_init(&ToolRegistry::default(), &McpConfig::default());
        let mut guard = handle.lock().await;
        let summary = guard
            .wait_until_ready()
            .await
            .expect("empty mcp init should succeed");
        assert!(summary.active_servers.is_empty());
        assert!(summary.statuses.is_empty());
        assert_eq!(summary.tool_count, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_runtime_bundle_returns_when_acp_manager_is_busy() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;
        let acp_manager = {
            let guard = runtime.acp_init.lock().await;
            guard.manager()
        };
        let _busy_guard = acp_manager.lock().await;

        let shutdown_result =
            tokio::time::timeout(Duration::from_secs(5), shutdown_runtime_bundle(&runtime)).await;

        assert!(
            shutdown_result.is_ok(),
            "shutdown should not hang while acp manager is busy"
        );
        assert!(
            shutdown_result
                .expect("shutdown future should complete")
                .is_ok(),
            "shutdown should continue even when acp manager lock is unavailable"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sync_runtime_tools_reloads_builtins() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;

        let mut config = AppConfig::default();
        disable_all_tools(&mut config);
        config.tools.channel_attachment.enabled = true;
        config.tools.geo.enabled = true;
        config.tools.sub_agent.enabled = true;

        let tool_names = sync_runtime_tools(&runtime, &config)
            .await
            .expect("tool sync should succeed");
        assert!(tool_names.iter().any(|name| name == "channel_attachment"));
        assert!(tool_names.iter().any(|name| name == "geo"));
        assert!(tool_names.iter().any(|name| name == "sub_agent"));
        assert!(!tool_names.iter().any(|name| name == "voice"));

        config.tools.geo.enabled = false;
        config.tools.local_search.enabled = true;

        let tool_names = sync_runtime_tools(&runtime, &config)
            .await
            .expect("tool resync should succeed");
        assert!(!tool_names.iter().any(|name| name == "geo"));
        assert!(tool_names.iter().any(|name| name == "local_search"));
        assert!(tool_names.iter().any(|name| name == "sub_agent"));
    }

    #[test]
    fn voice_tool_requires_voice_and_tool_flags() {
        let mut config = AppConfig::default();
        assert!(!voice_tool_is_enabled(&config));

        config.tools.voice.enabled = true;
        assert!(!voice_tool_is_enabled(&config));

        config.voice.enabled = true;
        assert!(voice_tool_is_enabled(&config));

        config.tools.voice.enabled = false;
        assert!(!voice_tool_is_enabled(&config));
    }

    #[test]
    fn builtin_tool_names_follow_tools_config_keys() {
        let config = AppConfig::default();
        let names = builtin_tool_names(&config);

        assert!(names.iter().any(|name| name == "voice"));
        assert!(names.iter().any(|name| name == "channel_attachment"));
        assert!(names.iter().any(|name| name == "shell"));
        assert!(names.iter().any(|name| name == "sub_agent"));
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
        assert_eq!(im_commands::parse_im_command("/help"), Some(("help", None)));
        assert_eq!(im_commands::parse_im_command("/stop"), Some(("stop", None)));
        assert_eq!(
            im_commands::parse_im_command("/model_provider openai"),
            Some(("model_provider", Some("openai")))
        );
        assert_eq!(
            im_commands::parse_im_command("/model qwen-plus /help"),
            Some(("model", Some("qwen-plus /help")))
        );
        assert_eq!(im_commands::parse_im_command("hello"), None);
    }

    #[test]
    fn new_session_bootstrap_user_message_mentions_bootstrap_or_greeting() {
        let message = build_new_session_bootstrap_user_message();
        assert!(message.contains("You just woke up"));
        assert!(message.contains("If `BOOTSTRAP.md` exists, use the available workspace tools"));
        assert!(message.contains("start with a short, natural greeting"));
        assert!(message.contains("do not recreate or restore `BOOTSTRAP.md`"));
        assert!(message.contains("If `BOOTSTRAP.md` is already absent, leave it absent"));
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
        assert_eq!(
            im_commands::first_arg_token(Some("openai /help")),
            Some("openai")
        );
        assert_eq!(
            im_commands::first_arg_token(Some("qwen-plus extra words")),
            Some("qwen-plus")
        );
        assert_eq!(im_commands::first_arg_token(Some("   ")), None);
    }

    #[test]
    fn second_arg_token_reads_second_token_only() {
        assert_eq!(
            im_commands::second_arg_token(Some("q1 option-a trailing")),
            Some("option-a")
        );
        assert_eq!(im_commands::second_arg_token(Some("single")), None);
        assert_eq!(im_commands::second_arg_token(Some("   ")), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn usage_command_reports_latest_turn_and_session_totals() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        let session_key = "stdio:usage".to_string();
        let chat_id = "chat-usage".to_string();

        sessions
            .get_or_create_session_state(
                &session_key,
                &chat_id,
                "stdio",
                "test-provider",
                "test-model",
            )
            .await
            .expect("session should exist");

        for record in [
            NewLlmUsageRecord {
                id: "usage-1".to_string(),
                session_key: session_key.clone(),
                chat_id: chat_id.clone(),
                turn_index: 1,
                request_seq: 1,
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                wire_api: "responses".to_string(),
                input_tokens: 3,
                output_tokens: 2,
                total_tokens: 5,
                cached_input_tokens: None,
                reasoning_tokens: None,
                source: klaw_storage::LlmUsageSource::ProviderReported,
                provider_request_id: None,
                provider_response_id: None,
            },
            NewLlmUsageRecord {
                id: "usage-2".to_string(),
                session_key: session_key.clone(),
                chat_id: chat_id.clone(),
                turn_index: 1,
                request_seq: 2,
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                wire_api: "responses".to_string(),
                input_tokens: 4,
                output_tokens: 3,
                total_tokens: 7,
                cached_input_tokens: Some(2),
                reasoning_tokens: None,
                source: klaw_storage::LlmUsageSource::ProviderReported,
                provider_request_id: None,
                provider_response_id: None,
            },
            NewLlmUsageRecord {
                id: "usage-3".to_string(),
                session_key: session_key.clone(),
                chat_id: chat_id.clone(),
                turn_index: 2,
                request_seq: 1,
                provider: "test-provider".to_string(),
                model: "test-model".to_string(),
                wire_api: "responses".to_string(),
                input_tokens: 6,
                output_tokens: 5,
                total_tokens: 11,
                cached_input_tokens: None,
                reasoning_tokens: Some(4),
                source: klaw_storage::LlmUsageSource::ProviderReported,
                provider_request_id: None,
                provider_response_id: None,
            },
        ] {
            sessions
                .append_llm_usage(&record)
                .await
                .expect("usage record should persist");
        }

        let response = im_commands::handle_im_command(
            &runtime,
            "stdio".to_string(),
            session_key.clone(),
            chat_id,
            "/usage".to_string(),
            BTreeMap::new(),
        )
        .await
        .expect("usage command should succeed")
        .expect("usage command should return a response");

        assert!(response.content.contains("**Latest turn:** #2"));
        assert!(response.content.contains("- total_tokens: `11`"));
        assert!(response.content.contains("**Session total:**"));
        assert!(response.content.contains("- requests: `3`"));
        assert!(response.content.contains("- input_tokens: `13`"));
        assert!(response.content.contains("- output_tokens: `10`"));
        assert!(response.content.contains("- total_tokens: `23`"));
        assert!(response.content.contains("- cached_input_tokens: `2`"));
        assert!(response.content.contains("- reasoning_tokens: `4`"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shell_command_executes_directly_from_shell_workspace() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let mut runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        let session_key = "stdio:shell".to_string();
        let chat_id = "chat-shell".to_string();

        sessions
            .get_or_create_session_state(
                &session_key,
                &chat_id,
                "stdio",
                "test-provider",
                "test-model",
            )
            .await
            .expect("session should exist");

        let workspace = std::env::temp_dir().join(format!("klaw-shell-command-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).expect("workspace should be created");
        let workspace = fs::canonicalize(workspace).expect("workspace should canonicalize");

        let mut config = AppConfig::default();
        config.tools.shell.workspace = Some(workspace.display().to_string());
        config.tools.shell.unsafe_patterns = vec!["echo risky".to_string()];
        runtime.runtime.tools.register(ShellTool::with_store(
            &config,
            runtime.session_store.clone(),
        ));

        let response = im_commands::handle_im_command(
            &runtime,
            "stdio".to_string(),
            session_key,
            chat_id,
            "/shell echo risky".to_string(),
            BTreeMap::new(),
        )
        .await
        .expect("shell command should succeed")
        .expect("shell command should return a response");

        assert!(response.content.contains("✅ **Shell command succeeded**"));
        assert!(response.content.contains("- Command: `echo risky`"));
        assert!(response.content.contains(&format!("- CWD: `{}`", workspace.display())));
        assert!(response.content.contains("- Exit code: `0`"));
        assert!(response.content.contains("````text\nrisky\n````"));
        assert!(!response.content.contains("\"stdout\": \"risky\""));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shell_command_does_not_bypass_blocked_patterns() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let mut runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        let session_key = "stdio:shell-blocked".to_string();
        let chat_id = "chat-shell-blocked".to_string();

        sessions
            .get_or_create_session_state(
                &session_key,
                &chat_id,
                "stdio",
                "test-provider",
                "test-model",
            )
            .await
            .expect("session should exist");

        let workspace =
            std::env::temp_dir().join(format!("klaw-shell-command-blocked-{}", Uuid::new_v4()));
        fs::create_dir_all(&workspace).expect("workspace should be created");
        let workspace = fs::canonicalize(workspace).expect("workspace should canonicalize");

        let mut config = AppConfig::default();
        config.tools.shell.workspace = Some(workspace.display().to_string());
        config.tools.shell.blocked_patterns = vec!["echo blocked".to_string()];
        runtime.runtime.tools.register(ShellTool::with_store(
            &config,
            runtime.session_store.clone(),
        ));

        let response = im_commands::handle_im_command(
            &runtime,
            "stdio".to_string(),
            session_key,
            chat_id,
            "/shell echo blocked".to_string(),
            BTreeMap::new(),
        )
        .await
        .expect("shell command should succeed")
        .expect("shell command should return a response");

        assert!(response.content.contains("security violation"));
        assert!(response.content.contains("tool `shell` failed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn help_command_lists_usage_and_shell_commands() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;
        let mut config = AppConfig::default();
        config.model_provider = "fresh".to_string();
        config.model_providers = BTreeMap::from([
            ("backup".to_string(), test_provider_config("backup-model")),
            ("fresh".to_string(), test_provider_config("fresh-model")),
        ]);
        sync_runtime_providers(&runtime, &config).expect("provider sync should succeed");

        let response = im_commands::handle_im_command(
            &runtime,
            "stdio".to_string(),
            "stdio:help".to_string(),
            "chat-help".to_string(),
            "/help".to_string(),
            BTreeMap::new(),
        )
        .await
        .expect("help command should succeed")
        .expect("help command should return a response");

        assert!(response.content.contains("/usage"));
        assert!(response.content.contains("/shell <command>"));
        assert!(!response.content.contains("🧩 Providers:"));
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

        assert_eq!(config.model_provider, "anthropic");
        assert_eq!(
            configured_default_model(&config, &config.model_provider),
            "claude-sonnet-4-5"
        );
    }

    #[test]
    fn resolve_new_session_target_uses_provider_default_model_even_with_root_model() {
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

        assert_eq!(config.model_provider, "anthropic");
        assert_eq!(
            configured_default_model(&config, &config.model_provider),
            "claude-sonnet-4-5"
        );
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

    #[tokio::test(flavor = "current_thread")]
    async fn sync_runtime_providers_refreshes_live_snapshot_and_clears_invalid_override() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;
        *runtime
            .runtime_provider_override
            .write()
            .unwrap_or_else(|err| err.into_inner()) = Some("missing-provider".to_string());

        let mut config = AppConfig::default();
        config.model_provider = "fresh".to_string();
        config.model = None;
        config.model_providers = BTreeMap::from([
            ("backup".to_string(), test_provider_config("backup-model")),
            ("fresh".to_string(), test_provider_config("fresh-model")),
        ]);

        let (active_provider, active_model) =
            sync_runtime_providers(&runtime, &config).expect("provider sync should succeed");
        let provider_runtime = runtime.runtime.provider_runtime_snapshot();

        assert_eq!(active_provider, "fresh");
        assert_eq!(active_model, "fresh-model");
        assert_eq!(provider_runtime.default_provider_id, "fresh");
        assert_eq!(provider_runtime.default_model, "fresh-model");
        assert_eq!(
            provider_runtime.provider_default_models.get("backup"),
            Some(&"backup-model".to_string())
        );
        assert_eq!(
            runtime
                .runtime_provider_override
                .read()
                .unwrap_or_else(|err| err.into_inner())
                .clone(),
            None
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sync_runtime_providers_ignores_legacy_root_model_field() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;

        let mut config = AppConfig::default();
        config.model_provider = "fresh".to_string();
        config.model = Some("root-override-should-be-ignored".to_string());
        config.model_providers = BTreeMap::from([
            ("backup".to_string(), test_provider_config("backup-model")),
            ("fresh".to_string(), test_provider_config("fresh-model")),
        ]);

        let (active_provider, active_model) =
            sync_runtime_providers(&runtime, &config).expect("provider sync should succeed");
        let provider_runtime = runtime.runtime.provider_runtime_snapshot();

        assert_eq!(active_provider, "fresh");
        assert_eq!(active_model, "fresh-model");
        assert_eq!(provider_runtime.default_provider_id, "fresh");
        assert_eq!(provider_runtime.default_model, "fresh-model");
        assert_eq!(
            provider_runtime.provider_default_models.get("fresh"),
            Some(&"fresh-model".to_string())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn model_provider_command_lists_live_runtime_providers_after_sync() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;

        let mut config = AppConfig::default();
        config.model_provider = "fresh".to_string();
        config.model_providers = BTreeMap::from([
            ("backup".to_string(), test_provider_config("backup-model")),
            ("fresh".to_string(), test_provider_config("fresh-model")),
        ]);
        sync_runtime_providers(&runtime, &config).expect("provider sync should succeed");

        let response = im_commands::handle_im_command(
            &runtime,
            "stdio".to_string(),
            "stdio:test".to_string(),
            "chat-1".to_string(),
            "/model_provider".to_string(),
            BTreeMap::new(),
        )
        .await
        .expect("command should succeed")
        .expect("command should produce a response");

        assert!(response.content.contains("fresh"));
        assert!(response.content.contains("backup"));
        assert!(!response.content.contains("test-provider"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_session_route_clears_invalid_provider_override_after_sync() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "stdio:test",
                "chat-1",
                "stdio",
                "test-provider",
                "test-model",
            )
            .await
            .expect("session should exist");
        sessions
            .set_model_provider(
                "stdio:test",
                "chat-1",
                "stdio",
                "legacy-provider",
                "legacy-model",
            )
            .await
            .expect("override should persist");

        let mut config = AppConfig::default();
        config.model_provider = "fresh".to_string();
        config.model_providers =
            BTreeMap::from([("fresh".to_string(), test_provider_config("fresh-model"))]);
        sync_runtime_providers(&runtime, &config).expect("provider sync should succeed");

        let route = resolve_session_route(&runtime, "stdio", "stdio:test", "chat-1")
            .await
            .expect("route should resolve");

        assert_eq!(route.model_provider, "fresh");
        assert_eq!(route.model, "fresh-model");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_session_route_preserves_explicit_provider_override_after_global_provider_change()
     {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "stdio:test",
                "chat-1",
                "stdio",
                "test-provider",
                "test-model",
            )
            .await
            .expect("session should exist");
        sessions
            .set_model_provider(
                "stdio:test",
                "chat-1",
                "stdio",
                "explicit-provider",
                "explicit-model",
            )
            .await
            .expect("explicit provider override should persist");

        let mut config = AppConfig::default();
        config.model_provider = "fresh".to_string();
        config.model_providers = BTreeMap::from([
            ("fresh".to_string(), test_provider_config("fresh-model")),
            (
                "explicit-provider".to_string(),
                test_provider_config("explicit-model"),
            ),
        ]);
        sync_runtime_providers(&runtime, &config).expect("provider sync should succeed");

        let route = resolve_session_route(&runtime, "stdio", "stdio:test", "chat-1")
            .await
            .expect("route should resolve");

        assert_eq!(route.model_provider, "explicit-provider");
        assert_eq!(route.model, "explicit-model");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_session_route_clears_legacy_model_only_override_after_global_provider_change()
    {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "stdio:test",
                "chat-1",
                "stdio",
                "test-provider",
                "test-model",
            )
            .await
            .expect("session should exist");
        sessions
            .set_model("stdio:test", "chat-1", "stdio", "legacy-model")
            .await
            .expect("legacy model-only override should persist");

        let mut config = AppConfig::default();
        config.model_provider = "fresh".to_string();
        config.model_providers =
            BTreeMap::from([("fresh".to_string(), test_provider_config("fresh-model"))]);
        sync_runtime_providers(&runtime, &config).expect("provider sync should succeed");

        let route = resolve_session_route(&runtime, "stdio", "stdio:test", "chat-1")
            .await
            .expect("route should resolve");

        assert_eq!(route.model_provider, "fresh");
        assert_eq!(route.model, "fresh-model");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_session_route_syncs_heartbeat_for_supported_channels() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;

        resolve_session_route(&runtime, "telegram", "telegram:test", "chat-1")
            .await
            .expect("route should resolve");

        let heartbeat = runtime
            .session_store
            .get_heartbeat_by_session_key("telegram:test")
            .await
            .expect("heartbeat should be created");
        assert_eq!(heartbeat.channel, "telegram");
        assert_eq!(heartbeat.chat_id, "chat-1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolve_session_route_skips_heartbeat_for_unsupported_channels() {
        let provider = Arc::new(BootstrapCaptureProvider::default()) as Arc<dyn LlmProvider>;
        let runtime = build_test_runtime(provider).await;

        resolve_session_route(&runtime, "stdio", "stdio:test", "chat-1")
            .await
            .expect("route should resolve");

        let err = runtime
            .session_store
            .get_heartbeat_by_session_key("stdio:test")
            .await
            .expect_err("heartbeat should not be created");
        assert!(format!("{err}").contains("not found"));
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
    fn configured_default_model_uses_provider_default_model() {
        let mut config = AppConfig::default();
        config
            .model_providers
            .get_mut("openai")
            .expect("openai provider should exist")
            .default_model = "gpt-4.1".to_string();
        config.model = Some("root-override-should-be-ignored".to_string());

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

        let response = im_commands::handle_im_command(
            &runtime,
            channel.clone(),
            base_session_key.clone(),
            chat_id.clone(),
            "/new".to_string(),
            BTreeMap::new(),
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
    async fn start_command_writes_bootstrap_user_message_into_new_session() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider.clone()).await;
        let channel = "telegram".to_string();
        let base_session_key = "telegram:chat-start".to_string();
        let chat_id = "chat-start".to_string();
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

        let response = im_commands::handle_im_command(
            &runtime,
            channel.clone(),
            base_session_key.clone(),
            chat_id.clone(),
            "/start".to_string(),
            BTreeMap::new(),
        )
        .await
        .expect("start command should succeed")
        .expect("start command should return a response");

        assert!(response.content.contains("New session started"));
        assert!(response.content.contains("bootstrap reply"));

        let route = resolve_session_route(&runtime, &channel, &base_session_key, &chat_id)
            .await
            .expect("start command route should resolve");
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
    }

    #[tokio::test(flavor = "current_thread")]
    async fn new_command_copies_dingtalk_delivery_metadata_to_new_session() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let channel = "dingtalk".to_string();
        let base_session_key = "dingtalk:acc:chat-1".to_string();
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

        let response = im_commands::handle_im_command(
            &runtime,
            channel.clone(),
            base_session_key.clone(),
            chat_id.clone(),
            "/new".to_string(),
            BTreeMap::from([
                (
                    "channel.dingtalk.session_webhook".to_string(),
                    json!("https://example/session-new"),
                ),
                ("channel.dingtalk.bot_title".to_string(), json!("Klaw")),
                ("channel.delivery_mode".to_string(), json!("direct_reply")),
            ]),
        )
        .await
        .expect("new command should succeed")
        .expect("new command should return a response");
        assert!(response.content.contains("New session started"));

        let route = resolve_session_route(&runtime, &channel, &base_session_key, &chat_id)
            .await
            .expect("new session route should resolve");
        let child = sessions
            .get_session(&route.active_session_key)
            .await
            .expect("child session should reload");
        assert_eq!(
            child.delivery_metadata_json.as_deref(),
            Some(
                "{\"channel.dingtalk.bot_title\":\"Klaw\",\"channel.dingtalk.session_webhook\":\"https://example/session-new\"}",
            )
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stop_command_returns_stopped_response_without_running_agent() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider.clone()).await;
        let channel = "telegram".to_string();
        let base_session_key = "telegram:chat-stop".to_string();
        let chat_id = "chat-stop".to_string();

        let response = im_commands::handle_im_command(
            &runtime,
            channel,
            base_session_key,
            chat_id,
            "/stop".to_string(),
            BTreeMap::new(),
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

    #[test]
    fn approved_shell_followup_metadata_inherits_channel_state_and_limits_retries() {
        let metadata = build_approved_shell_followup_request_metadata(&BTreeMap::from([
            ("channel.delivery_mode".to_string(), json!("direct_reply")),
            (
                "channel.base_session_key".to_string(),
                json!("telegram:base:chat-1"),
            ),
            ("other".to_string(), json!("ignored")),
        ]));

        assert_eq!(
            metadata.get("channel.delivery_mode"),
            Some(&json!("direct_reply"))
        );
        assert_eq!(
            metadata.get("channel.base_session_key"),
            Some(&json!("telegram:base:chat-1"))
        );
        assert_eq!(metadata.get("agent.max_tool_iterations"), Some(&json!(2)));
        assert_eq!(metadata.get("agent.max_tool_calls"), Some(&json!(1)));
        assert!(!metadata.contains_key("other"));
    }

    #[test]
    fn ask_question_followup_metadata_inherits_channel_state_and_structured_answer() {
        let metadata = build_ask_question_followup_request_metadata(
            &BTreeMap::from([
                ("channel.delivery_mode".to_string(), json!("direct_reply")),
                ("other".to_string(), json!("ignored")),
            ]),
            "question-1",
            "Which provider should I use?",
            "openai",
            "OpenAI",
        );
        assert_eq!(
            metadata.get("channel.delivery_mode"),
            Some(&json!("direct_reply"))
        );
        assert_eq!(
            metadata.get("ask_question.answer"),
            Some(&json!({
                "question_id": "question-1",
                "question": "Which provider should I use?",
                "selected_option_id": "openai",
                "selected_option_label": "OpenAI",
            }))
        );
        assert!(!metadata.contains_key("other"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn card_answer_command_resumes_session_with_selected_option() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider.clone()).await;
        let channel = "telegram".to_string();
        let session_key = "telegram:chat-ask".to_string();
        let chat_id = "chat-ask".to_string();
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                &session_key,
                &chat_id,
                &channel,
                "test-provider",
                "test-model",
            )
            .await
            .expect("session should exist");

        let manager = ask_question_manager(&runtime);
        let question = manager
            .create_question(
                &session_key,
                Some("Choose provider".to_string()),
                "Which provider should I use?".to_string(),
                vec![
                    klaw_tool::ask_question::AskQuestionOption {
                        id: "openai".to_string(),
                        label: "OpenAI".to_string(),
                    },
                    klaw_tool::ask_question::AskQuestionOption {
                        id: "anthropic".to_string(),
                        label: "Anthropic".to_string(),
                    },
                ],
                Some(5),
            )
            .await
            .expect("question should be created");

        let response = im_commands::handle_im_command(
            &runtime,
            channel,
            session_key.clone(),
            chat_id,
            format!("/card_answer {} anthropic", question.id),
            BTreeMap::from([("channel.delivery_mode".to_string(), json!("direct_reply"))]),
        )
        .await
        .expect("card_answer should succeed")
        .expect("card_answer should return a response");

        assert_eq!(response.content, "bootstrap reply");
        assert_eq!(
            response.metadata.get("channel.delivery_mode"),
            Some(&json!("direct_reply"))
        );
        let captured = provider
            .last_user_message
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        let captured = captured.expect("follow-up user message should be captured");
        assert!(captured.contains("The user answered a pending ask_question prompt."));
        assert!(captured.contains("Selected option ID: anthropic"));
        assert!(captured.contains("Selected option label: Anthropic"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn approve_command_preserves_direct_reply_metadata_for_followup() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let channel = "telegram".to_string();
        let base_session_key = "telegram:tg7:chat-approval".to_string();
        let chat_id = "chat-approval".to_string();
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

        let manager = approval_manager(&runtime);
        let approval = manager
            .create_approval(ApprovalCreateInput {
                session_key: base_session_key.clone(),
                tool_name: "shell".to_string(),
                command_text: "gh issue list --state open".to_string(),
                command_preview: Some("gh issue list --state open".to_string()),
                command_hash: Some("command-hash-1".to_string()),
                risk_level: Some("unsafe".to_string()),
                requested_by: Some("agent".to_string()),
                justification: None,
                expires_in_minutes: Some(10),
            })
            .await
            .expect("approval should be created");

        let response = im_commands::handle_im_command(
            &runtime,
            channel,
            base_session_key,
            chat_id,
            format!("/approve {}", approval.id),
            BTreeMap::from([("channel.delivery_mode".to_string(), json!("direct_reply"))]),
        )
        .await
        .expect("approve command should succeed")
        .expect("approve command should return a response");

        assert_eq!(response.content, "bootstrap reply");
        assert_eq!(
            response.metadata.get("channel.delivery_mode"),
            Some(&json!("direct_reply"))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn submit_and_get_output_persists_dingtalk_delivery_metadata_on_active_session() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "dingtalk:acc:chat-1",
                "chat-1",
                "dingtalk",
                "test-provider",
                "test-model",
            )
            .await
            .expect("base session should exist");
        sessions
            .get_or_create_session_state(
                "dingtalk:acc:chat-1:child",
                "chat-1",
                "dingtalk",
                "test-provider",
                "test-model",
            )
            .await
            .expect("child session should exist");
        sessions
            .set_active_session(
                "dingtalk:acc:chat-1",
                "chat-1",
                "dingtalk",
                "dingtalk:acc:chat-1:child",
            )
            .await
            .expect("active session should switch");

        let output = submit_and_get_output(
            &runtime,
            "dingtalk".to_string(),
            "hello".to_string(),
            "dingtalk:acc:chat-1:child".to_string(),
            "chat-1".to_string(),
            "local-user".to_string(),
            "test-provider".to_string(),
            "test-model".to_string(),
            Vec::new(),
            BTreeMap::from([
                (
                    "channel.dingtalk.session_webhook".to_string(),
                    json!("https://example/session-latest"),
                ),
                ("channel.dingtalk.bot_title".to_string(), json!("Klaw")),
                ("channel.delivery_mode".to_string(), json!("direct_reply")),
            ]),
        )
        .await
        .expect("submit should succeed");
        assert!(output.is_some());

        let updated = sessions
            .get_session("dingtalk:acc:chat-1:child")
            .await
            .expect("active session should reload");
        assert_eq!(
            updated.delivery_metadata_json.as_deref(),
            Some(
                "{\"channel.dingtalk.bot_title\":\"Klaw\",\"channel.dingtalk.session_webhook\":\"https://example/session-latest\"}",
            )
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn webhook_agent_model_override_uses_requested_provider_default_model() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let mut provider_runtime = runtime.runtime.provider_runtime_snapshot();
        provider_runtime
            .provider_default_models
            .insert("alt-provider".to_string(), "alt-default-model".to_string());
        runtime
            .runtime
            .set_provider_runtime_snapshot(provider_runtime);

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

    #[tokio::test(flavor = "current_thread")]
    async fn submit_webhook_event_routes_output_to_active_delivery_session() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "dingtalk:acc:chat-1",
                "chat-1",
                "dingtalk",
                "test-provider",
                "test-model",
            )
            .await
            .expect("base session should exist");
        sessions
            .get_or_create_session_state(
                "dingtalk:acc:chat-1:active",
                "chat-1",
                "dingtalk",
                "test-provider",
                "test-model",
            )
            .await
            .expect("active session should exist");
        sessions
            .set_active_session(
                "dingtalk:acc:chat-1",
                "chat-1",
                "dingtalk",
                "dingtalk:acc:chat-1:active",
            )
            .await
            .expect("active session should switch");
        sessions
            .set_delivery_metadata(
                "dingtalk:acc:chat-1:active",
                "chat-1",
                "dingtalk",
                Some(
                    "{\"channel.dingtalk.bot_title\":\"Klaw\",\"channel.dingtalk.session_webhook\":\"https://example/session-active\"}",
                ),
            )
            .await
            .expect("delivery metadata should persist");

        let output = submit_webhook_event(
            &runtime,
            &GatewayWebhookRequest {
                event_id: "evt-1".to_string(),
                source: "github".to_string(),
                event_type: "push".to_string(),
                content: "run webhook".to_string(),
                session_key: "webhook:github:req-1".to_string(),
                base_session_key: Some("dingtalk:acc:chat-1".to_string()),
                chat_id: "webhook:github:req-1".to_string(),
                sender_id: "github:webhook".to_string(),
                payload: None,
                metadata: BTreeMap::new(),
                remote_addr: None,
                received_at_ms: 1,
            },
        )
        .await
        .expect("webhook event should succeed")
        .expect("webhook event should produce output");

        assert_eq!(output.content, "bootstrap reply");
        let webhook_history = sessions
            .read_chat_records("webhook:github:req-1")
            .await
            .expect("webhook session history should load");
        assert_eq!(webhook_history.len(), 2);

        let base_history = sessions
            .read_chat_records("dingtalk:acc:chat-1")
            .await
            .expect("base session history should load");
        assert!(base_history.is_empty());
        let active_history = sessions
            .read_chat_records("dingtalk:acc:chat-1:active")
            .await
            .expect("active session history should load");
        assert!(active_history.is_empty());

        let outbound = runtime.outbound_transport.published_messages().await;
        assert_eq!(outbound.len(), 1);
        assert_eq!(outbound[0].header.session_key, "webhook:github:req-1");
        assert_eq!(outbound[0].payload.channel, "dingtalk");
        assert_eq!(outbound[0].payload.chat_id, "chat-1");
        assert_eq!(
            outbound[0]
                .payload
                .metadata
                .get("channel.dingtalk.session_webhook"),
            Some(&json!("https://example/session-active"))
        );
        assert_eq!(
            outbound[0].payload.metadata.get("channel.base_session_key"),
            Some(&json!("dingtalk:acc:chat-1"))
        );
        assert_eq!(
            outbound[0]
                .payload
                .metadata
                .get("channel.delivery_session_key"),
            Some(&json!("dingtalk:acc:chat-1:active"))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn submit_webhook_agent_uses_isolated_session_and_legacy_base_alias() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider.clone()).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "telegram:acc:chat-1",
                "chat-1",
                "telegram",
                "test-provider",
                "test-model",
            )
            .await
            .expect("telegram session should exist");

        let execution_session_key = "webhook:order_sync:req-1".to_string();
        let output = submit_webhook_agent(
            &runtime,
            &GatewayWebhookAgentRequest {
                request_id: "req-1".to_string(),
                hook_id: "order_sync".to_string(),
                session_key: execution_session_key.clone(),
                base_session_key: Some("telegram:acc:chat-1".to_string()),
                chat_id: execution_session_key.clone(),
                sender_id: "webhook-agent:order_sync".to_string(),
                provider: None,
                model: None,
                body: json!({"order_id":"A123"}),
                metadata: BTreeMap::new(),
                remote_addr: None,
                received_at_ms: 1,
            },
            "template body".to_string(),
        )
        .await
        .expect("webhook agent should succeed")
        .expect("webhook agent should produce output");

        assert_eq!(output.content, "bootstrap reply");
        let captured = provider
            .last_user_message
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert_eq!(captured.as_deref(), Some("template body"));

        let webhook_history = sessions
            .read_chat_records(&execution_session_key)
            .await
            .expect("webhook history should load");
        assert_eq!(webhook_history.len(), 2);
        let target_history = sessions
            .read_chat_records("telegram:acc:chat-1")
            .await
            .expect("target history should load");
        assert!(target_history.is_empty());

        let outbound = runtime.outbound_transport.published_messages().await;
        assert_eq!(outbound.len(), 1);
        assert_eq!(outbound[0].payload.channel, "telegram");
        assert_eq!(outbound[0].payload.chat_id, "chat-1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn submit_webhook_event_without_delivery_target_keeps_output_local_to_webhook_session() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);

        let output = submit_webhook_event(
            &runtime,
            &GatewayWebhookRequest {
                event_id: "evt-2".to_string(),
                source: "github".to_string(),
                event_type: "push".to_string(),
                content: "local only".to_string(),
                session_key: "webhook:github:req-2".to_string(),
                base_session_key: None,
                chat_id: "webhook:github:req-2".to_string(),
                sender_id: "github:webhook".to_string(),
                payload: None,
                metadata: BTreeMap::new(),
                remote_addr: None,
                received_at_ms: 2,
            },
        )
        .await
        .expect("webhook event should succeed")
        .expect("webhook event should produce output");

        assert_eq!(output.content, "bootstrap reply");
        assert!(
            runtime
                .outbound_transport
                .published_messages()
                .await
                .is_empty()
        );
        let webhook_history = sessions
            .read_chat_records("webhook:github:req-2")
            .await
            .expect("webhook history should load");
        assert_eq!(webhook_history.len(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn submit_webhook_event_fails_when_dingtalk_delivery_metadata_is_missing() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "dingtalk:acc:chat-missing",
                "chat-missing",
                "dingtalk",
                "test-provider",
                "test-model",
            )
            .await
            .expect("dingtalk session should exist");

        let err = submit_webhook_event(
            &runtime,
            &GatewayWebhookRequest {
                event_id: "evt-3".to_string(),
                source: "github".to_string(),
                event_type: "push".to_string(),
                content: "should fail".to_string(),
                session_key: "webhook:github:req-3".to_string(),
                base_session_key: Some("dingtalk:acc:chat-missing".to_string()),
                chat_id: "webhook:github:req-3".to_string(),
                sender_id: "github:webhook".to_string(),
                payload: None,
                metadata: BTreeMap::new(),
                remote_addr: None,
                received_at_ms: 3,
            },
        )
        .await
        .expect_err("missing delivery metadata should fail");

        assert!(err.contains("missing dingtalk session webhook metadata"));
        assert!(
            runtime
                .outbound_transport
                .published_messages()
                .await
                .is_empty()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn submit_webhook_event_rejects_webhook_execution_session_as_base_target() {
        let provider = Arc::new(BootstrapCaptureProvider::default());
        let runtime = build_test_runtime(provider).await;
        let sessions = test_session_manager(&runtime);
        sessions
            .get_or_create_session_state(
                "webhook:legacy:req-1",
                "webhook:legacy:req-1",
                "webhook",
                "test-provider",
                "test-model",
            )
            .await
            .expect("webhook execution session should exist");

        let err = submit_webhook_event(
            &runtime,
            &GatewayWebhookRequest {
                event_id: "evt-4".to_string(),
                source: "github".to_string(),
                event_type: "push".to_string(),
                content: "should fail".to_string(),
                session_key: "webhook:github:req-4".to_string(),
                base_session_key: Some("webhook:legacy:req-1".to_string()),
                chat_id: "webhook:github:req-4".to_string(),
                sender_id: "github:webhook".to_string(),
                payload: None,
                metadata: BTreeMap::new(),
                remote_addr: None,
                received_at_ms: 4,
            },
        )
        .await
        .expect_err("webhook execution session should be rejected as base target");

        assert!(err.contains("points to a webhook execution session"));
        assert!(
            runtime
                .outbound_transport
                .published_messages()
                .await
                .is_empty()
        );
    }
}
