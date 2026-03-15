use crate::{
    domain::{DeadLetterMessage, InboundMessage, OutboundMessage},
    protocol::{Envelope, ErrorCode, MessageTopic},
    reliability::{
        idempotency_key, CircuitBreaker, DeadLetterPolicy, IdempotencyStore, RetryDecision,
        RetryPolicy,
    },
    transport::{MessageTransport, Subscription, TransportAckHandle, TransportError},
};
use async_trait::async_trait;
use klaw_agent::{
    run_agent_execution, AgentExecutionError, AgentExecutionInput, AgentExecutionLimits,
    ConversationMessage, ToolExecutor,
};
use klaw_llm::{LlmError, LlmMedia, LlmProvider, ToolDefinition};
use klaw_tool::{ToolContext, ToolRegistry};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

const META_SYSTEM_PROMPT_KEY: &str = "agent.system_prompt";
const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";
const TOOL_RESULT_LOG_LIMIT: usize = 4000;

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
}

#[derive(Debug, Error)]
pub enum AgentRuntimeError {
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
}

pub struct AgentLoop {
    pub limits: RunLimits,
    pub scheduling: SessionSchedulingPolicy,
    pub provider: Arc<dyn LlmProvider>,
    pub provider_registry: BTreeMap<String, Arc<dyn LlmProvider>>,
    pub active_provider_id: String,
    pub active_model: String,
    pub tools: ToolRegistry,
    pub system_prompt: Option<String>,
}

struct RegistryToolExecutor<'a> {
    tools: &'a ToolRegistry,
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
    ) -> String {
        let Some(tool) = self.tools.get(name) else {
            return format!("tool `{}` not found", name);
        };
        info!(tool = name, arguments = %arguments, "calling tool");

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
                output.content_for_model
            }
            Err(err) => {
                let message = format!("tool `{}` failed: {}", name, err);
                debug!(
                    tool = name,
                    result = %truncate_for_log(&message, TOOL_RESULT_LOG_LIMIT),
                    "tool result"
                );
                message
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
            provider: provider.clone(),
            provider_registry: BTreeMap::from([("default".to_string(), provider)]),
            active_provider_id: "default".to_string(),
            active_model: "default".to_string(),
            tools,
            system_prompt: None,
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
            provider: provider.clone(),
            provider_registry: BTreeMap::from([(active_provider_id.clone(), provider)]),
            active_provider_id,
            active_model,
            tools,
            system_prompt: None,
        }
    }

    pub fn with_provider_registry(
        mut self,
        provider_registry: BTreeMap<String, Arc<dyn LlmProvider>>,
    ) -> Self {
        self.provider_registry = provider_registry;
        self
    }

    pub fn with_system_prompt(mut self, system_prompt: Option<String>) -> Self {
        self.system_prompt = system_prompt
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
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
        info!(message_id = %msg.header.message_id, "process message");
        if msg.payload.content.trim().is_empty() {
            return ProcessOutcome {
                final_response: None,
                error_code: Some(ErrorCode::ValidationFailed),
                final_state: AgentRunState::Failed,
            };
        }

        let mut state = AgentRunState::Received;
        state = self.transition(state, StateTransitionEvent::StartValidation);
        state = self.transition(state, StateTransitionEvent::ValidationPassed);
        state = self.transition(state, StateTransitionEvent::Scheduled);
        state = self.transition(state, StateTransitionEvent::ContextBuilt);

        let conversation_history = extract_conversation_history(&msg.payload.metadata);
        let user_media = extract_user_media(&msg.payload);
        let requested_provider_id = msg
            .payload
            .metadata
            .get("agent.provider_id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| self.active_provider_id.clone());
        let (resolved_provider_id, provider) =
            if let Some(provider) = self.provider_registry.get(&requested_provider_id) {
                (requested_provider_id, Arc::clone(provider))
            } else {
                warn!(
                    requested_provider = requested_provider_id.as_str(),
                    fallback_provider = self.active_provider_id.as_str(),
                    "provider override not found, falling back to active provider"
                );
                (self.active_provider_id.clone(), Arc::clone(&self.provider))
            };
        let resolved_model = msg
            .payload
            .metadata
            .get("agent.model")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| self.active_model.clone());

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
        if let Some(system_prompt) = &self.system_prompt {
            tool_metadata
                .entry(META_SYSTEM_PROMPT_KEY.to_string())
                .or_insert_with(|| serde_json::Value::String(system_prompt.clone()));
        }

        state = self.transition(state, StateTransitionEvent::ModelCalled);
        state = self.transition(state, StateTransitionEvent::ToolRequested);
        let executor = RegistryToolExecutor { tools: &self.tools };
        let result = run_agent_execution(
            provider.as_ref(),
            &executor,
            AgentExecutionInput {
                user_content: msg.payload.content.clone(),
                user_media,
                conversation_history,
                session_key: msg.payload.session_key.clone(),
                tool_metadata,
                model: Some(resolved_model),
            },
            AgentExecutionLimits {
                max_tool_iterations: self.limits.max_tool_iterations,
                max_tool_calls: self.limits.max_tool_calls,
            },
        )
        .await;
        state = self.transition(state, StateTransitionEvent::ToolLoopFinished);

        match result {
            Ok(output) => {
                state = self.transition(state, StateTransitionEvent::FinalResponseReady);
                state = self.transition(state, StateTransitionEvent::Published);
                let mut response_metadata = heartbeat_response_metadata(&msg.payload.metadata);
                if let Some(reasoning) = output.reasoning.filter(|value| !value.trim().is_empty()) {
                    response_metadata.insert(
                        "reasoning".to_string(),
                        serde_json::Value::String(reasoning),
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
                }
            }
            Err(AgentExecutionError::Provider(err)) => {
                warn!(error = %err, "provider failed");
                ProcessOutcome {
                    final_response: None,
                    error_code: Some(map_llm_error_to_code(&err)),
                    final_state: AgentRunState::Degraded,
                }
            }
            Err(AgentExecutionError::ToolLoopExhausted) => ProcessOutcome {
                final_response: None,
                error_code: Some(ErrorCode::RetryExhausted),
                final_state: AgentRunState::Failed,
            },
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
            RetryDecision::RetryNow => Ok(None),
            RetryDecision::RetryAfter(delay) => {
                sleep(delay).await;
                Ok(None)
            }
            RetryDecision::Abort => {
                inbound_transport.ack(ack_handle).await?;
                Ok(Some(ProcessOutcome {
                    final_response: None,
                    error_code: Some(ErrorCode::RetryExhausted),
                    final_state: AgentRunState::Failed,
                }))
            }
            RetryDecision::SendToDeadLetter => {
                error!(attempt, "send to dlq");
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
        LlmError::ProviderUnavailable(_) | LlmError::RequestFailed(_) => {
            ErrorCode::ProviderUnavailable
        }
        LlmError::InvalidResponse(_) => ErrorCode::ProviderResponseInvalid,
    }
}

fn heartbeat_response_metadata(
    inbound_metadata: &BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    let mut response_metadata = BTreeMap::new();
    for (key, value) in inbound_metadata {
        if key == "trigger.kind" || key.starts_with("heartbeat.") {
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
            Some(LlmMedia { mime_type, url })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        heartbeat_response_metadata, AgentLoop, QueueStrategy, RunLimits, SessionSchedulingPolicy,
    };
    use crate::{domain::InboundMessage, protocol::EnvelopeHeader};
    use async_trait::async_trait;
    use klaw_llm::{ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolDefinition};
    use klaw_tool::ToolRegistry;
    use serde_json::json;
    use std::{collections::BTreeMap, sync::Arc, time::Duration};

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
            })
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
}
