use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// LLM 对话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    /// 消息角色（system/user/assistant/tool）。
    pub role: String,
    /// 消息文本内容。
    pub content: String,
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
}

/// 模型请求工具调用的描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
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
    /// 模型要求执行的工具调用列表。
    pub tool_calls: Vec<ToolCall>,
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
}

/// LLM Provider 统一抽象。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Provider 名称。
    fn name(&self) -> &str;
    /// 默认模型名。
    fn default_model(&self) -> &str;

    /// 单轮聊天调用接口。
    async fn chat(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
    ) -> Result<LlmResponse, LlmError>;
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

        Ok(LlmResponse {
            content,
            tool_calls: Vec::new(),
        })
    }
}

/// OpenAI Compatible 配置。
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    /// 兼容网关基础地址（例如 `https://api.openai.com/v1`）。
    pub base_url: String,
    /// API 密钥。
    pub api_key: String,
    /// 默认模型名。
    pub default_model: String,
}

/// OpenAI Compatible Provider 实现。
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    client: Client,
    config: OpenAiCompatibleConfig,
}

impl OpenAiCompatibleProvider {
    /// 创建 provider 实例。
    pub fn new(config: OpenAiCompatibleConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        "openai-compatible"
    }

    fn default_model(&self) -> &str {
        &self.config.default_model
    }

    async fn chat(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let request = OpenAiChatCompletionRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            temperature: options.temperature,
            max_tokens: options.max_tokens,
            messages: messages
                .into_iter()
                .map(|m| OpenAiMessage {
                    role: m.role,
                    content: Some(m.content),
                    tool_calls: None,
                    tool_call_id: None,
                })
                .collect(),
            tools: if tools.is_empty() {
                None
            } else {
                Some(
                    tools
                        .into_iter()
                        .map(|tool| OpenAiToolDefinition {
                            r#type: "function".to_string(),
                            function: OpenAiFunctionDefinition {
                                name: tool.name,
                                description: tool.description,
                                parameters: tool.parameters,
                            },
                        })
                        .collect(),
                )
            },
        };

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(LlmError::ProviderUnavailable(format!(
                "http_status={status}, body={body}"
            )));
        }

        let payload: OpenAiChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| LlmError::InvalidResponse(e.to_string()))?;

        let first = payload
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvalidResponse("no choices in response".to_string()))?;

        let content = first.message.content.unwrap_or_default();
        let tool_calls = first
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|call| {
                let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or_else(|_| serde_json::Value::String(call.function.arguments));
                ToolCall {
                    name: call.function.name,
                    arguments: args,
                }
            })
            .collect();

        Ok(LlmResponse {
            content,
            tool_calls,
        })
    }
}

#[derive(Debug, Serialize)]
struct OpenAiChatCompletionRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDefinition>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiToolDefinition {
    r#type: String,
    function: OpenAiFunctionDefinition,
}

#[derive(Debug, Serialize)]
struct OpenAiFunctionDefinition {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatCompletionResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiToolCall {
    #[serde(rename = "id")]
    _id: Option<String>,
    #[serde(rename = "type")]
    _type: String,
    function: OpenAiToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiToolCallFunction {
    name: String,
    arguments: String,
}
