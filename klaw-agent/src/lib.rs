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
    let mut llm_messages = vec![LlmMessage {
        role: "user".to_string(),
        content: input.user_content,
    }];
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
            });
        }

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
