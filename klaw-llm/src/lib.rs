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

/// LLM chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    /// Message role (system/user/assistant/tool).
    pub role: String,
    /// Message text content.
    pub content: String,
    /// Media URLs carried by user messages (e.g., https://... or data: URL).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<LlmMedia>,
    /// Tool calls initiated by the assistant role (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Tool call ID corresponding to the tool role message (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMedia {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub url: String,
}

/// Tool definition exposed to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
    /// JSON Schema parameter definition.
    pub parameters: serde_json::Value,
}

/// Chat invocation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatOptions {
    /// Sampling temperature.
    pub temperature: f32,
    /// Maximum generated tokens (optional).
    pub max_tokens: Option<u32>,
    /// Responses API output token limit (optional, takes precedence over max_tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Responses API reuse previous response ID (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    /// Responses API instructions field (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Responses API metadata (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    /// Responses API include parameter (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    /// Responses API whether to persist results (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    /// Responses API whether to allow parallel tool calls (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    /// Responses API tool_choice parameter (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    /// Responses API text parameter (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<serde_json::Value>,
    /// Responses API reasoning parameter (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<serde_json::Value>,
    /// Responses API truncation parameter (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,
    /// OpenAI user field (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// OpenAI service_tier field (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

/// Description of a tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Tool call ID (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Tool name.
    pub name: String,
    /// Tool arguments.
    pub arguments: serde_json::Value,
}

/// Model response object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// Text response.
    pub content: String,
    /// Optional reasoning content (provided by some models).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Tool calls requested by the model for execution.
    pub tool_calls: Vec<ToolCall>,
    /// Token usage returned by the provider (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<LlmUsage>,
    /// Token usage source (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_source: Option<LlmUsageSource>,
    /// Request/response audit information captured at the provider boundary (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit: Option<LlmAuditPayload>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmAuditStatus {
    Success,
    Failed,
}

impl LlmAuditStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditPayload {
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub status: LlmAuditStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
    pub request_body: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body: Option<serde_json::Value>,
    pub requested_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responded_at_ms: Option<i64>,
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

/// LLM layer error.
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider unavailable: {message}")]
    ProviderUnavailable {
        message: String,
        audit: Option<Box<LlmAuditPayload>>,
    },
    #[error("invalid response: {message}")]
    InvalidResponse {
        message: String,
        audit: Option<Box<LlmAuditPayload>>,
    },
    #[error("request failed: {message}")]
    RequestFailed {
        message: String,
        audit: Option<Box<LlmAuditPayload>>,
    },
    #[error("stream failed: {message}")]
    StreamFailed {
        message: String,
        audit: Option<Box<LlmAuditPayload>>,
    },
}

impl LlmError {
    #[must_use]
    pub fn provider_unavailable(message: impl Into<String>) -> Self {
        Self::ProviderUnavailable {
            message: message.into(),
            audit: None,
        }
    }

    #[must_use]
    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self::InvalidResponse {
            message: message.into(),
            audit: None,
        }
    }

    #[must_use]
    pub fn request_failed(message: impl Into<String>) -> Self {
        Self::RequestFailed {
            message: message.into(),
            audit: None,
        }
    }

    #[must_use]
    pub fn stream_failed(message: impl Into<String>) -> Self {
        Self::StreamFailed {
            message: message.into(),
            audit: None,
        }
    }

    #[must_use]
    pub fn with_audit(self, audit_payload: LlmAuditPayload) -> Self {
        match self {
            Self::ProviderUnavailable { message, .. } => Self::ProviderUnavailable {
                message,
                audit: Some(Box::new(audit_payload)),
            },
            Self::InvalidResponse { message, .. } => Self::InvalidResponse {
                message,
                audit: Some(Box::new(audit_payload)),
            },
            Self::RequestFailed { message, .. } => Self::RequestFailed {
                message,
                audit: Some(Box::new(audit_payload)),
            },
            Self::StreamFailed { message, .. } => Self::StreamFailed {
                message,
                audit: Some(Box::new(audit_payload)),
            },
        }
    }

    #[must_use]
    pub fn audit(&self) -> Option<&LlmAuditPayload> {
        match self {
            Self::ProviderUnavailable { audit, .. }
            | Self::InvalidResponse { audit, .. }
            | Self::RequestFailed { audit, .. }
            | Self::StreamFailed { audit, .. } => audit.as_deref(),
        }
    }
}

/// Unified LLM provider abstraction.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider name.
    fn name(&self) -> &str;
    /// Default model name.
    fn default_model(&self) -> &str;
    /// Underlying wire API used by the provider.
    fn wire_api(&self) -> Option<&str> {
        None
    }
    /// Optional local tokenizer file path.
    fn tokenizer_path(&self) -> Option<&str> {
        None
    }

    /// Single-turn chat invocation interface.
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

/// Local echo provider, primarily used for integration testing.
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
            audit: None,
        })
    }
}
