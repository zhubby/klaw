use async_trait::async_trait;
use reqwest::{header::USER_AGENT, Client};
use serde::{Deserialize, Serialize};

use crate::{
    ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolCall, ToolDefinition,
};

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
                    role: m.role.clone(),
                    content: if m.role == "assistant"
                        && m.tool_calls.is_some()
                        && m.content.is_empty()
                    {
                        None
                    } else {
                        Some(m.content)
                    },
                    reasoning_content: None,
                    reasoning: None,
                    tool_calls: m.tool_calls.map(|calls| {
                        calls
                            .into_iter()
                            .map(|call| OpenAiToolCall {
                                id: call.id,
                                r#type: "function".to_string(),
                                function: OpenAiToolCallFunction {
                                    name: call.name,
                                    arguments: call.arguments.to_string(),
                                },
                            })
                            .collect()
                    }),
                    tool_call_id: m.tool_call_id,
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
            .header(USER_AGENT, "openclaw/0.3.0")
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
        let reasoning = first
            .message
            .reasoning_content
            .or(first.message.reasoning)
            .filter(|value| !value.trim().is_empty());
        let tool_calls = first
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|call| {
                let args: serde_json::Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or_else(|_| serde_json::Value::String(call.function.arguments));
                ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: args,
                }
            })
            .collect();

        Ok(LlmResponse {
            content,
            reasoning,
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
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<String>,
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
    id: Option<String>,
    #[serde(rename = "type")]
    r#type: String,
    function: OpenAiToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiToolCallFunction {
    name: String,
    arguments: String,
}
