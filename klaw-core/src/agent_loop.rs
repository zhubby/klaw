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
    ToolExecutor,
};
use klaw_llm::{LlmError, LlmProvider, ToolDefinition};
use klaw_tool::{ToolContext, ToolRegistry};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::time::sleep;
use tracing::{error, info, warn};

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
    pub active_provider_id: String,
    pub active_model: String,
    pub tools: ToolRegistry,
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
            Ok(output) => output.content_for_model,
            Err(err) => format!("tool `{}` failed: {}", name, err),
        }
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
            provider,
            active_provider_id: "default".to_string(),
            active_model: "default".to_string(),
            tools,
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
            provider,
            active_provider_id,
            active_model,
            tools,
        }
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

        let mut tool_metadata = msg.payload.metadata.clone();
        tool_metadata
            .entry("agent.provider_id".to_string())
            .or_insert_with(|| serde_json::Value::String(self.active_provider_id.clone()));
        tool_metadata
            .entry("agent.model".to_string())
            .or_insert_with(|| serde_json::Value::String(self.active_model.clone()));
        tool_metadata.insert(
            "agent.parent_session_key".to_string(),
            serde_json::Value::String(msg.payload.session_key.clone()),
        );

        state = self.transition(state, StateTransitionEvent::ModelCalled);
        state = self.transition(state, StateTransitionEvent::ToolRequested);
        let executor = RegistryToolExecutor { tools: &self.tools };
        let result = run_agent_execution(
            self.provider.as_ref(),
            &executor,
            AgentExecutionInput {
                user_content: msg.payload.content.clone(),
                session_key: msg.payload.session_key.clone(),
                tool_metadata,
                model: Some(self.active_model.clone()),
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
                let mut response_metadata = BTreeMap::new();
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
