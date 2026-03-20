mod estimate;
mod providers;

use async_trait::async_trait;
use estimate::estimate_chat_usage;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc::UnboundedSender;

pub use providers::{
    anthropic::{AnthropicConfig, AnthropicProvider},
    openai_compatible::{OpenAiCompatibleConfig, OpenAiCompatibleProvider, OpenAiWireApi},
};

/// LLM 对话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    /// 消息角色（system/user/assistant/tool）。
    pub role: String,
    /// 消息文本内容。
    pub content: String,
    /// 用户消息携带的媒体 URL（例如 https://... 或 data: URL）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<LlmMedia>,
    /// assistant 角色发起的工具调用（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// tool 角色消息对应的工具调用 id（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMedia {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub url: String,
}

/// 暴露给模型的工具定义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// 工具名称。
    pub name: String,
    /// 工具描述。
    pub description: String,
    /// JSON Schema 参数定义。
    pub parameters: serde_json::Value,
}

/// 聊天调用参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatOptions {
    /// 采样温度。
    pub temperature: f32,
    /// 最大生成 token（可选）。
    pub max_tokens: Option<u32>,
    /// Responses API 输出 token 上限（可选，优先于 max_tokens）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Responses API 复用上轮 response id（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// Responses API 指令字段（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Responses API 元数据（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    /// Responses API include 参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Responses API 是否持久化结果（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    /// Responses API 是否并行 tool calls（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    /// Responses API tool_choice 参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    /// Responses API text 参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<serde_json::Value>,
    /// Responses API reasoning 参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<serde_json::Value>,
    /// Responses API truncation 参数（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,
    /// OpenAI user 字段（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// OpenAI service_tier 字段（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

/// 模型请求工具调用的描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// 工具调用 id（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 工具名。
    pub name: String,
    /// 工具参数。
    pub arguments: serde_json::Value,
}

/// 模型响应对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// 文本回复。
    pub content: String,
    /// 可选的推理内容（部分模型提供）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// 模型要求执行的工具调用列表。
    pub tool_calls: Vec<ToolCall>,
    /// provider 返回的 token 用量（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LlmUsage>,
    /// token 用量来源（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_source: Option<LlmUsageSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmStreamEvent {
    ContentDelta(String),
    ReasoningDelta(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmUsageSource {
    ProviderReported,
    EstimatedLocal,
}

impl LlmUsageSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderReported => "provider_reported",
            Self::EstimatedLocal => "estimated_local",
        }
    }
}

/// LLM 层错误。
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("stream failed: {0}")]
    StreamFailed(String),
}

/// LLM Provider 统一抽象。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider 名称。
    fn name(&self) -> &str;
    /// 默认模型名。
    fn default_model(&self) -> &str;
    /// provider 使用的底层 wire API。
    fn wire_api(&self) -> Option<&str> {
        None
    }
    /// 可选的本地 tokenizer 文件路径。
    fn tokenizer_path(&self) -> Option<&str> {
        None
    }

    /// 单轮聊天调用接口。
    async fn chat(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
    ) -> Result<LlmResponse, LlmError>;

    async fn chat_stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
        stream: Option<UnboundedSender<LlmStreamEvent>>,
    ) -> Result<LlmResponse, LlmError> {
        let response = self.chat(messages, tools, model, options).await?;
        if let Some(stream) = stream {
            if !response.content.is_empty() {
                let _ = stream.send(LlmStreamEvent::ContentDelta(response.content.clone()));
            }
            if let Some(reasoning) = response
                .reasoning
                .as_ref()
                .filter(|value| !value.is_empty())
            {
                let _ = stream.send(LlmStreamEvent::ReasoningDelta(reasoning.clone()));
            }
        }
        Ok(response)
    }
}

/// 本地回显 Provider，主要用于联调。
#[derive(Debug, Default)]
pub struct EchoProvider;

#[async_trait]
impl LlmProvider for EchoProvider {
    fn name(&self) -> &str {
        "echo"
    }

    fn default_model(&self) -> &str {
        "echo-v1"
    }

    fn wire_api(&self) -> Option<&str> {
        Some("echo")
    }

    async fn chat(
        &self,
        messages: Vec<LlmMessage>,
        _tools: Vec<ToolDefinition>,
        _model: Option<&str>,
        _options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let content = messages
            .last()
            .map(|m| format!("EchoProvider: {}", m.content))
            .unwrap_or_else(|| "EchoProvider: <empty>".to_string());
        let usage = estimate_chat_usage(
            self.name(),
            self.default_model(),
            self.wire_api().unwrap_or("echo"),
            self.tokenizer_path(),
            &messages,
            &[],
            &content,
            None,
            &[],
        );

        Ok(LlmResponse {
            content,
            reasoning: None,
            tool_calls: Vec::new(),
            usage: Some(usage),
            usage_source: Some(LlmUsageSource::EstimatedLocal),
        })
    }
}
