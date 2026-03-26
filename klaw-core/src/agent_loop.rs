use crate::{
    domain::{DeadLetterMessage, InboundMessage, OutboundMessage},
    observability::{
        AgentTelemetry, ModelRequestRecord, ModelRequestStatus, ModelToolOutcomeRecord,
        ToolOutcomeStatus, TurnOutcomeRecord,
    },
    protocol::{Envelope, ErrorCode, MessageTopic},
    reliability::{
        CircuitBreaker, DeadLetterPolicy, IdempotencyStore, RetryDecision, RetryPolicy,
        idempotency_key,
    },
    transport::{MessageTransport, Subscription, TransportAckHandle, TransportError},
};
use async_trait::async_trait;
use klaw_agent::{
    AgentExecutionError, AgentExecutionInput, AgentExecutionLimits, AgentExecutionStreamEvent,
    ConversationMessage, ToolExecutor, ToolInvocationResult, ToolInvocationSignal,
    run_agent_execution,
};
use klaw_llm::{LlmAuditPayload, LlmError, LlmMedia, LlmProvider, ToolDefinition};
use klaw_tool::{ToolContext, ToolRegistry};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

const META_SYSTEM_PROMPT_KEY: &str = "agent.system_prompt";
const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";
const META_CURRENT_ATTACHMENTS_KEY: &str = "agent.current_attachments";
const META_LLM_USAGE_RECORDS_KEY: &str = "llm.usage.records";
const META_LLM_AUDIT_RECORDS_KEY: &str = "llm.audit.records";
const TOOL_RESULT_LOG_LIMIT: usize = 4000;
const STOP_SIGNAL: &str = "stop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunState {
    Received,
    Validating,
    Scheduling,
    BuildingContext,
    CallingModel,
    ToolLoop,
    Finalizing,
    Publishing,
    Completed,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueStrategy {
    Collect,
    FollowUp,
    Drop,
}

#[derive(Debug, Clone)]
pub struct SessionSchedulingPolicy {
    pub strategy: QueueStrategy,
    pub max_queue_depth: usize,
    pub lock_ttl: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateTransitionEvent {
    StartValidation,
    ValidationPassed,
    ValidationFailed,
    Scheduled,
    QueueAccepted,
    QueueRejected,
    ContextBuilt,
    ModelCalled,
    ToolRequested,
    ToolLoopFinished,
    FinalResponseReady,
    Published,
    RecoverableError,
    FatalError,
}

#[derive(Debug, Clone)]
pub struct RunLimits {
    pub max_tool_iterations: u32,
    pub max_tool_calls: u32,
    pub token_budget: u64,
    pub agent_timeout: Duration,
    pub tool_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub final_response: Option<Envelope<OutboundMessage>>,
    pub error_code: Option<ErrorCode>,
    pub final_state: AgentRunState,
    pub llm_audits: Vec<LlmAuditPayload>,
    pub audit_message_id: Option<uuid::Uuid>,
    pub audit_session_key: Option<String>,
    pub audit_chat_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum AgentRuntimeError {
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
}

#[derive(Clone)]
pub struct ProviderRuntimeSnapshot {
    pub default_provider: Arc<dyn LlmProvider>,
    pub provider_registry: BTreeMap<String, Arc<dyn LlmProvider>>,
    pub default_provider_id: String,
    pub default_model: String,
    pub provider_default_models: BTreeMap<String, String>,
}

pub struct AgentLoop {
    pub limits: RunLimits,
    pub scheduling: SessionSchedulingPolicy,
    pub provider_runtime: std::sync::RwLock<ProviderRuntimeSnapshot>,
    pub tools: ToolRegistry,
    pub system_prompt: std::sync::RwLock<Option<String>>,
    pub telemetry: Option<Arc<dyn AgentTelemetry>>,
}

struct RegistryToolExecutor<'a> {
    tools: &'a ToolRegistry,
    telemetry: Option<&'a Arc<dyn AgentTelemetry>>,
}

#[async_trait]
impl ToolExecutor for RegistryToolExecutor<'_> {
    fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .list()
            .into_iter()
            .filter_map(|name| self.tools.get(&name))
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect()
    }

    async fn execute(
        &self,
        name: &str,
        arguments: serde_json::Value,
        session_key: &str,
        metadata: &BTreeMap<String, serde_json::Value>,
    ) -> ToolInvocationResult {
        let Some(tool) = self.tools.get(name) else {
            return ToolInvocationResult::error(
                format!("tool `{name}` not found"),
                "tool_not_found".to_string(),
                None,
                false,
                Vec::new(),
            );
        };
        info!(tool = name, arguments = %arguments, "calling tool");

        if let Some(telemetry) = self.telemetry {
            telemetry
                .emit_audit_event(
                    "tool_called",
                    uuid::Uuid::new_v4(),
                    serde_json::json!({
                        "tool_name": name,
                        "session_key": session_key,
                    }),
                )
                .await;
        }

        let start = Instant::now();
        match tool
            .execute(
                arguments,
                &ToolContext {
                    session_key: session_key.to_string(),
                    metadata: metadata.clone(),
                },
            )
            .await
        {
            Ok(output) => {
                debug!(
                    tool = name,
                    result = %truncate_for_log(&output.content_for_model, TOOL_RESULT_LOG_LIMIT),
                    "tool result"
                );
                if let Some(telemetry) = self.telemetry {
                    telemetry
                        .record_tool_outcome(
                            session_key,
                            name,
                            ToolOutcomeStatus::Success,
                            None,
                            start.elapsed(),
                        )
                        .await;
                    telemetry
                        .record_model_tool_outcome(ModelToolOutcomeRecord {
                            session_key: session_key.to_string(),
                            provider: metadata_provider(metadata).to_string(),
                            model: metadata_model(metadata).to_string(),
                            tool_name: name.to_string(),
                            status: ToolOutcomeStatus::Success,
                            error_code: None,
                            duration: start.elapsed(),
                            approval_required: false,
                        })
                        .await;
                    telemetry
                        .incr_counter(
                            "agent_tool_success_total",
                            &[("session_key", session_key), ("tool_name", name)],
                            1,
                        )
                        .await;
                    telemetry
                        .observe_histogram(
                            "agent_run_duration_ms",
                            &[("session_key", session_key), ("stage", name)],
                            start.elapsed(),
                        )
                        .await;
                }
                ToolInvocationResult::success(output.content_for_model)
            }
            Err(err) => {
                let error_code = err.code().to_string();
                let message = if error_code == "approval_required" {
                    "approval requested".to_string()
                } else {
                    format!("tool `{name}` failed: {err}")
                };
                let signals = err
                    .signals()
                    .iter()
                    .cloned()
                    .map(|signal| ToolInvocationSignal {
                        kind: signal.kind,
                        payload: signal.payload,
                    })
                    .collect::<Vec<_>>();
                debug!(
                    tool = name,
                    result = %truncate_for_log(&message, TOOL_RESULT_LOG_LIMIT),
                    "tool result"
                );
                if let Some(telemetry) = self.telemetry {
                    let approval_required = error_code == "approval_required";
                    telemetry
                        .record_tool_outcome(
                            session_key,
                            name,
                            ToolOutcomeStatus::Failure,
                            Some(&error_code),
                            start.elapsed(),
                        )
                        .await;
                    telemetry
                        .record_model_tool_outcome(ModelToolOutcomeRecord {
                            session_key: session_key.to_string(),
                            provider: metadata_provider(metadata).to_string(),
                            model: metadata_model(metadata).to_string(),
                            tool_name: name.to_string(),
                            status: ToolOutcomeStatus::Failure,
                            error_code: Some(error_code.clone()),
                            duration: start.elapsed(),
                            approval_required,
                        })
                        .await;
                    telemetry
                        .incr_counter(
                            "agent_tool_failure_total",
                            &[
                                ("session_key", session_key),
                                ("tool_name", name),
                                ("error_code", &error_code),
                            ],
                            1,
                        )
                        .await;
                    telemetry
                        .emit_audit_event(
                            "tool_failed",
                            uuid::Uuid::new_v4(),
                            serde_json::json!({
                                "tool_name": name,
                                "session_key": session_key,
                                "error_code": error_code,
                            }),
                        )
                        .await;
                }
                ToolInvocationResult::error(
                    message,
                    error_code,
                    err.details().cloned(),
                    err.retryable(),
                    signals,
                )
            }
        }
    }
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...[truncated]");
    truncated
}

fn metadata_provider(metadata: &BTreeMap<String, serde_json::Value>) -> &str {
    metadata
        .get("agent.provider_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
}

fn metadata_model(metadata: &BTreeMap<String, serde_json::Value>) -> &str {
    metadata
        .get("agent.model")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
}

async fn record_model_requests(
    telemetry: Option<&Arc<dyn AgentTelemetry>>,
    session_key: &str,
    fallback_provider: &str,
    fallback_model: &str,
    fallback_wire_api: &str,
    audits: &[klaw_agent::AgentRequestAudit],
    usages: &[klaw_agent::AgentRequestUsage],
    fallback_error_code: Option<&str>,
) {
    let Some(telemetry) = telemetry else {
        return;
    };
    let usage_by_seq = usages
        .iter()
        .map(|usage| (usage.request_seq, usage))
        .collect::<BTreeMap<_, _>>();

    for audit in audits {
        let usage = usage_by_seq.get(&audit.request_seq).copied();
        let duration = audit
            .payload
            .responded_at_ms
            .and_then(|responded_at_ms| {
                responded_at_ms
                    .checked_sub(audit.payload.requested_at_ms)
                    .map(|ms| Duration::from_millis(ms.max(0) as u64))
            })
            .unwrap_or_default();
        let tool_call_count = audit
            .payload
            .response_body
            .as_ref()
            .and_then(extract_tool_call_count)
            .or_else(|| usage.map(|_| 0))
            .unwrap_or(0);
        let empty_response = audit
            .payload
            .response_body
            .as_ref()
            .is_some_and(response_body_is_empty);
        telemetry
            .record_model_request(ModelRequestRecord {
                session_key: session_key.to_string(),
                provider: audit.payload.provider.clone(),
                model: audit.payload.model.clone(),
                wire_api: audit.payload.wire_api.clone(),
                status: match audit.payload.status {
                    klaw_llm::LlmAuditStatus::Success => ModelRequestStatus::Success,
                    klaw_llm::LlmAuditStatus::Failed => ModelRequestStatus::Failure,
                },
                error_code: audit
                    .payload
                    .error_code
                    .clone()
                    .or_else(|| fallback_error_code.map(ToString::to_string)),
                duration,
                input_tokens: usage.map(|item| item.usage.input_tokens).unwrap_or(0),
                output_tokens: usage.map(|item| item.usage.output_tokens).unwrap_or(0),
                total_tokens: usage.map(|item| item.usage.total_tokens).unwrap_or(0),
                cached_input_tokens: usage
                    .and_then(|item| item.usage.cached_input_tokens)
                    .unwrap_or(0),
                reasoning_tokens: usage
                    .and_then(|item| item.usage.reasoning_tokens)
                    .unwrap_or(0),
                provider_request_id: audit.payload.provider_request_id.clone(),
                provider_response_id: audit.payload.provider_response_id.clone(),
                tool_call_count,
                has_tool_call: tool_call_count > 0,
                empty_response,
            })
            .await;
    }

    if audits.is_empty() && !usages.is_empty() {
        for usage in usages {
            telemetry
                .record_model_request(ModelRequestRecord {
                    session_key: session_key.to_string(),
                    provider: fallback_provider.to_string(),
                    model: fallback_model.to_string(),
                    wire_api: fallback_wire_api.to_string(),
                    status: ModelRequestStatus::Success,
                    error_code: None,
                    duration: Duration::default(),
                    input_tokens: usage.usage.input_tokens,
                    output_tokens: usage.usage.output_tokens,
                    total_tokens: usage.usage.total_tokens,
                    cached_input_tokens: usage.usage.cached_input_tokens.unwrap_or(0),
                    reasoning_tokens: usage.usage.reasoning_tokens.unwrap_or(0),
                    provider_request_id: usage.usage.provider_request_id.clone(),
                    provider_response_id: usage.usage.provider_response_id.clone(),
                    tool_call_count: 0,
                    has_tool_call: false,
                    empty_response: false,
                })
                .await;
        }
    }
}

async fn record_turn_outcome(
    telemetry: Option<&Arc<dyn AgentTelemetry>>,
    session_key: &str,
    provider: &str,
    model: &str,
    requests_in_turn: usize,
    tool_iterations: usize,
    completed: bool,
    degraded: bool,
    token_budget_exceeded: bool,
    tool_loop_exhausted: bool,
) {
    let Some(telemetry) = telemetry else {
        return;
    };
    telemetry
        .record_turn_outcome(TurnOutcomeRecord {
            session_key: session_key.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            requests_in_turn: requests_in_turn as u32,
            tool_iterations: tool_iterations as u32,
            completed,
            degraded,
            token_budget_exceeded,
            tool_loop_exhausted,
        })
        .await;
}

fn extract_tool_call_count(value: &serde_json::Value) -> Option<u32> {
    value
        .get("output")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items.iter().fold(0u32, |count, item| {
                count
                    + u32::from(
                        item.get("type")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|kind| kind.contains("tool_call")),
                    )
            })
        })
}

fn response_body_is_empty(value: &serde_json::Value) -> bool {
    value.as_object().is_some_and(|object| object.is_empty())
}

fn error_code_label(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::InvalidSchema => "invalid_schema",
        ErrorCode::ValidationFailed => "validation_failed",
        ErrorCode::DuplicateMessage => "duplicate_message",
        ErrorCode::SessionBusy => "session_busy",
        ErrorCode::AgentTimeout => "agent_timeout",
        ErrorCode::ToolTimeout => "tool_timeout",
        ErrorCode::ProviderUnavailable => "provider_unavailable",
        ErrorCode::ProviderResponseInvalid => "provider_response_invalid",
        ErrorCode::TransportUnavailable => "transport_unavailable",
        ErrorCode::RetryExhausted => "retry_exhausted",
        ErrorCode::BudgetExceeded => "budget_exceeded",
        ErrorCode::SentToDeadLetter => "sent_to_deadletter",
    }
}

impl AgentLoop {
    pub fn new(
        limits: RunLimits,
        scheduling: SessionSchedulingPolicy,
        provider: Arc<dyn LlmProvider>,
        tools: ToolRegistry,
    ) -> Self {
        Self {
            limits,
            scheduling,
            provider_runtime: std::sync::RwLock::new(ProviderRuntimeSnapshot {
                default_provider: provider.clone(),
                provider_registry: BTreeMap::from([("default".to_string(), provider)]),
                default_provider_id: "default".to_string(),
                default_model: "default".to_string(),
                provider_default_models: BTreeMap::from([(
                    "default".to_string(),
                    "default".to_string(),
                )]),
            }),
            tools,
            system_prompt: std::sync::RwLock::new(None),
            telemetry: None,
        }
    }

    pub fn new_with_identity(
        limits: RunLimits,
        scheduling: SessionSchedulingPolicy,
        provider: Arc<dyn LlmProvider>,
        active_provider_id: String,
        active_model: String,
        tools: ToolRegistry,
    ) -> Self {
        Self {
            limits,
            scheduling,
            provider_runtime: std::sync::RwLock::new(ProviderRuntimeSnapshot {
                default_provider: provider.clone(),
                provider_registry: BTreeMap::from([(active_provider_id.clone(), provider)]),
                default_provider_id: active_provider_id.clone(),
                default_model: active_model.clone(),
                provider_default_models: BTreeMap::from([(active_provider_id, active_model)]),
            }),
            tools,
            system_prompt: std::sync::RwLock::new(None),
            telemetry: None,
        }
    }

    pub fn with_provider_registry(
        self,
        provider_registry: BTreeMap<String, Arc<dyn LlmProvider>>,
    ) -> Self {
        let provider_default_models = provider_registry
            .iter()
            .map(|(provider_id, provider)| {
                (provider_id.clone(), provider.default_model().to_string())
            })
            .collect();
        let mut guard = self
            .provider_runtime
            .write()
            .unwrap_or_else(|err| err.into_inner());
        guard.provider_registry = provider_registry;
        guard.provider_default_models = provider_default_models;
        drop(guard);
        self
    }

    pub fn provider_runtime_snapshot(&self) -> ProviderRuntimeSnapshot {
        self.provider_runtime
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub fn set_provider_runtime_snapshot(&self, provider_runtime: ProviderRuntimeSnapshot) {
        let mut guard = self
            .provider_runtime
            .write()
            .unwrap_or_else(|err| err.into_inner());
        *guard = provider_runtime;
    }

    pub fn with_system_prompt(self, system_prompt: Option<String>) -> Self {
        self.set_system_prompt(system_prompt);
        self
    }

    pub fn with_telemetry(mut self, telemetry: Arc<dyn AgentTelemetry>) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub fn set_system_prompt(&self, system_prompt: Option<String>) {
        let next = system_prompt
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let mut guard = self
            .system_prompt
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = next;
    }

    pub fn system_prompt(&self) -> Option<String> {
        self.system_prompt
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn transition(&self, state: AgentRunState, event: StateTransitionEvent) -> AgentRunState {
        use AgentRunState as S;
        use StateTransitionEvent as E;
        match (state, event) {
            (S::Received, E::StartValidation) => S::Validating,
            (S::Validating, E::ValidationPassed) => S::Scheduling,
            (S::Validating, E::ValidationFailed) => S::Failed,
            (S::Scheduling, E::Scheduled) => S::BuildingContext,
            (S::Scheduling, E::QueueAccepted) => S::Degraded,
            (S::Scheduling, E::QueueRejected) => S::Failed,
            (S::BuildingContext, E::ContextBuilt) => S::CallingModel,
            (S::CallingModel, E::ModelCalled) => S::Finalizing,
            (S::CallingModel, E::ToolRequested) => S::ToolLoop,
            (S::ToolLoop, E::ToolLoopFinished) => S::Finalizing,
            (S::Finalizing, E::FinalResponseReady) => S::Publishing,
            (S::Publishing, E::Published) => S::Completed,
            (_, E::RecoverableError) => S::Degraded,
            (_, E::FatalError) => S::Failed,
            (s, _) => s,
        }
    }

    pub async fn process_message(
        &self,
        msg: Envelope<InboundMessage>,
        _enable_streaming: bool,
    ) -> ProcessOutcome {
        self.process_message_inner(msg, None).await
    }

    pub async fn process_message_streaming(
        &self,
        msg: Envelope<InboundMessage>,
        stream: UnboundedSender<AgentExecutionStreamEvent>,
    ) -> ProcessOutcome {
        self.process_message_inner(msg, Some(stream)).await
    }

    async fn process_message_inner(
        &self,
        msg: Envelope<InboundMessage>,
        stream: Option<UnboundedSender<AgentExecutionStreamEvent>>,
    ) -> ProcessOutcome {
        let start = Instant::now();
        let session_key = msg.payload.session_key.as_str();
        let provider_runtime = self.provider_runtime_snapshot();
        let provider_id = msg
            .payload
            .metadata
            .get("agent.provider_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(provider_runtime.default_provider_id.as_str());

        info!(message_id = %msg.header.message_id, "process message");

        if let Some(ref telemetry) = self.telemetry {
            telemetry
                .incr_counter(
                    "agent_inbound_consumed_total",
                    &[("session_key", session_key), ("provider", provider_id)],
                    1,
                )
                .await;
            telemetry
                .emit_audit_event(
                    "inbound_received",
                    msg.header.message_id,
                    serde_json::json!({"session_key": session_key, "provider": provider_id}),
                )
                .await;
        }

        if msg.payload.content.trim().is_empty() {
            if let Some(ref telemetry) = self.telemetry {
                telemetry
                    .emit_audit_event(
                        "validation_failed",
                        msg.header.message_id,
                        serde_json::json!({
                            "session_key": session_key,
                            "error_code": "validation_failed",
                            "reason": "empty_content"
                        }),
                    )
                    .await;
            }
            return ProcessOutcome {
                final_response: None,
                error_code: Some(ErrorCode::ValidationFailed),
                final_state: AgentRunState::Failed,
                llm_audits: Vec::new(),
                audit_message_id: Some(msg.header.message_id),
                audit_session_key: Some(msg.payload.session_key.clone()),
                audit_chat_id: Some(msg.payload.chat_id.clone()),
            };
        }

        let mut state = AgentRunState::Received;
        state = self.transition(state, StateTransitionEvent::StartValidation);
        state = self.transition(state, StateTransitionEvent::ValidationPassed);
        state = self.transition(state, StateTransitionEvent::Scheduled);
        state = self.transition(state, StateTransitionEvent::ContextBuilt);

        let conversation_history = extract_conversation_history(&msg.payload.metadata);
        let user_media = extract_user_media(&msg.payload);
        let current_attachments = current_attachment_contexts(&msg.payload.media_references);
        let user_content = augment_user_content_with_attachment_context(
            &msg.payload.content,
            &current_attachments,
        );
        if !msg.payload.media_references.is_empty() {
            info!(
                message_id = %msg.header.message_id,
                media_references = msg.payload.media_references.len(),
                user_media = user_media.len(),
                "agent inbound media summary"
            );
        }
        let requested_provider_id = msg
            .payload
            .metadata
            .get("agent.provider_id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| provider_runtime.default_provider_id.clone());
        let (resolved_provider_id, provider) = if let Some(provider) = provider_runtime
            .provider_registry
            .get(&requested_provider_id)
        {
            (requested_provider_id, Arc::clone(provider))
        } else {
            warn!(
                requested_provider = requested_provider_id.as_str(),
                fallback_provider = provider_runtime.default_provider_id.as_str(),
                "provider override not found, falling back to active provider"
            );
            (
                provider_runtime.default_provider_id.clone(),
                Arc::clone(&provider_runtime.default_provider),
            )
        };
        let resolved_model = msg
            .payload
            .metadata
            .get("agent.model")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| provider_runtime.default_model.clone());

        let mut tool_metadata = msg.payload.metadata.clone();
        tool_metadata.remove(META_CONVERSATION_HISTORY_KEY);
        tool_metadata.insert(
            "agent.provider_id".to_string(),
            serde_json::Value::String(resolved_provider_id.clone()),
        );
        tool_metadata.insert(
            "agent.model".to_string(),
            serde_json::Value::String(resolved_model.clone()),
        );
        tool_metadata.insert(
            "agent.parent_session_key".to_string(),
            serde_json::Value::String(msg.payload.session_key.clone()),
        );
        tool_metadata.insert(
            "agent.message_id".to_string(),
            serde_json::Value::String(msg.header.message_id.to_string()),
        );
        if !current_attachments.is_empty() {
            tool_metadata.insert(
                META_CURRENT_ATTACHMENTS_KEY.to_string(),
                serde_json::Value::Array(current_attachments),
            );
        }
        if let Some(system_prompt) = self.system_prompt() {
            tool_metadata
                .entry(META_SYSTEM_PROMPT_KEY.to_string())
                .or_insert_with(|| serde_json::Value::String(system_prompt));
        }

        state = self.transition(state, StateTransitionEvent::ModelCalled);
        state = self.transition(state, StateTransitionEvent::ToolRequested);
        let executor = RegistryToolExecutor {
            tools: &self.tools,
            telemetry: self.telemetry.as_ref(),
        };
        let result = run_agent_execution(
            provider.as_ref(),
            &executor,
            AgentExecutionInput {
                user_content,
                user_media,
                conversation_history,
                session_key: msg.payload.session_key.clone(),
                tool_metadata,
                model: Some(resolved_model.clone()),
            },
            AgentExecutionLimits {
                max_tool_iterations: self.limits.max_tool_iterations,
                max_tool_calls: self.limits.max_tool_calls,
                token_budget: self.limits.token_budget,
            },
            stream,
        )
        .await;
        state = self.transition(state, StateTransitionEvent::ToolLoopFinished);

        match result {
            Ok(output) => {
                let request_count = output.request_audits.len().max(output.request_usages.len());
                let tool_iterations = output
                    .request_audits
                    .iter()
                    .filter(|record| {
                        record
                            .payload
                            .response_body
                            .as_ref()
                            .and_then(extract_tool_call_count)
                            .is_some_and(|count| count > 0)
                    })
                    .count();
                record_model_requests(
                    self.telemetry.as_ref(),
                    session_key,
                    &resolved_provider_id,
                    &resolved_model,
                    provider.wire_api().unwrap_or(provider.name()),
                    &output.request_audits,
                    &output.request_usages,
                    None,
                )
                .await;
                record_turn_outcome(
                    self.telemetry.as_ref(),
                    session_key,
                    &resolved_provider_id,
                    &resolved_model,
                    request_count,
                    tool_iterations,
                    true,
                    false,
                    false,
                    false,
                )
                .await;
                state = self.transition(state, StateTransitionEvent::FinalResponseReady);
                state = self.transition(state, StateTransitionEvent::Published);

                if let Some(ref telemetry) = self.telemetry {
                    telemetry
                        .incr_counter(
                            "agent_outbound_published_total",
                            &[
                                ("session_key", session_key),
                                ("provider", &resolved_provider_id),
                            ],
                            1,
                        )
                        .await;
                    telemetry
                        .observe_histogram(
                            "agent_run_duration_ms",
                            &[("session_key", session_key), ("stage", "process")],
                            start.elapsed(),
                        )
                        .await;
                    telemetry
                        .emit_audit_event(
                            "final_response_published",
                            msg.header.message_id,
                            serde_json::json!({"session_key": session_key, "provider": resolved_provider_id}),
                        )
                        .await;
                }

                let mut response_metadata = heartbeat_response_metadata(&msg.payload.metadata);
                if !output.tool_signals.is_empty() {
                    response_metadata.insert(
                        "tool.signals".to_string(),
                        serde_json::to_value(&output.tool_signals)
                            .unwrap_or(serde_json::Value::Null),
                    );
                    if let Some(approval_required) = output
                        .tool_signals
                        .iter()
                        .find(|signal| signal.kind == "approval_required")
                    {
                        response_metadata.insert(
                            "approval.required".to_string(),
                            serde_json::Value::Bool(true),
                        );
                        response_metadata.insert(
                            "approval.signal".to_string(),
                            approval_required.payload.clone(),
                        );
                        if let Some(approval_id) = approval_required
                            .payload
                            .get("approval_id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            response_metadata.insert(
                                "approval.id".to_string(),
                                serde_json::Value::String(approval_id.to_string()),
                            );
                        }
                    }
                    if let Some(stop_signal) = output
                        .tool_signals
                        .iter()
                        .find(|signal| signal.kind == STOP_SIGNAL)
                    {
                        response_metadata
                            .insert("turn.stopped".to_string(), serde_json::Value::Bool(true));
                        response_metadata
                            .insert("turn.stop_signal".to_string(), stop_signal.payload.clone());
                    }
                }
                if let Some(reasoning) = output.reasoning.filter(|value| !value.trim().is_empty()) {
                    response_metadata.insert(
                        "reasoning".to_string(),
                        serde_json::Value::String(reasoning),
                    );
                }
                if !output.request_usages.is_empty() {
                    response_metadata.insert(
                        META_LLM_USAGE_RECORDS_KEY.to_string(),
                        serde_json::Value::Array(
                            output
                                .request_usages
                                .into_iter()
                                .map(|record| {
                                    serde_json::json!({
                                        "request_seq": record.request_seq,
                                        "provider": resolved_provider_id,
                                        "model": resolved_model,
                                        "wire_api": provider.wire_api().unwrap_or(provider.name()),
                                        "input_tokens": record.usage.input_tokens,
                                        "output_tokens": record.usage.output_tokens,
                                        "total_tokens": record.usage.total_tokens,
                                        "cached_input_tokens": record.usage.cached_input_tokens,
                                        "reasoning_tokens": record.usage.reasoning_tokens,
                                        "source": record.source.as_str(),
                                        "provider_request_id": record.usage.provider_request_id,
                                        "provider_response_id": record.usage.provider_response_id
                                    })
                                })
                                .collect(),
                        ),
                    );
                }
                if !output.request_audits.is_empty() {
                    response_metadata.insert(
                        META_LLM_AUDIT_RECORDS_KEY.to_string(),
                        serde_json::Value::Array(
                            output
                                .request_audits
                                .iter()
                                .map(|record| {
                                    serde_json::json!({
                                        "request_seq": record.request_seq,
                                        "provider": record.payload.provider,
                                        "model": record.payload.model,
                                        "wire_api": record.payload.wire_api,
                                        "status": record.payload.status.as_str(),
                                        "error_code": record.payload.error_code,
                                        "error_message": record.payload.error_message,
                                        "provider_request_id": record.payload.provider_request_id,
                                        "provider_response_id": record.payload.provider_response_id,
                                        "request_body": record.payload.request_body,
                                        "response_body": record.payload.response_body,
                                        "requested_at_ms": record.payload.requested_at_ms,
                                        "responded_at_ms": record.payload.responded_at_ms
                                    })
                                })
                                .collect(),
                        ),
                    );
                }
                ProcessOutcome {
                    final_response: Some(Envelope {
                        header: msg.header.clone(),
                        metadata: BTreeMap::new(),
                        payload: OutboundMessage {
                            channel: msg.payload.channel.clone(),
                            chat_id: msg.payload.chat_id.clone(),
                            content: output.content,
                            reply_to: None,
                            metadata: response_metadata,
                        },
                    }),
                    error_code: None,
                    final_state: state,
                    llm_audits: output
                        .request_audits
                        .into_iter()
                        .map(|record| record.payload)
                        .collect(),
                    audit_message_id: Some(msg.header.message_id),
                    audit_session_key: Some(msg.payload.session_key.clone()),
                    audit_chat_id: Some(msg.payload.chat_id.clone()),
                }
            }
            Err(AgentExecutionError::Provider(err)) => {
                warn!(error = %err, "provider failed");
                let audits = err.audit().cloned().into_iter().collect::<Vec<_>>();
                let request_audits = audits
                    .iter()
                    .enumerate()
                    .map(|(index, payload)| klaw_agent::AgentRequestAudit {
                        request_seq: (index as i64) + 1,
                        payload: payload.clone(),
                    })
                    .collect::<Vec<_>>();
                record_model_requests(
                    self.telemetry.as_ref(),
                    session_key,
                    &resolved_provider_id,
                    &resolved_model,
                    provider.wire_api().unwrap_or(provider.name()),
                    &request_audits,
                    &[],
                    Some(error_code_label(map_llm_error_to_code(&err))),
                )
                .await;
                record_turn_outcome(
                    self.telemetry.as_ref(),
                    session_key,
                    &resolved_provider_id,
                    &resolved_model,
                    request_audits.len(),
                    0,
                    false,
                    true,
                    false,
                    false,
                )
                .await;
                if let Some(ref telemetry) = self.telemetry {
                    telemetry
                        .incr_counter(
                            "agent_tool_failure_total",
                            &[
                                ("session_key", session_key),
                                ("tool_name", "provider"),
                                ("error_code", "provider_error"),
                            ],
                            1,
                        )
                        .await;
                    telemetry
                        .emit_audit_event(
                            "tool_failed",
                            msg.header.message_id,
                            serde_json::json!({
                                "session_key": session_key,
                                "error_code": "provider_error",
                                "error": err.to_string()
                            }),
                        )
                        .await;
                }
                ProcessOutcome {
                    final_response: None,
                    error_code: Some(map_llm_error_to_code(&err)),
                    final_state: AgentRunState::Degraded,
                    llm_audits: audits,
                    audit_message_id: Some(msg.header.message_id),
                    audit_session_key: Some(msg.payload.session_key.clone()),
                    audit_chat_id: Some(msg.payload.chat_id.clone()),
                }
            }
            Err(AgentExecutionError::ToolLoopExhausted) => {
                record_turn_outcome(
                    self.telemetry.as_ref(),
                    session_key,
                    &resolved_provider_id,
                    &resolved_model,
                    0,
                    self.limits.max_tool_iterations as usize,
                    false,
                    true,
                    false,
                    true,
                )
                .await;
                if let Some(ref telemetry) = self.telemetry {
                    telemetry
                        .incr_counter(
                            "agent_tool_failure_total",
                            &[
                                ("session_key", session_key),
                                ("tool_name", "tool_loop"),
                                ("error_code", "retry_exhausted"),
                            ],
                            1,
                        )
                        .await;
                }
                ProcessOutcome {
                    final_response: None,
                    error_code: Some(ErrorCode::RetryExhausted),
                    final_state: AgentRunState::Failed,
                    llm_audits: Vec::new(),
                    audit_message_id: Some(msg.header.message_id),
                    audit_session_key: Some(msg.payload.session_key.clone()),
                    audit_chat_id: Some(msg.payload.chat_id.clone()),
                }
            }
            Err(AgentExecutionError::BudgetExceeded {
                used_tokens,
                token_budget,
            }) => {
                warn!(used_tokens, token_budget, "agent token budget exceeded");
                record_turn_outcome(
                    self.telemetry.as_ref(),
                    session_key,
                    &resolved_provider_id,
                    &resolved_model,
                    0,
                    0,
                    false,
                    false,
                    true,
                    false,
                )
                .await;
                if let Some(ref telemetry) = self.telemetry {
                    telemetry
                        .incr_counter(
                            "agent_tool_failure_total",
                            &[
                                ("session_key", session_key),
                                ("tool_name", "budget"),
                                ("error_code", "budget_exceeded"),
                            ],
                            1,
                        )
                        .await;
                }
                ProcessOutcome {
                    final_response: None,
                    error_code: Some(ErrorCode::BudgetExceeded),
                    final_state: AgentRunState::Failed,
                    llm_audits: Vec::new(),
                    audit_message_id: Some(msg.header.message_id),
                    audit_session_key: Some(msg.payload.session_key.clone()),
                    audit_chat_id: Some(msg.payload.chat_id.clone()),
                }
            }
        }
    }

    pub async fn run_once<InT, OutT, S>(
        &self,
        inbound_transport: &InT,
        outbound_transport: &OutT,
        inbound_subscription: &Subscription,
        idempotency: &S,
    ) -> Result<ProcessOutcome, AgentRuntimeError>
    where
        InT: MessageTransport<InboundMessage>,
        OutT: MessageTransport<OutboundMessage>,
        S: IdempotencyStore,
    {
        let inbound = inbound_transport.consume(inbound_subscription).await?;
        let dedupe_key = idempotency_key(
            &inbound.payload.header.message_id.to_string(),
            &inbound.payload.header.session_key,
            "agent_run",
        );
        if idempotency.seen(&dedupe_key).await {
            inbound_transport.ack(&inbound.ack_handle).await?;
            return Ok(ProcessOutcome {
                final_response: None,
                error_code: Some(ErrorCode::DuplicateMessage),
                final_state: AgentRunState::Completed,
                llm_audits: Vec::new(),
                audit_message_id: Some(inbound.payload.header.message_id),
                audit_session_key: Some(inbound.payload.header.session_key.clone()),
                audit_chat_id: Some(inbound.payload.payload.chat_id.clone()),
            });
        }

        let outcome = self.process_message(inbound.payload, false).await;
        if let Some(outbound) = outcome.final_response.clone() {
            outbound_transport
                .publish(MessageTopic::Outbound.as_str(), outbound)
                .await?;
        }
        idempotency
            .mark_seen(
                &dedupe_key,
                self.limits.agent_timeout + self.scheduling.lock_ttl,
            )
            .await;
        inbound_transport.ack(&inbound.ack_handle).await?;
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_once_reliable<InT, OutT, DlqT, S, R, C>(
        &self,
        inbound_transport: &InT,
        outbound_transport: &OutT,
        deadletter_transport: &DlqT,
        inbound_subscription: &Subscription,
        idempotency: &S,
        retry_policy: &R,
        deadletter_policy: &DeadLetterPolicy,
        circuit_breaker: &C,
    ) -> Result<ProcessOutcome, AgentRuntimeError>
    where
        InT: MessageTransport<InboundMessage>,
        OutT: MessageTransport<OutboundMessage>,
        DlqT: MessageTransport<DeadLetterMessage>,
        S: IdempotencyStore,
        R: RetryPolicy,
        C: CircuitBreaker,
    {
        let inbound = inbound_transport.consume(inbound_subscription).await?;
        let dedupe_key = idempotency_key(
            &inbound.payload.header.message_id.to_string(),
            &inbound.payload.header.session_key,
            "agent_run",
        );
        if idempotency.seen(&dedupe_key).await {
            inbound_transport.ack(&inbound.ack_handle).await?;
            return Ok(ProcessOutcome {
                final_response: None,
                error_code: Some(ErrorCode::DuplicateMessage),
                final_state: AgentRunState::Completed,
                llm_audits: Vec::new(),
                audit_message_id: Some(inbound.payload.header.message_id),
                audit_session_key: Some(inbound.payload.header.session_key.clone()),
                audit_chat_id: Some(inbound.payload.payload.chat_id.clone()),
            });
        }

        let mut attempt = inbound.payload.header.attempt.max(1);
        loop {
            if !circuit_breaker.allow_request().await {
                let decision = retry_policy.classify("provider_unavailable", attempt);
                if let Some(done) = self
                    .handle_retry_decision(
                        decision,
                        attempt,
                        &inbound.payload,
                        inbound_transport,
                        deadletter_transport,
                        &inbound.ack_handle,
                        deadletter_policy,
                    )
                    .await?
                {
                    return Ok(done);
                }
                attempt += 1;
                continue;
            }

            let outcome = self.process_message(inbound.payload.clone(), false).await;
            if outcome.error_code.is_none() {
                if let Some(outbound) = outcome.final_response.clone() {
                    match outbound_transport
                        .publish(MessageTopic::Outbound.as_str(), outbound)
                        .await
                    {
                        Ok(_) => {
                            circuit_breaker.on_success().await;
                            idempotency
                                .mark_seen(
                                    &dedupe_key,
                                    self.limits.agent_timeout + self.scheduling.lock_ttl,
                                )
                                .await;
                            inbound_transport.ack(&inbound.ack_handle).await?;
                            return Ok(outcome);
                        }
                        Err(_) => {
                            circuit_breaker.on_failure().await;
                            let decision = retry_policy.classify("transport_unavailable", attempt);
                            if let Some(done) = self
                                .handle_retry_decision(
                                    decision,
                                    attempt,
                                    &inbound.payload,
                                    inbound_transport,
                                    deadletter_transport,
                                    &inbound.ack_handle,
                                    deadletter_policy,
                                )
                                .await?
                            {
                                return Ok(done);
                            }
                            attempt += 1;
                            continue;
                        }
                    }
                }
            }

            let decision = retry_policy.classify(classify_error_kind(outcome.error_code), attempt);
            if matches!(
                outcome.error_code,
                Some(ErrorCode::ProviderUnavailable | ErrorCode::ToolTimeout)
            ) {
                circuit_breaker.on_failure().await;
            }
            if let Some(done) = self
                .handle_retry_decision(
                    decision,
                    attempt,
                    &inbound.payload,
                    inbound_transport,
                    deadletter_transport,
                    &inbound.ack_handle,
                    deadletter_policy,
                )
                .await?
            {
                return Ok(done);
            }
            attempt += 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_retry_decision<InT, DlqT>(
        &self,
        decision: RetryDecision,
        attempt: u32,
        inbound_payload: &Envelope<InboundMessage>,
        inbound_transport: &InT,
        deadletter_transport: &DlqT,
        ack_handle: &TransportAckHandle,
        deadletter_policy: &DeadLetterPolicy,
    ) -> Result<Option<ProcessOutcome>, AgentRuntimeError>
    where
        InT: MessageTransport<InboundMessage>,
        DlqT: MessageTransport<DeadLetterMessage>,
    {
        match decision {
            RetryDecision::RetryNow => {
                if let Some(ref telemetry) = self.telemetry {
                    telemetry
                        .incr_counter(
                            "agent_retry_total",
                            &[
                                ("session_key", inbound_payload.header.session_key.as_str()),
                                ("error_code", "retry_now"),
                            ],
                            1,
                        )
                        .await;
                }
                Ok(None)
            }
            RetryDecision::RetryAfter(delay) => {
                if let Some(ref telemetry) = self.telemetry {
                    telemetry
                        .incr_counter(
                            "agent_retry_total",
                            &[
                                ("session_key", inbound_payload.header.session_key.as_str()),
                                ("error_code", "retry_after"),
                            ],
                            1,
                        )
                        .await;
                }
                sleep(delay).await;
                Ok(None)
            }
            RetryDecision::Abort => {
                inbound_transport.ack(ack_handle).await?;
                Ok(Some(ProcessOutcome {
                    final_response: None,
                    error_code: Some(ErrorCode::RetryExhausted),
                    final_state: AgentRunState::Failed,
                    llm_audits: Vec::new(),
                    audit_message_id: Some(inbound_payload.header.message_id),
                    audit_session_key: Some(inbound_payload.header.session_key.clone()),
                    audit_chat_id: Some(inbound_payload.payload.chat_id.clone()),
                }))
            }
            RetryDecision::SendToDeadLetter => {
                error!(attempt, "send to dlq");
                let session_key = inbound_payload.header.session_key.as_str();

                if let Some(ref telemetry) = self.telemetry {
                    telemetry
                        .incr_counter("agent_deadletter_total", &[("session_key", session_key)], 1)
                        .await;
                    telemetry
                        .emit_audit_event(
                            "message_sent_dlq",
                            inbound_payload.header.message_id,
                            serde_json::json!({
                                "session_key": session_key,
                                "attempts": attempt,
                                "reason": "exhausted_retries"
                            }),
                        )
                        .await;
                }

                let deadletter = Envelope {
                    header: inbound_payload.header.clone(),
                    metadata: BTreeMap::new(),
                    payload: DeadLetterMessage {
                        original_message_id: inbound_payload.header.message_id.to_string(),
                        session_key: inbound_payload.header.session_key.clone(),
                        final_error: format!("{:?}", ErrorCode::SentToDeadLetter),
                        attempts: attempt,
                        reason: format!("exhausted retries, topic={}", deadletter_policy.topic),
                        original_payload: inbound_payload.payload.clone(),
                    },
                };
                deadletter_transport
                    .publish(MessageTopic::DeadLetter.as_str(), deadletter)
                    .await?;
                inbound_transport.ack(ack_handle).await?;
                Ok(Some(ProcessOutcome {
                    final_response: None,
                    error_code: Some(ErrorCode::SentToDeadLetter),
                    final_state: AgentRunState::Failed,
                    llm_audits: Vec::new(),
                    audit_message_id: Some(inbound_payload.header.message_id),
                    audit_session_key: Some(inbound_payload.header.session_key.clone()),
                    audit_chat_id: Some(inbound_payload.payload.chat_id.clone()),
                }))
            }
        }
    }
}

fn classify_error_kind(code: Option<ErrorCode>) -> &'static str {
    match code {
        Some(ErrorCode::ValidationFailed | ErrorCode::InvalidSchema) => "validation",
        Some(ErrorCode::DuplicateMessage) => "duplicate",
        Some(ErrorCode::ProviderUnavailable) => "provider_unavailable",
        Some(ErrorCode::ToolTimeout) => "tool_timeout",
        Some(ErrorCode::TransportUnavailable) => "transport_unavailable",
        _ => "unknown",
    }
}

fn map_llm_error_to_code(err: &LlmError) -> ErrorCode {
    match err {
        LlmError::ProviderUnavailable { .. }
        | LlmError::RequestFailed { .. }
        | LlmError::StreamFailed { .. } => ErrorCode::ProviderUnavailable,
        LlmError::InvalidResponse { .. } => ErrorCode::ProviderResponseInvalid,
    }
}

fn heartbeat_response_metadata(
    inbound_metadata: &BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    let mut response_metadata = BTreeMap::new();
    for (key, value) in inbound_metadata {
        if key == "trigger.kind" || key.starts_with("heartbeat.") || key.starts_with("channel.") {
            response_metadata.insert(key.clone(), value.clone());
        }
    }
    response_metadata
}

fn extract_conversation_history(
    metadata: &BTreeMap<String, serde_json::Value>,
) -> Vec<ConversationMessage> {
    metadata
        .get(META_CONVERSATION_HISTORY_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<ConversationMessage>>(value).ok())
        .unwrap_or_default()
}

fn extract_user_media(inbound: &InboundMessage) -> Vec<LlmMedia> {
    inbound
        .media_references
        .iter()
        .filter_map(|media| {
            let url = media
                .remote_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)?;
            let mime_type = media
                .mime_type
                .clone()
                .or_else(|| {
                    media
                        .metadata
                        .get("archive.mime_type")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .filter(|value| !value.trim().is_empty());
            let is_image = mime_type
                .as_deref()
                .map(|value| value.trim().to_ascii_lowercase().starts_with("image/"))
                .unwrap_or_else(|| url.trim().to_ascii_lowercase().starts_with("data:image/"));
            if !is_image {
                return None;
            }
            Some(LlmMedia { mime_type, url })
        })
        .collect()
}

fn current_attachment_contexts(
    media_references: &[crate::MediaReference],
) -> Vec<serde_json::Value> {
    media_references
        .iter()
        .filter_map(|media| {
            let archive_id = media
                .metadata
                .get("archive.id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            let mut item = serde_json::Map::new();
            item.insert(
                "archive_id".to_string(),
                serde_json::Value::String(archive_id.to_string()),
            );
            item.insert(
                "access".to_string(),
                serde_json::Value::String("read_only".to_string()),
            );
            item.insert(
                "recommended_workflow".to_string(),
                serde_json::Value::String("copy_to_workspace_before_edit".to_string()),
            );
            item.insert(
                "source_kind".to_string(),
                serde_json::Value::String(format!("{:?}", media.source_kind).to_ascii_lowercase()),
            );
            if let Some(filename) = media
                .filename
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                item.insert(
                    "filename".to_string(),
                    serde_json::Value::String(filename.to_string()),
                );
            }
            if let Some(mime_type) = media
                .mime_type
                .as_deref()
                .or_else(|| {
                    media
                        .metadata
                        .get("archive.mime_type")
                        .and_then(serde_json::Value::as_str)
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                item.insert(
                    "mime_type".to_string(),
                    serde_json::Value::String(mime_type.to_string()),
                );
            }
            if let Some(storage_rel_path) = media
                .metadata
                .get("archive.storage_rel_path")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                item.insert(
                    "storage_rel_path".to_string(),
                    serde_json::Value::String(storage_rel_path.to_string()),
                );
            }
            if let Some(size_bytes) = media
                .metadata
                .get("archive.size_bytes")
                .and_then(serde_json::Value::as_i64)
            {
                item.insert(
                    "size_bytes".to_string(),
                    serde_json::Value::from(size_bytes),
                );
            }
            if let Some(message_id) = media
                .message_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                item.insert(
                    "message_id".to_string(),
                    serde_json::Value::String(message_id.to_string()),
                );
            }
            Some(serde_json::Value::Object(item))
        })
        .collect()
}

fn augment_user_content_with_attachment_context(
    user_content: &str,
    attachments: &[serde_json::Value],
) -> String {
    if attachments.is_empty() {
        return user_content.to_string();
    }

    let mut lines = vec![
        "Current message attachments:".to_string(),
        "If an attachment below already includes an archive_id, prefer calling the archive tool with action=get and that exact archive_id. Use list_current_attachments only to confirm attachments from the current message, and use list_session_attachments when the user is referring to files from earlier turns in this same session. For audio or voice attachments, use the voice tool with action=stt and the attachment archive_id to transcribe them.".to_string(),
    ];
    for (idx, attachment) in attachments.iter().enumerate() {
        let Some(item) = attachment.as_object() else {
            continue;
        };
        let filename = item
            .get("filename")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unnamed");
        let archive_id = item
            .get("archive_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let storage_rel_path = item
            .get("storage_rel_path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let mime_type = item
            .get("mime_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let size_bytes = item
            .get("size_bytes")
            .and_then(serde_json::Value::as_i64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        lines.push(format!(
            "{idx}. filename={filename}; archive_id={archive_id}; storage_rel_path={storage_rel_path}; mime_type={mime_type}; size_bytes={size_bytes}; access=read_only archive; if modification is needed, copy the file into workspace first and edit the copied file there.",
            idx = idx + 1
        ));
    }

    if lines.len() == 1 {
        return user_content.to_string();
    }

    let trimmed = user_content.trim_end();
    if trimmed.is_empty() {
        lines.join("\n")
    } else {
        format!("{trimmed}\n\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentLoop, QueueStrategy, RunLimits, SessionSchedulingPolicy,
        augment_user_content_with_attachment_context, current_attachment_contexts,
        heartbeat_response_metadata,
    };
    use crate::{
        domain::InboundMessage,
        observability::{
            AgentTelemetry, ModelRequestRecord, ModelToolOutcomeRecord, ToolOutcomeStatus,
            TurnOutcomeRecord,
        },
        protocol::EnvelopeHeader,
    };
    use async_trait::async_trait;
    use klaw_llm::{ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolDefinition};
    use klaw_tool::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolRegistry};
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use uuid::Uuid;

    #[test]
    fn heartbeat_metadata_is_passthrough_only_for_heartbeat_keys() {
        let inbound = BTreeMap::from([
            ("trigger.kind".to_string(), json!("heartbeat")),
            ("heartbeat.session_key".to_string(), json!("stdio:test")),
            ("reasoning".to_string(), json!("ignore")),
        ]);

        let metadata = heartbeat_response_metadata(&inbound);
        assert_eq!(metadata.get("trigger.kind"), Some(&json!("heartbeat")));
        assert_eq!(
            metadata.get("heartbeat.session_key"),
            Some(&json!("stdio:test"))
        );
        assert!(!metadata.contains_key("reasoning"));
    }

    #[test]
    fn current_attachment_contexts_extract_archive_metadata() {
        let attachments = current_attachment_contexts(&[crate::MediaReference {
            source_kind: crate::MediaSourceKind::ChannelInbound,
            filename: Some("report.pdf".to_string()),
            mime_type: Some("application/pdf".to_string()),
            remote_url: None,
            bytes: None,
            message_id: Some("msg-1".to_string()),
            metadata: BTreeMap::from([
                ("archive.id".to_string(), json!("arch-1")),
                (
                    "archive.storage_rel_path".to_string(),
                    json!("archives/2026-03-20/arch-1.pdf"),
                ),
                ("archive.size_bytes".to_string(), json!(1234)),
            ]),
        }]);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].get("archive_id"), Some(&json!("arch-1")));
        assert_eq!(
            attachments[0].get("storage_rel_path"),
            Some(&json!("archives/2026-03-20/arch-1.pdf"))
        );
        assert_eq!(attachments[0].get("access"), Some(&json!("read_only")));
    }

    #[test]
    fn augment_user_content_with_attachment_context_appends_read_only_guidance() {
        let content = augment_user_content_with_attachment_context(
            "Please summarize this file.",
            &[json!({
                "filename": "report.pdf",
                "archive_id": "arch-1",
                "storage_rel_path": "archives/2026-03-20/arch-1.pdf",
                "mime_type": "application/pdf",
                "size_bytes": 1234
            })],
        );
        assert!(content.contains("Current message attachments:"));
        assert!(content.contains("prefer calling the archive tool with action=get"));
        assert!(content.contains("archive_id=arch-1"));
        assert!(content.contains("use list_session_attachments"));
        assert!(content.contains("voice tool with action=stt"));
        assert!(content.contains("access=read_only archive"));
        assert!(content.contains("copy the file into workspace first"));
    }

    struct NamedProvider {
        id: String,
    }

    #[async_trait]
    impl LlmProvider for NamedProvider {
        fn name(&self) -> &str {
            &self.id
        }

        fn default_model(&self) -> &str {
            "default-model"
        }

        async fn chat(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                content: format!("provider={} model={}", self.id, model.unwrap_or("none")),
                reasoning: None,
                tool_calls: Vec::new(),
                usage: None,
                usage_source: None,
                audit: None,
            })
        }
    }

    #[derive(Default)]
    struct CaptureMediaProvider {
        user_media_count: Arc<Mutex<Option<usize>>>,
    }

    #[async_trait]
    impl LlmProvider for CaptureMediaProvider {
        fn name(&self) -> &str {
            "capture-media"
        }

        fn default_model(&self) -> &str {
            "capture-media-v1"
        }

        async fn chat(
            &self,
            messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let user_media = messages
                .iter()
                .find(|message| message.role == "user")
                .map(|message| message.media.len())
                .unwrap_or_default();
            *self.user_media_count.lock().expect("mutex poisoned") = Some(user_media);
            Ok(LlmResponse {
                content: "ok".to_string(),
                reasoning: None,
                tool_calls: Vec::new(),
                usage: None,
                usage_source: None,
                audit: None,
            })
        }
    }

    #[derive(Default)]
    struct ToolCallingProvider {
        call_count: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl LlmProvider for ToolCallingProvider {
        fn name(&self) -> &str {
            "tool-calling"
        }

        fn default_model(&self) -> &str {
            "tool-calling-v1"
        }

        async fn chat(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let mut call_count = self.call_count.lock().expect("mutex poisoned");
            *call_count += 1;
            if *call_count == 1 {
                return Ok(LlmResponse {
                    content: String::new(),
                    reasoning: None,
                    tool_calls: vec![klaw_llm::ToolCall {
                        id: Some("call_approval_1".to_string()),
                        name: "mock_approval".to_string(),
                        arguments: json!({}),
                    }],
                    usage: None,
                    usage_source: None,
                    audit: None,
                });
            }
            Ok(LlmResponse {
                content: "approval requested".to_string(),
                reasoning: None,
                tool_calls: Vec::new(),
                usage: None,
                usage_source: None,
                audit: None,
            })
        }
    }

    struct MockApprovalTool;

    struct MockStopTool;

    #[async_trait]
    impl Tool for MockApprovalTool {
        fn name(&self) -> &str {
            "mock_approval"
        }

        fn description(&self) -> &str {
            "mock approval tool"
        }

        fn parameters(&self) -> serde_json::Value {
            json!({"type":"object"})
        }

        fn category(&self) -> ToolCategory {
            ToolCategory::Messaging
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
            ctx: &ToolContext,
        ) -> Result<ToolOutput, ToolError> {
            Err(ToolError::structured_execution_failed(
                "approval required",
                "approval_required",
                Some(json!({
                    "approval_id": "approval-core-test",
                    "tool_name": "mock_approval",
                    "session_key": ctx.session_key,
                })),
                true,
                vec![klaw_tool::ToolSignal::approval_required(
                    "approval-core-test",
                    "mock_approval",
                    &ctx.session_key,
                    None,
                )],
            ))
        }
    }

    #[async_trait]
    impl Tool for MockStopTool {
        fn name(&self) -> &str {
            "mock_stop"
        }

        fn description(&self) -> &str {
            "mock stop tool"
        }

        fn parameters(&self) -> serde_json::Value {
            json!({"type":"object"})
        }

        fn category(&self) -> ToolCategory {
            ToolCategory::Messaging
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolOutput, ToolError> {
            Err(ToolError::structured_execution_failed(
                "stop requested",
                "stop_requested",
                None,
                false,
                vec![klaw_tool::ToolSignal::stop_current_turn(
                    Some("tool requested stop"),
                    Some("mock_stop"),
                )],
            ))
        }
    }

    #[tokio::test]
    async fn process_message_uses_provider_and_model_from_metadata() {
        let default_provider: Arc<dyn LlmProvider> = Arc::new(NamedProvider {
            id: "openai".to_string(),
        });
        let anthropic_provider: Arc<dyn LlmProvider> = Arc::new(NamedProvider {
            id: "anthropic".to_string(),
        });
        let agent = AgentLoop::new_with_identity(
            RunLimits {
                max_tool_iterations: 1,
                max_tool_calls: 1,
                token_budget: 0,
                agent_timeout: Duration::from_secs(1),
                tool_timeout: Duration::from_secs(1),
            },
            SessionSchedulingPolicy {
                strategy: QueueStrategy::Collect,
                max_queue_depth: 1,
                lock_ttl: Duration::from_secs(1),
            },
            default_provider,
            "openai".to_string(),
            "gpt-4o-mini".to_string(),
            ToolRegistry::default(),
        )
        .with_provider_registry(BTreeMap::from([
            (
                "openai".to_string(),
                Arc::new(NamedProvider {
                    id: "openai".to_string(),
                }) as Arc<dyn LlmProvider>,
            ),
            ("anthropic".to_string(), anthropic_provider),
        ]));

        let inbound = crate::protocol::Envelope {
            header: EnvelopeHeader::new("im:chat-1"),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "im".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "chat-1".to_string(),
                session_key: "im:chat-1".to_string(),
                content: "hello".to_string(),
                media_references: Vec::new(),
                metadata: BTreeMap::from([
                    ("agent.provider_id".to_string(), json!("anthropic")),
                    ("agent.model".to_string(), json!("claude-opus-4")),
                ]),
            },
        };
        let outcome = agent.process_message(inbound, false).await;
        let response = outcome.final_response.expect("response should be present");
        assert_eq!(
            response.payload.content,
            "provider=anthropic model=claude-opus-4"
        );
    }

    struct UsageProvider;

    #[async_trait]
    impl LlmProvider for UsageProvider {
        fn name(&self) -> &str {
            "usage-provider"
        }

        fn default_model(&self) -> &str {
            "usage-model"
        }

        fn wire_api(&self) -> Option<&str> {
            Some("responses")
        }

        async fn chat(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            Ok(LlmResponse {
                content: "ok".to_string(),
                reasoning: None,
                tool_calls: Vec::new(),
                usage: Some(klaw_llm::LlmUsage {
                    input_tokens: 11,
                    output_tokens: 5,
                    total_tokens: 16,
                    cached_input_tokens: Some(3),
                    reasoning_tokens: Some(2),
                    provider_request_id: None,
                    provider_response_id: Some("resp-usage".to_string()),
                }),
                usage_source: Some(klaw_llm::LlmUsageSource::ProviderReported),
                audit: None,
            })
        }
    }

    #[tokio::test]
    async fn process_message_propagates_llm_usage_to_outbound_metadata() {
        let provider = Arc::new(UsageProvider) as Arc<dyn LlmProvider>;
        let agent = AgentLoop::new_with_identity(
            RunLimits {
                max_tool_iterations: 1,
                max_tool_calls: 1,
                token_budget: 0,
                agent_timeout: Duration::from_secs(1),
                tool_timeout: Duration::from_secs(1),
            },
            SessionSchedulingPolicy {
                strategy: QueueStrategy::Collect,
                max_queue_depth: 1,
                lock_ttl: Duration::from_secs(1),
            },
            Arc::clone(&provider),
            "openai".to_string(),
            "gpt-4.1-mini".to_string(),
            ToolRegistry::default(),
        )
        .with_provider_registry(BTreeMap::from([("openai".to_string(), provider)]));

        let inbound = crate::protocol::Envelope {
            header: EnvelopeHeader::new("im:chat-usage"),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "im".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "chat-usage".to_string(),
                session_key: "im:chat-usage".to_string(),
                content: "hello".to_string(),
                media_references: Vec::new(),
                metadata: BTreeMap::new(),
            },
        };

        let outcome = agent.process_message(inbound, false).await;
        let response = outcome.final_response.expect("response should be present");
        let usage_records = response
            .payload
            .metadata
            .get("llm.usage.records")
            .and_then(serde_json::Value::as_array)
            .expect("usage records should be array");
        assert_eq!(usage_records.len(), 1);
        assert_eq!(usage_records[0].get("provider"), Some(&json!("openai")));
        assert_eq!(usage_records[0].get("wire_api"), Some(&json!("responses")));
        assert_eq!(usage_records[0].get("total_tokens"), Some(&json!(16)));
    }

    #[tokio::test]
    async fn process_message_only_forwards_image_media_to_provider() {
        let capture = Arc::new(CaptureMediaProvider::default());
        let provider = Arc::clone(&capture) as Arc<dyn LlmProvider>;
        let agent = AgentLoop::new_with_identity(
            RunLimits {
                max_tool_iterations: 1,
                max_tool_calls: 1,
                token_budget: 0,
                agent_timeout: Duration::from_secs(1),
                tool_timeout: Duration::from_secs(1),
            },
            SessionSchedulingPolicy {
                strategy: QueueStrategy::Collect,
                max_queue_depth: 1,
                lock_ttl: Duration::from_secs(1),
            },
            Arc::clone(&provider),
            "capture-media".to_string(),
            "capture-media-v1".to_string(),
            ToolRegistry::default(),
        )
        .with_provider_registry(BTreeMap::from([("capture-media".to_string(), provider)]));

        let inbound = crate::protocol::Envelope {
            header: EnvelopeHeader::new("im:chat-2"),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "im".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "chat-2".to_string(),
                session_key: "im:chat-2".to_string(),
                content: "hello".to_string(),
                media_references: vec![
                    crate::MediaReference {
                        source_kind: crate::MediaSourceKind::ChannelInbound,
                        filename: None,
                        mime_type: Some("image/png".to_string()),
                        remote_url: Some("data:image/png;base64,AAAA".to_string()),
                        bytes: None,
                        message_id: None,
                        metadata: BTreeMap::new(),
                    },
                    crate::MediaReference {
                        source_kind: crate::MediaSourceKind::ChannelInbound,
                        filename: None,
                        mime_type: Some("audio/wav".to_string()),
                        remote_url: Some("data:audio/wav;base64,AAAA".to_string()),
                        bytes: None,
                        message_id: None,
                        metadata: BTreeMap::new(),
                    },
                ],
                metadata: BTreeMap::new(),
            },
        };
        let _ = agent.process_message(inbound, false).await;
        let captured_count = *capture
            .user_media_count
            .lock()
            .expect("capture media mutex poisoned");
        assert_eq!(captured_count, Some(1));
    }

    #[tokio::test]
    async fn process_message_propagates_approval_signal_to_outbound_metadata() {
        let provider = Arc::new(ToolCallingProvider::default()) as Arc<dyn LlmProvider>;
        let mut tools = ToolRegistry::default();
        tools.register(MockApprovalTool);
        let agent = AgentLoop::new_with_identity(
            RunLimits {
                max_tool_iterations: 2,
                max_tool_calls: 2,
                token_budget: 0,
                agent_timeout: Duration::from_secs(1),
                tool_timeout: Duration::from_secs(1),
            },
            SessionSchedulingPolicy {
                strategy: QueueStrategy::Collect,
                max_queue_depth: 1,
                lock_ttl: Duration::from_secs(1),
            },
            Arc::clone(&provider),
            "tool-calling".to_string(),
            "tool-calling-v1".to_string(),
            tools,
        )
        .with_provider_registry(BTreeMap::from([("tool-calling".to_string(), provider)]));

        let inbound = crate::protocol::Envelope {
            header: EnvelopeHeader::new("im:chat-approval"),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "im".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "chat-approval".to_string(),
                session_key: "im:chat-approval".to_string(),
                content: "run tool".to_string(),
                media_references: Vec::new(),
                metadata: BTreeMap::new(),
            },
        };
        let outcome = agent.process_message(inbound, false).await;
        let response = outcome.final_response.expect("response should be present");
        assert_eq!(response.payload.content, "approval requested");
        assert_eq!(
            response
                .payload
                .metadata
                .get("approval.required")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            response
                .payload
                .metadata
                .get("approval.id")
                .and_then(serde_json::Value::as_str),
            Some("approval-core-test")
        );
    }

    #[derive(Default)]
    struct StopToolCallingProvider {
        call_count: Mutex<u32>,
    }

    #[async_trait]
    impl LlmProvider for StopToolCallingProvider {
        fn name(&self) -> &str {
            "stop-calling"
        }

        fn default_model(&self) -> &str {
            "stop-calling-v1"
        }

        async fn chat(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let mut call_count = self.call_count.lock().expect("mutex poisoned");
            *call_count += 1;
            if *call_count > 1 {
                panic!("provider should not be called again after stop signal");
            }
            Ok(LlmResponse {
                content: String::new(),
                reasoning: None,
                tool_calls: vec![klaw_llm::ToolCall {
                    id: Some("call_stop_1".to_string()),
                    name: "mock_stop".to_string(),
                    arguments: json!({}),
                }],
                usage: None,
                usage_source: None,
                audit: None,
            })
        }
    }

    #[tokio::test]
    async fn process_message_propagates_stop_signal_to_outbound_metadata() {
        let provider = Arc::new(StopToolCallingProvider::default()) as Arc<dyn LlmProvider>;
        let mut tools = ToolRegistry::default();
        tools.register(MockStopTool);
        let agent = AgentLoop::new_with_identity(
            RunLimits {
                max_tool_iterations: 2,
                max_tool_calls: 2,
                token_budget: 0,
                agent_timeout: Duration::from_secs(1),
                tool_timeout: Duration::from_secs(1),
            },
            SessionSchedulingPolicy {
                strategy: QueueStrategy::Collect,
                max_queue_depth: 1,
                lock_ttl: Duration::from_secs(1),
            },
            Arc::clone(&provider),
            "stop-calling".to_string(),
            "stop-calling-v1".to_string(),
            tools,
        )
        .with_provider_registry(BTreeMap::from([("stop-calling".to_string(), provider)]));

        let inbound = crate::protocol::Envelope {
            header: EnvelopeHeader::new("im:chat-stop"),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "im".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "chat-stop".to_string(),
                session_key: "im:chat-stop".to_string(),
                content: "stop tool".to_string(),
                media_references: Vec::new(),
                metadata: BTreeMap::new(),
            },
        };
        let outcome = agent.process_message(inbound, false).await;
        let response = outcome.final_response.expect("response should be present");
        assert_eq!(
            response.payload.content,
            "Current turn stopped. No further tool calls were made."
        );
        assert_eq!(
            response
                .payload
                .metadata
                .get("turn.stopped")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            response.payload.metadata.get("turn.stop_signal"),
            Some(&json!({
                "reason": "tool requested stop",
                "source": "mock_stop"
            }))
        );
    }

    #[derive(Default)]
    struct CaptureTelemetry {
        model_requests: Mutex<Vec<ModelRequestRecord>>,
        model_tool_outcomes: Mutex<Vec<ModelToolOutcomeRecord>>,
        turn_outcomes: Mutex<Vec<TurnOutcomeRecord>>,
    }

    #[async_trait]
    impl AgentTelemetry for CaptureTelemetry {
        async fn record_tool_outcome(
            &self,
            _session_key: &str,
            _tool_name: &str,
            _status: ToolOutcomeStatus,
            _error_code: Option<&str>,
            _duration: Duration,
        ) {
        }

        async fn record_model_request(&self, record: ModelRequestRecord) {
            self.model_requests
                .lock()
                .expect("model request mutex poisoned")
                .push(record);
        }

        async fn record_model_tool_outcome(&self, record: ModelToolOutcomeRecord) {
            self.model_tool_outcomes
                .lock()
                .expect("model tool outcome mutex poisoned")
                .push(record);
        }

        async fn record_turn_outcome(&self, record: TurnOutcomeRecord) {
            self.turn_outcomes
                .lock()
                .expect("turn outcome mutex poisoned")
                .push(record);
        }

        async fn incr_counter(&self, _name: &'static str, _labels: &[(&str, &str)], _value: u64) {}

        async fn observe_histogram(
            &self,
            _name: &'static str,
            _labels: &[(&str, &str)],
            _duration: Duration,
        ) {
        }

        async fn emit_audit_event(
            &self,
            _event_name: &'static str,
            _trace_id: Uuid,
            _payload: serde_json::Value,
        ) {
        }

        async fn set_health(&self, _component: &'static str, _status: crate::HealthStatus) {}
    }

    #[tokio::test]
    async fn process_message_records_model_and_turn_observability() {
        let provider = Arc::new(UsageProvider) as Arc<dyn LlmProvider>;
        let telemetry = Arc::new(CaptureTelemetry::default());
        let mut tools = ToolRegistry::default();
        tools.register(MockApprovalTool);
        let agent = AgentLoop::new_with_identity(
            RunLimits {
                max_tool_iterations: 2,
                max_tool_calls: 2,
                token_budget: 0,
                agent_timeout: Duration::from_secs(1),
                tool_timeout: Duration::from_secs(1),
            },
            SessionSchedulingPolicy {
                strategy: QueueStrategy::Collect,
                max_queue_depth: 1,
                lock_ttl: Duration::from_secs(1),
            },
            Arc::clone(&provider),
            "openai".to_string(),
            "gpt-4.1-mini".to_string(),
            tools,
        )
        .with_provider_registry(BTreeMap::from([("openai".to_string(), provider)]))
        .with_telemetry(telemetry.clone());

        let inbound = crate::protocol::Envelope {
            header: EnvelopeHeader::new("im:chat-observe"),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "im".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "chat-observe".to_string(),
                session_key: "im:chat-observe".to_string(),
                content: "hello".to_string(),
                media_references: Vec::new(),
                metadata: BTreeMap::new(),
            },
        };

        let _ = agent.process_message(inbound, false).await;
        let model_requests = telemetry
            .model_requests
            .lock()
            .expect("model request mutex poisoned")
            .clone();
        assert_eq!(model_requests.len(), 1);
        assert_eq!(model_requests[0].provider, "openai");
        assert_eq!(model_requests[0].model, "gpt-4.1-mini");
        assert_eq!(model_requests[0].total_tokens, 16);

        let turn_outcomes = telemetry
            .turn_outcomes
            .lock()
            .expect("turn outcome mutex poisoned")
            .clone();
        assert_eq!(turn_outcomes.len(), 1);
        assert!(turn_outcomes[0].completed);
        assert_eq!(turn_outcomes[0].requests_in_turn, 1);
    }
}
