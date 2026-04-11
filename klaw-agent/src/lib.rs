mod context_compression;

use async_trait::async_trait;
use klaw_config::{AppConfig, ModelProviderConfig};
use klaw_llm::{
    ChatOptions, LlmAuditPayload, LlmError, LlmMedia, LlmMessage, LlmProvider, LlmStreamEvent,
    LlmUsage, LlmUsageSource, OpenAiCompatibleConfig, OpenAiCompatibleProvider, OpenAiWireApi,
    ToolDefinition,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use thiserror::Error;
use time::OffsetDateTime;
use tokio::sync::mpsc::{self, UnboundedSender};
use tracing::{trace, warn};

pub use context_compression::{
    ConversationSummary, build_compression_prompt, merge_or_reset_summary,
    parse_conversation_summary,
};

const META_SYSTEM_PROMPT_KEY: &str = "agent.system_prompt";
const META_TOOL_CHOICE_KEY: &str = "agent.tool_choice";
const META_PROVIDER_KEY: &str = "agent.provider_id";
const META_MODEL_KEY: &str = "agent.model";
const META_PARENT_SESSION_KEY: &str = "agent.parent_session_key";
const META_MESSAGE_ID_KEY: &str = "agent.message_id";
const META_CURRENT_ATTACHMENTS_KEY: &str = "agent.current_attachments";
const APPROVAL_REQUIRED_SIGNAL: &str = "approval_required";
const STOP_SIGNAL: &str = "stop";
const STOPPED_TURN_MESSAGE: &str = "Current turn stopped. No further tool calls were made.";
const FINAL_ITERATION_PROMPT: &str = "You are about to reach the maximum tool call limit. This is your final iteration. Please respond directly to the user with a summary of what you have accomplished so far, and explain that you have reached the tool call limit. Do NOT call any more tools in this response.";

#[derive(Debug, Clone, Copy)]
pub struct AgentExecutionLimits {
    pub max_tool_iterations: u32,
    pub max_tool_calls: u32,
    pub token_budget: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Default)]
pub struct AgentExecutionContext {
    pub system_prompt: Option<String>,
    pub tool_choice: Option<Value>,
    pub provider_id: Option<String>,
    pub resolved_model: Option<String>,
    pub parent_session_key: Option<String>,
    pub message_id: Option<String>,
    pub current_attachments: Vec<Value>,
    pub tool_metadata: BTreeMap<String, Value>,
}

impl AgentExecutionContext {
    #[must_use]
    pub fn merged_tool_metadata(&self) -> BTreeMap<String, Value> {
        let mut metadata = self.tool_metadata.clone();
        if let Some(system_prompt) = self.system_prompt.clone() {
            metadata.insert(
                META_SYSTEM_PROMPT_KEY.to_string(),
                Value::String(system_prompt),
            );
        }
        if let Some(tool_choice) = self.tool_choice.clone().filter(|value| !value.is_null()) {
            metadata.insert(META_TOOL_CHOICE_KEY.to_string(), tool_choice);
        }
        if let Some(provider_id) = self.provider_id.clone() {
            metadata.insert(META_PROVIDER_KEY.to_string(), Value::String(provider_id));
        }
        if let Some(resolved_model) = self.resolved_model.clone() {
            metadata.insert(META_MODEL_KEY.to_string(), Value::String(resolved_model));
        }
        if let Some(parent_session_key) = self.parent_session_key.clone() {
            metadata.insert(
                META_PARENT_SESSION_KEY.to_string(),
                Value::String(parent_session_key),
            );
        }
        if let Some(message_id) = self.message_id.clone() {
            metadata.insert(META_MESSAGE_ID_KEY.to_string(), Value::String(message_id));
        }
        if !self.current_attachments.is_empty() {
            metadata.insert(
                META_CURRENT_ATTACHMENTS_KEY.to_string(),
                Value::Array(self.current_attachments.clone()),
            );
        }
        metadata
    }
}

#[derive(Debug, Clone)]
pub struct AgentExecutionInput {
    pub user_content: String,
    pub user_media: Vec<LlmMedia>,
    pub conversation_history: Vec<ConversationMessage>,
    pub session_key: String,
    pub execution_context: AgentExecutionContext,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentExecutionDisposition {
    FinalMessage,
    ApprovalRequired,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct AgentExecutionOutput {
    pub content: String,
    pub reasoning: Option<String>,
    pub disposition: AgentExecutionDisposition,
    pub tool_signals: Vec<ToolInvocationSignal>,
    pub request_usages: Vec<AgentRequestUsage>,
    pub request_audits: Vec<AgentRequestAudit>,
    pub tool_audits: Vec<AgentToolAudit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentExecutionStreamEvent {
    Snapshot {
        content: String,
        reasoning: Option<String>,
    },
    Clear,
}

#[derive(Debug, Clone)]
pub struct AgentRequestUsage {
    pub request_seq: i64,
    pub usage: LlmUsage,
    pub source: LlmUsageSource,
}

#[derive(Debug, Clone)]
pub struct AgentRequestAudit {
    pub request_seq: i64,
    pub payload: LlmAuditPayload,
}

#[derive(Debug, Clone)]
pub struct AgentToolAudit {
    pub request_seq: i64,
    pub tool_call_seq: i64,
    pub tool_call_id: Option<String>,
    pub tool_name: String,
    pub arguments: Value,
    pub result: ToolInvocationResult,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolInvocationSignal {
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct ToolInvocationResult {
    pub ok: bool,
    pub content_for_model: String,
    pub error_code: Option<String>,
    pub error_details: Option<Value>,
    pub retryable: Option<bool>,
    pub signals: Vec<ToolInvocationSignal>,
}

impl ToolInvocationResult {
    #[must_use]
    pub fn success(content_for_model: String) -> Self {
        Self {
            ok: true,
            content_for_model,
            error_code: None,
            error_details: None,
            retryable: None,
            signals: Vec::new(),
        }
    }

    #[must_use]
    pub fn success_with_signals(
        content_for_model: String,
        signals: Vec<ToolInvocationSignal>,
    ) -> Self {
        Self {
            ok: true,
            content_for_model,
            error_code: None,
            error_details: None,
            retryable: None,
            signals,
        }
    }

    #[must_use]
    pub fn error(
        content_for_model: String,
        error_code: String,
        error_details: Option<Value>,
        retryable: bool,
        signals: Vec<ToolInvocationSignal>,
    ) -> Self {
        Self {
            ok: false,
            content_for_model,
            error_code: Some(error_code),
            error_details,
            retryable: Some(retryable),
            signals,
        }
    }

    #[must_use]
    pub fn to_tool_message_content(&self, tool_name: &str) -> String {
        let mut envelope = json!({
            "ok": self.ok,
            "tool": tool_name,
            "content": self.content_for_model,
        });
        if let Some(code) = self.error_code.as_ref() {
            envelope["error"] = json!({
                "code": code,
                "details": self.error_details,
                "retryable": self.retryable,
            });
        }
        if !self.signals.is_empty() {
            envelope["signals"] = serde_json::to_value(&self.signals).unwrap_or(Value::Null);
        }
        serde_json::to_string(&envelope).unwrap_or_else(|_| self.content_for_model.clone())
    }
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn definitions(&self) -> Vec<ToolDefinition>;

    async fn execute(
        &self,
        name: &str,
        arguments: Value,
        session_key: &str,
        metadata: &BTreeMap<String, Value>,
    ) -> ToolInvocationResult;
}

#[derive(Debug, Error)]
pub enum AgentExecutionError {
    #[error("provider failed: {0}")]
    Provider(#[from] LlmError),
    #[error("tool loop exhausted")]
    ToolLoopExhausted,
    #[error("token budget exceeded: used {used_tokens} > budget {token_budget}")]
    BudgetExceeded { used_tokens: u64, token_budget: u64 },
}

#[derive(Debug, Error)]
pub enum ProviderBuildError {
    #[error("provider `{0}` not found")]
    ProviderNotFound(String),
    #[error("provider `{0}` requires api_key or env_key")]
    MissingApiKey(String),
    #[error("unsupported wire_api `{wire_api}` for provider `{provider_id}`")]
    UnsupportedWireApi {
        provider_id: String,
        wire_api: String,
    },
}

pub struct ProviderInstance {
    pub provider: Arc<dyn LlmProvider>,
    pub default_model: String,
}

pub fn build_provider_from_config(
    config: &AppConfig,
    provider_id: &str,
) -> Result<ProviderInstance, ProviderBuildError> {
    let provider = config
        .model_providers
        .get(provider_id)
        .ok_or_else(|| ProviderBuildError::ProviderNotFound(provider_id.to_string()))?;

    match provider.wire_api.as_str() {
        "chat_completions" | "responses" => {}
        other => {
            return Err(ProviderBuildError::UnsupportedWireApi {
                provider_id: provider_id.to_string(),
                wire_api: other.to_string(),
            });
        }
    }

    let api_key = resolve_api_key(provider)
        .ok_or_else(|| ProviderBuildError::MissingApiKey(provider_id.to_string()))?;
    let default_model = provider.default_model.clone();

    Ok(ProviderInstance {
        provider: Arc::new(OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
            base_url: provider.base_url.clone(),
            api_key,
            default_model: default_model.clone(),
            tokenizer_path: provider.tokenizer_path.clone(),
            proxy: provider.proxy,
            wire_api: OpenAiWireApi::parse(provider.wire_api.as_str()).ok_or_else(|| {
                ProviderBuildError::UnsupportedWireApi {
                    provider_id: provider_id.to_string(),
                    wire_api: provider.wire_api.clone(),
                }
            })?,
            stream: provider.stream,
        })),
        default_model,
    })
}

pub async fn run_agent_execution(
    provider: &dyn LlmProvider,
    tools: &dyn ToolExecutor,
    input: AgentExecutionInput,
    limits: AgentExecutionLimits,
    stream: Option<UnboundedSender<AgentExecutionStreamEvent>>,
) -> Result<AgentExecutionOutput, AgentExecutionError> {
    let tool_defs = tools.definitions();
    let mut llm_messages = Vec::new();
    if let Some(system_prompt) = input.execution_context.system_prompt.clone() {
        llm_messages.push(LlmMessage {
            role: "system".to_string(),
            content: system_prompt,
            media: Vec::new(),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    llm_messages.extend(
        input
            .conversation_history
            .into_iter()
            .filter(|message| is_supported_history_role(&message.role))
            .map(|message| LlmMessage {
                role: message.role,
                content: message.content,
                media: Vec::new(),
                tool_calls: None,
                tool_call_id: None,
            }),
    );
    llm_messages.push(LlmMessage {
        role: "user".to_string(),
        content: input.user_content,
        media: input.user_media,
        tool_calls: None,
        tool_call_id: None,
    });
    let mut tool_calls_used = 0u32;
    let mut tool_signals = Vec::new();
    let mut request_usages = Vec::new();
    let mut request_audits = Vec::new();
    let mut tool_audits = Vec::new();
    let mut tokens_used = 0u64;

    let max_tool_iterations = limits.max_tool_iterations;
    let mut iteration = 0u32;
    loop {
        if max_tool_iterations > 0 && iteration >= max_tool_iterations {
            break;
        }
        iteration = iteration.saturating_add(1);
        let is_final_allowed_iteration =
            max_tool_iterations > 0 && iteration == max_tool_iterations;
        let should_warn_final_iteration = is_final_allowed_iteration
            && max_tool_iterations >= 3
            && !llm_messages.iter().any(|m| {
                m.role == "system"
                    && m.content
                        .contains("You are about to reach the maximum tool call limit")
            });
        let final_iteration_messages: Option<Vec<LlmMessage>> = if should_warn_final_iteration {
            let mut msgs = llm_messages.clone();
            msgs.push(LlmMessage {
                role: "system".to_string(),
                content: FINAL_ITERATION_PROMPT.to_string(),
                media: Vec::new(),
                tool_calls: None,
                tool_call_id: None,
            });
            Some(msgs)
        } else {
            None
        };
        let messages_for_request: &Vec<LlmMessage> =
            final_iteration_messages.as_ref().unwrap_or(&llm_messages);
        let chat_options = ChatOptions {
            temperature: 0.2,
            max_tokens: None,
            max_output_tokens: None,
            previous_response_id: None,
            instructions: None,
            metadata: None,
            include: None,
            store: None,
            parallel_tool_calls: None,
            tool_choice: input.execution_context.tool_choice.clone(),
            text: None,
            reasoning: None,
            truncation: None,
            user: None,
            service_tier: None,
        };
        trace!(
            iteration,
            session_key = %input.session_key,
            model_override = ?input.execution_context.resolved_model,
            messages = ?llm_messages,
            tools = ?tool_defs,
            options = ?chat_options,
            "sending chat request to model provider"
        );
        let tool_metadata = input.execution_context.merged_tool_metadata();
        let mut stream_forwarder = None;
        let llm_stream = stream.as_ref().map(|agent_stream| {
            let (llm_tx, mut llm_rx) = mpsc::unbounded_channel::<LlmStreamEvent>();
            let agent_stream = agent_stream.clone();
            stream_forwarder = Some(tokio::spawn(async move {
                let mut content = String::new();
                let mut reasoning = String::new();
                while let Some(event) = llm_rx.recv().await {
                    match event {
                        LlmStreamEvent::ContentDelta(delta) => content.push_str(&delta),
                        LlmStreamEvent::ReasoningDelta(delta) => reasoning.push_str(&delta),
                    }
                    let _ = agent_stream.send(AgentExecutionStreamEvent::Snapshot {
                        content: content.clone(),
                        reasoning: (!reasoning.trim().is_empty()).then_some(reasoning.clone()),
                    });
                }
            }));
            llm_tx
        });
        let llm_response = provider
            .chat_stream(
                messages_for_request.clone(),
                tool_defs.clone(),
                input.execution_context.resolved_model.as_deref(),
                chat_options,
                llm_stream,
            )
            .await?;
        if let Some(forwarder) = stream_forwarder.take() {
            let _ = forwarder.await;
        }
        let request_seq = i64::from(iteration);
        if let Some(usage) = llm_response.usage.clone() {
            tokens_used = tokens_used.saturating_add(usage.total_tokens);
            request_usages.push(AgentRequestUsage {
                request_seq,
                usage,
                source: llm_response
                    .usage_source
                    .unwrap_or(LlmUsageSource::ProviderReported),
            });
            if limits.token_budget > 0 && tokens_used > limits.token_budget {
                warn!(
                    token_budget = limits.token_budget,
                    tokens_used,
                    iteration,
                    session_key = %input.session_key,
                    "agent token budget exceeded"
                );
                return Err(AgentExecutionError::BudgetExceeded {
                    used_tokens: tokens_used,
                    token_budget: limits.token_budget,
                });
            }
        }
        if let Some(audit) = llm_response.audit.clone() {
            request_audits.push(AgentRequestAudit {
                request_seq,
                payload: audit,
            });
        }

        if llm_response.tool_calls.is_empty() {
            return Ok(AgentExecutionOutput {
                content: llm_response.content,
                reasoning: llm_response.reasoning,
                disposition: AgentExecutionDisposition::FinalMessage,
                tool_signals,
                request_usages,
                request_audits,
                tool_audits,
            });
        }
        if let Some(stream) = &stream {
            let _ = stream.send(AgentExecutionStreamEvent::Clear);
        }

        let assistant_content = llm_response.content.clone();
        llm_messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: assistant_content.clone(),
            media: Vec::new(),
            tool_calls: Some(llm_response.tool_calls.clone()),
            tool_call_id: None,
        });

        if let Some(short_circuit) = apply_tool_calls(
            tools,
            &mut llm_messages,
            llm_response.tool_calls,
            &input.session_key,
            &tool_metadata,
            &mut tool_calls_used,
            limits.max_tool_calls,
            &mut tool_signals,
            request_seq,
            &mut tool_audits,
        )
        .await?
        {
            let approval_required = short_circuit
                .signals
                .iter()
                .any(|signal| signal.kind == APPROVAL_REQUIRED_SIGNAL);
            let stopped = short_circuit
                .signals
                .iter()
                .any(|signal| signal.kind == STOP_SIGNAL);
            return Ok(AgentExecutionOutput {
                content: if approval_required && !assistant_content.trim().is_empty() {
                    assistant_content
                } else if stopped {
                    STOPPED_TURN_MESSAGE.to_string()
                } else {
                    short_circuit.content_for_model
                },
                reasoning: llm_response.reasoning,
                disposition: if approval_required {
                    AgentExecutionDisposition::ApprovalRequired
                } else {
                    AgentExecutionDisposition::Stopped
                },
                tool_signals,
                request_usages,
                request_audits,
                tool_audits,
            });
        }
    }

    warn!(
        max_tool_iterations,
        iterations_used = iteration,
        tool_calls_used,
        max_tool_calls = limits.max_tool_calls,
        "agent tool loop exhausted by iteration limit"
    );
    Err(AgentExecutionError::ToolLoopExhausted)
}

fn is_supported_history_role(role: &str) -> bool {
    matches!(role, "system" | "user" | "assistant" | "tool")
}

async fn apply_tool_calls(
    tools: &dyn ToolExecutor,
    llm_messages: &mut Vec<LlmMessage>,
    tool_calls: Vec<klaw_llm::ToolCall>,
    session_key: &str,
    tool_metadata: &BTreeMap<String, Value>,
    tool_calls_used: &mut u32,
    max_tool_calls: u32,
    collected_signals: &mut Vec<ToolInvocationSignal>,
    request_seq: i64,
    collected_audits: &mut Vec<AgentToolAudit>,
) -> Result<Option<ToolInvocationResult>, AgentExecutionError> {
    for (tool_call_index, call) in tool_calls.into_iter().enumerate() {
        *tool_calls_used += 1;
        if max_tool_calls > 0 && *tool_calls_used > max_tool_calls {
            warn!(
                tool_calls_used = *tool_calls_used,
                max_tool_calls, "agent tool loop exhausted by tool call limit"
            );
            return Err(AgentExecutionError::ToolLoopExhausted);
        }
        let arguments = call.arguments.clone();
        let started_at_ms = now_ms();
        let result = tools
            .execute(&call.name, arguments.clone(), session_key, tool_metadata)
            .await;
        let finished_at_ms = now_ms();
        collected_audits.push(AgentToolAudit {
            request_seq,
            tool_call_seq: (tool_call_index as i64) + 1,
            tool_call_id: call.id.clone(),
            tool_name: call.name.clone(),
            arguments,
            result: result.clone(),
            started_at_ms,
            finished_at_ms,
        });
        let approval_required = result
            .signals
            .iter()
            .any(|signal| signal.kind == APPROVAL_REQUIRED_SIGNAL);
        let stop_requested = result
            .signals
            .iter()
            .any(|signal| signal.kind == STOP_SIGNAL);
        collected_signals.extend(result.signals.clone());
        llm_messages.push(LlmMessage {
            role: "tool".to_string(),
            content: result.to_tool_message_content(&call.name),
            media: Vec::new(),
            tool_calls: None,
            tool_call_id: call.id,
        });
        if approval_required || stop_requested {
            return Ok(Some(result));
        }
    }

    Ok(None)
}

fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

pub fn resolve_api_key(provider: &ModelProviderConfig) -> Option<String> {
    provider.api_key.clone().or_else(|| {
        provider
            .env_key
            .as_ref()
            .and_then(|env_name| env::var(env_name).ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use klaw_config::AppConfig;
    use klaw_llm::{LlmResponse, LlmUsage, ToolCall};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockToolExecutor;

    #[async_trait]
    impl ToolExecutor for MockToolExecutor {
        fn definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "echo_tool".to_string(),
                description: "echo".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        async fn execute(
            &self,
            _name: &str,
            _arguments: Value,
            _session_key: &str,
            _metadata: &BTreeMap<String, Value>,
        ) -> ToolInvocationResult {
            ToolInvocationResult::success("tool-result".to_string())
        }
    }

    fn test_execution_context() -> AgentExecutionContext {
        AgentExecutionContext::default()
    }

    #[derive(Default)]
    struct SequencedProvider {
        call_count: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl LlmProvider for SequencedProvider {
        fn name(&self) -> &str {
            "sequenced"
        }

        fn default_model(&self) -> &str {
            "sequenced-v1"
        }

        async fn chat(
            &self,
            messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let mut count = self.call_count.lock().expect("mutex poisoned");
            *count += 1;

            if *count == 1 {
                return Ok(LlmResponse {
                    content: String::new(),
                    reasoning: None,
                    tool_calls: vec![ToolCall {
                        id: Some("call_123".to_string()),
                        name: "echo_tool".to_string(),
                        arguments: serde_json::json!({}),
                    }],
                    usage: Some(LlmUsage {
                        input_tokens: 10,
                        output_tokens: 1,
                        total_tokens: 11,
                        cached_input_tokens: None,
                        reasoning_tokens: None,
                        provider_request_id: None,
                        provider_response_id: Some("resp-1".to_string()),
                    }),
                    usage_source: Some(LlmUsageSource::ProviderReported),
                    audit: None,
                });
            }

            assert_eq!(messages.len(), 3);
            assert_eq!(messages[0].role, "user");
            assert_eq!(messages[1].role, "assistant");
            assert!(messages[1].tool_calls.is_some());
            assert_eq!(messages[2].role, "tool");
            assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_123"));

            Ok(LlmResponse {
                content: "done".to_string(),
                reasoning: Some("inner chain".to_string()),
                tool_calls: Vec::new(),
                usage: Some(LlmUsage {
                    input_tokens: 15,
                    output_tokens: 4,
                    total_tokens: 19,
                    cached_input_tokens: Some(2),
                    reasoning_tokens: Some(1),
                    provider_request_id: None,
                    provider_response_id: Some("resp-2".to_string()),
                }),
                usage_source: Some(LlmUsageSource::ProviderReported),
                audit: None,
            })
        }
    }

    #[tokio::test]
    async fn run_agent_execution_preserves_tool_call_sequence_for_provider() {
        let provider = SequencedProvider::default();
        let tools = MockToolExecutor;
        let output = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "hello".to_string(),
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: "s1".to_string(),
                execution_context: test_execution_context(),
            },
            AgentExecutionLimits {
                max_tool_iterations: 4,
                max_tool_calls: 4,
                token_budget: 0,
            },
            None,
        )
        .await
        .expect("agent execution should succeed");

        assert_eq!(output.content, "done");
        assert_eq!(output.reasoning.as_deref(), Some("inner chain"));
        assert_eq!(output.disposition, AgentExecutionDisposition::FinalMessage);
        assert_eq!(output.request_usages.len(), 2);
        assert_eq!(output.tool_audits.len(), 1);
        assert_eq!(output.tool_audits[0].request_seq, 1);
        assert_eq!(output.tool_audits[0].tool_call_seq, 1);
        assert_eq!(output.tool_audits[0].tool_name, "echo_tool");
        assert_eq!(
            output.tool_audits[0].result.content_for_model,
            "tool-result"
        );
        assert_eq!(output.request_usages[0].request_seq, 1);
        assert_eq!(output.request_usages[0].usage.total_tokens, 11);
        assert_eq!(output.request_usages[1].request_seq, 2);
        assert_eq!(output.request_usages[1].usage.total_tokens, 19);
    }

    #[tokio::test]
    async fn run_agent_execution_treats_zero_limits_as_unbounded() {
        let provider = SequencedProvider::default();
        let tools = MockToolExecutor;
        let output = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "hello".to_string(),
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: "s1".to_string(),
                execution_context: test_execution_context(),
            },
            AgentExecutionLimits {
                max_tool_iterations: 0,
                max_tool_calls: 0,
                token_budget: 0,
            },
            None,
        )
        .await
        .expect("agent execution should succeed with unbounded limits");

        assert_eq!(output.content, "done");
        assert_eq!(output.disposition, AgentExecutionDisposition::FinalMessage);
    }

    #[derive(Default)]
    struct CaptureFirstMessageProvider {
        first_message_role: Arc<Mutex<Option<String>>>,
        tool_choice: Arc<Mutex<Option<Value>>>,
    }

    #[async_trait]
    impl LlmProvider for CaptureFirstMessageProvider {
        fn name(&self) -> &str {
            "capture"
        }

        fn default_model(&self) -> &str {
            "capture-v1"
        }

        async fn chat(
            &self,
            messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let first_role = messages.first().map(|m| m.role.clone());
            *self
                .first_message_role
                .lock()
                .expect("mutex poisoned for first role") = first_role;
            *self
                .tool_choice
                .lock()
                .expect("mutex poisoned for tool choice") = options.tool_choice;
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

    #[tokio::test]
    async fn run_agent_execution_includes_system_prompt_from_context() {
        let provider = CaptureFirstMessageProvider::default();
        let tools = MockToolExecutor;
        let _ = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "hello".to_string(),
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: "s1".to_string(),
                execution_context: AgentExecutionContext {
                    system_prompt: Some("skill context".to_string()),
                    ..test_execution_context()
                },
            },
            AgentExecutionLimits {
                max_tool_iterations: 2,
                max_tool_calls: 2,
                token_budget: 0,
            },
            None,
        )
        .await
        .expect("agent execution should succeed");

        let first_role = provider
            .first_message_role
            .lock()
            .expect("mutex poisoned for assert")
            .clone();
        assert_eq!(first_role.as_deref(), Some("system"));
    }

    #[tokio::test]
    async fn run_agent_execution_includes_tool_choice_from_context() {
        let provider = CaptureFirstMessageProvider::default();
        let tools = MockToolExecutor;
        let _ = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "hello".to_string(),
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: "s1".to_string(),
                execution_context: AgentExecutionContext {
                    tool_choice: Some(Value::String("required".to_string())),
                    ..test_execution_context()
                },
            },
            AgentExecutionLimits {
                max_tool_iterations: 2,
                max_tool_calls: 2,
                token_budget: 0,
            },
            None,
        )
        .await
        .expect("agent execution should succeed");

        let tool_choice = provider
            .tool_choice
            .lock()
            .expect("mutex poisoned for tool choice assert")
            .clone();
        assert_eq!(tool_choice, Some(Value::String("required".to_string())));
    }

    #[derive(Default)]
    struct CaptureConversationProvider {
        messages: Arc<Mutex<Vec<LlmMessage>>>,
    }

    #[async_trait]
    impl LlmProvider for CaptureConversationProvider {
        fn name(&self) -> &str {
            "capture-conversation"
        }

        fn default_model(&self) -> &str {
            "capture-conversation-v1"
        }

        async fn chat(
            &self,
            messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            *self.messages.lock().expect("mutex poisoned") = messages;
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

    #[tokio::test]
    async fn run_agent_execution_includes_prior_conversation_before_current_user_turn() {
        let provider = CaptureConversationProvider::default();
        let tools = MockToolExecutor;
        let _ = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "current".to_string(),
                user_media: Vec::new(),
                conversation_history: vec![
                    ConversationMessage {
                        role: "user".to_string(),
                        content: "previous user".to_string(),
                    },
                    ConversationMessage {
                        role: "assistant".to_string(),
                        content: "previous assistant".to_string(),
                    },
                ],
                session_key: "s1".to_string(),
                execution_context: test_execution_context(),
            },
            AgentExecutionLimits {
                max_tool_iterations: 1,
                max_tool_calls: 1,
                token_budget: 0,
            },
            None,
        )
        .await
        .expect("agent execution should succeed");

        let messages = provider.messages.lock().expect("mutex poisoned").clone();
        let summary: Vec<(&str, &str)> = messages
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("user", "previous user"),
                ("assistant", "previous assistant"),
                ("user", "current"),
            ]
        );
    }

    #[tokio::test]
    async fn run_agent_execution_stops_when_token_budget_is_exceeded() {
        let provider = SequencedProvider::default();
        let tools = MockToolExecutor;
        let err = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "hello".to_string(),
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: "s1".to_string(),
                execution_context: test_execution_context(),
            },
            AgentExecutionLimits {
                max_tool_iterations: 4,
                max_tool_calls: 4,
                token_budget: 20,
            },
            None,
        )
        .await
        .expect_err("token budget should be enforced");

        match err {
            AgentExecutionError::BudgetExceeded {
                used_tokens,
                token_budget,
            } => {
                assert_eq!(used_tokens, 30);
                assert_eq!(token_budget, 20);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[derive(Default)]
    struct ApprovalShortCircuitProvider {
        call_count: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl LlmProvider for ApprovalShortCircuitProvider {
        fn name(&self) -> &str {
            "approval-short-circuit"
        }

        fn default_model(&self) -> &str {
            "approval-short-circuit-v1"
        }

        async fn chat(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let mut count = self.call_count.lock().expect("mutex poisoned");
            *count += 1;
            if *count > 1 {
                panic!("provider should not be called again after approval_required");
            }
            Ok(LlmResponse {
                content: String::new(),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: Some("call_approval".to_string()),
                    name: "approval_tool".to_string(),
                    arguments: serde_json::json!({}),
                }],
                usage: None,
                usage_source: None,
                audit: None,
            })
        }
    }

    #[derive(Default)]
    struct ApprovalRequiredToolExecutor {
        call_count: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl ToolExecutor for ApprovalRequiredToolExecutor {
        fn definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "approval_tool".to_string(),
                description: "requires approval".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        async fn execute(
            &self,
            _name: &str,
            _arguments: Value,
            _session_key: &str,
            _metadata: &BTreeMap<String, Value>,
        ) -> ToolInvocationResult {
            let mut count = self.call_count.lock().expect("mutex poisoned");
            *count += 1;
            ToolInvocationResult::error(
                "tool `approval_tool` failed: approval required: approval_id=test-approval"
                    .to_string(),
                "approval_required".to_string(),
                None,
                true,
                vec![ToolInvocationSignal {
                    kind: APPROVAL_REQUIRED_SIGNAL.to_string(),
                    payload: serde_json::json!({
                        "approval_id": "test-approval",
                        "tool_name": "shell"
                    }),
                }],
            )
        }
    }

    #[tokio::test]
    async fn run_agent_execution_stops_after_approval_required_signal() {
        let provider = ApprovalShortCircuitProvider::default();
        let tools = ApprovalRequiredToolExecutor::default();
        let output = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "run something risky".to_string(),
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: "s1".to_string(),
                execution_context: test_execution_context(),
            },
            AgentExecutionLimits {
                max_tool_iterations: 4,
                max_tool_calls: 4,
                token_budget: 0,
            },
            None,
        )
        .await
        .expect("agent execution should short-circuit successfully");

        assert!(output.content.contains("approval required"));
        assert_eq!(
            output.disposition,
            AgentExecutionDisposition::ApprovalRequired
        );
        assert_eq!(output.tool_signals.len(), 1);
        assert_eq!(output.tool_signals[0].kind, APPROVAL_REQUIRED_SIGNAL);
        assert_eq!(
            *provider.call_count.lock().expect("mutex poisoned"),
            1,
            "provider should only be called once"
        );
        assert_eq!(
            *tools.call_count.lock().expect("mutex poisoned"),
            1,
            "tool should only be called once"
        );
    }

    #[derive(Default)]
    struct StopShortCircuitProvider {
        call_count: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl LlmProvider for StopShortCircuitProvider {
        fn name(&self) -> &str {
            "stop-provider"
        }

        fn default_model(&self) -> &str {
            "stop-model"
        }

        async fn chat(
            &self,
            _messages: Vec<LlmMessage>,
            _tools: Vec<ToolDefinition>,
            _model: Option<&str>,
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let mut count = self.call_count.lock().expect("mutex poisoned");
            *count += 1;
            if *count > 1 {
                panic!("provider should not be called again after stop signal");
            }
            Ok(LlmResponse {
                content: String::new(),
                reasoning: None,
                tool_calls: vec![ToolCall {
                    id: Some("call_stop".to_string()),
                    name: "stop_tool".to_string(),
                    arguments: serde_json::json!({}),
                }],
                usage: None,
                usage_source: None,
                audit: None,
            })
        }
    }

    #[derive(Default)]
    struct StopSignalToolExecutor {
        call_count: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl ToolExecutor for StopSignalToolExecutor {
        fn definitions(&self) -> Vec<ToolDefinition> {
            vec![ToolDefinition {
                name: "stop_tool".to_string(),
                description: "stops current turn".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }]
        }

        async fn execute(
            &self,
            _name: &str,
            _arguments: Value,
            _session_key: &str,
            _metadata: &BTreeMap<String, Value>,
        ) -> ToolInvocationResult {
            let mut count = self.call_count.lock().expect("mutex poisoned");
            *count += 1;
            ToolInvocationResult::error(
                "stop requested".to_string(),
                "stop_requested".to_string(),
                None,
                false,
                vec![ToolInvocationSignal {
                    kind: STOP_SIGNAL.to_string(),
                    payload: serde_json::json!({
                        "reason": "tool requested stop",
                        "source": "stop_tool"
                    }),
                }],
            )
        }
    }

    #[tokio::test]
    async fn run_agent_execution_stops_after_stop_signal() {
        let provider = StopShortCircuitProvider::default();
        let tools = StopSignalToolExecutor::default();
        let output = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "stop this turn".to_string(),
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: "s1".to_string(),
                execution_context: test_execution_context(),
            },
            AgentExecutionLimits {
                max_tool_iterations: 4,
                max_tool_calls: 4,
                token_budget: 0,
            },
            None,
        )
        .await
        .expect("agent execution should short-circuit successfully");

        assert_eq!(output.content, STOPPED_TURN_MESSAGE);
        assert_eq!(output.disposition, AgentExecutionDisposition::Stopped);
        assert_eq!(output.tool_signals.len(), 1);
        assert_eq!(output.tool_signals[0].kind, STOP_SIGNAL);
        assert_eq!(
            *provider.call_count.lock().expect("mutex poisoned"),
            1,
            "provider should only be called once"
        );
        assert_eq!(
            *tools.call_count.lock().expect("mutex poisoned"),
            1,
            "tool should only be called once"
        );
    }

    #[test]
    fn build_provider_from_config_supports_responses_wire_api() {
        let mut config = AppConfig::default();
        let provider = config
            .model_providers
            .get_mut(&config.model_provider)
            .unwrap_or_else(|| panic!("missing default provider '{}'", config.model_provider));
        provider.wire_api = "responses".to_string();
        provider.api_key = Some("test-key".to_string());
        provider.env_key = None;

        let built = build_provider_from_config(&config, &config.model_provider);
        assert!(built.is_ok());
    }

    #[test]
    fn build_provider_from_config_uses_provider_default_model() {
        let mut config = AppConfig::default();
        let provider = config
            .model_providers
            .get_mut(&config.model_provider)
            .unwrap_or_else(|| panic!("missing default provider '{}'", config.model_provider));
        provider.default_model = "gpt-4o-mini".to_string();
        provider.api_key = Some("test-key".to_string());
        provider.env_key = None;

        let built = build_provider_from_config(&config, &config.model_provider)
            .expect("provider should build");
        assert_eq!(built.default_model, "gpt-4o-mini");
        assert_eq!(built.provider.default_model(), "gpt-4o-mini");
    }
}
