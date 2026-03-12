use async_trait::async_trait;
use klaw_config::{AppConfig, ModelProviderConfig};
use klaw_llm::{
    ChatOptions, LlmError, LlmMessage, LlmProvider, OpenAiCompatibleConfig,
    OpenAiCompatibleProvider, ToolDefinition,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use thiserror::Error;

const META_SYSTEM_PROMPT_KEY: &str = "agent.system_prompt";

#[derive(Debug, Clone, Copy)]
pub struct AgentExecutionLimits {
    pub max_tool_iterations: u32,
    pub max_tool_calls: u32,
}

#[derive(Debug, Clone)]
pub struct AgentExecutionInput {
    pub user_content: String,
    pub session_key: String,
    pub tool_metadata: BTreeMap<String, Value>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentExecutionOutput {
    pub content: String,
    pub reasoning: Option<String>,
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
    ) -> String;
}

#[derive(Debug, Error)]
pub enum AgentExecutionError {
    #[error("provider failed: {0}")]
    Provider(#[from] LlmError),
    #[error("tool loop exhausted")]
    ToolLoopExhausted,
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

    Ok(ProviderInstance {
        provider: Arc::new(OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
            base_url: provider.base_url.clone(),
            api_key,
            default_model: provider.default_model.clone(),
        })),
        default_model: provider.default_model.clone(),
    })
}

pub async fn run_agent_execution(
    provider: &dyn LlmProvider,
    tools: &dyn ToolExecutor,
    input: AgentExecutionInput,
    limits: AgentExecutionLimits,
) -> Result<AgentExecutionOutput, AgentExecutionError> {
    let tool_defs = tools.definitions();
    let mut llm_messages = Vec::new();
    if let Some(system_prompt) = extract_system_prompt(&input.tool_metadata) {
        llm_messages.push(LlmMessage {
            role: "system".to_string(),
            content: system_prompt,
            tool_calls: None,
            tool_call_id: None,
        });
    }
    llm_messages.push(LlmMessage {
        role: "user".to_string(),
        content: input.user_content,
        tool_calls: None,
        tool_call_id: None,
    });
    let mut tool_calls_used = 0u32;

    for _ in 0..limits.max_tool_iterations.max(1) {
        let llm_response = provider
            .chat(
                llm_messages.clone(),
                tool_defs.clone(),
                input.model.as_deref(),
                ChatOptions {
                    temperature: 0.2,
                    max_tokens: None,
                },
            )
            .await?;

        if llm_response.tool_calls.is_empty() {
            return Ok(AgentExecutionOutput {
                content: llm_response.content,
                reasoning: llm_response.reasoning,
            });
        }

        llm_messages.push(LlmMessage {
            role: "assistant".to_string(),
            content: llm_response.content,
            tool_calls: Some(llm_response.tool_calls.clone()),
            tool_call_id: None,
        });

        apply_tool_calls(
            tools,
            &mut llm_messages,
            llm_response.tool_calls,
            &input.session_key,
            &input.tool_metadata,
            &mut tool_calls_used,
            limits.max_tool_calls,
        )
        .await?;
    }

    Err(AgentExecutionError::ToolLoopExhausted)
}

fn extract_system_prompt(metadata: &BTreeMap<String, Value>) -> Option<String> {
    metadata
        .get(META_SYSTEM_PROMPT_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

async fn apply_tool_calls(
    tools: &dyn ToolExecutor,
    llm_messages: &mut Vec<LlmMessage>,
    tool_calls: Vec<klaw_llm::ToolCall>,
    session_key: &str,
    tool_metadata: &BTreeMap<String, Value>,
    tool_calls_used: &mut u32,
    max_tool_calls: u32,
) -> Result<(), AgentExecutionError> {
    for call in tool_calls {
        *tool_calls_used += 1;
        if *tool_calls_used > max_tool_calls {
            return Err(AgentExecutionError::ToolLoopExhausted);
        }
        let content = tools
            .execute(&call.name, call.arguments, session_key, tool_metadata)
            .await;
        llm_messages.push(LlmMessage {
            role: "tool".to_string(),
            content,
            tool_calls: None,
            tool_call_id: call.id,
        });
    }

    Ok(())
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
    use klaw_llm::{LlmResponse, ToolCall};
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
        ) -> String {
            "tool-result".to_string()
        }
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
                session_key: "s1".to_string(),
                tool_metadata: BTreeMap::new(),
                model: None,
            },
            AgentExecutionLimits {
                max_tool_iterations: 4,
                max_tool_calls: 4,
            },
        )
        .await
        .expect("agent execution should succeed");

        assert_eq!(output.content, "done");
        assert_eq!(output.reasoning.as_deref(), Some("inner chain"));
    }

    #[derive(Default)]
    struct CaptureFirstMessageProvider {
        first_message_role: Arc<Mutex<Option<String>>>,
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
            _options: ChatOptions,
        ) -> Result<LlmResponse, LlmError> {
            let first_role = messages.first().map(|m| m.role.clone());
            *self
                .first_message_role
                .lock()
                .expect("mutex poisoned for first role") = first_role;
            Ok(LlmResponse {
                content: "ok".to_string(),
                reasoning: None,
                tool_calls: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn run_agent_execution_includes_system_prompt_from_metadata() {
        let provider = CaptureFirstMessageProvider::default();
        let tools = MockToolExecutor;
        let mut metadata = BTreeMap::new();
        metadata.insert(
            META_SYSTEM_PROMPT_KEY.to_string(),
            Value::String("skill context".to_string()),
        );
        let _ = run_agent_execution(
            &provider,
            &tools,
            AgentExecutionInput {
                user_content: "hello".to_string(),
                session_key: "s1".to_string(),
                tool_metadata: metadata,
                model: None,
            },
            AgentExecutionLimits {
                max_tool_iterations: 2,
                max_tool_calls: 2,
            },
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
}
