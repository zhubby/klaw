use async_trait::async_trait;
use reqwest::{header::USER_AGENT, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolCall, ToolDefinition,
};

/// Anthropic 配置。
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// Anthropic API 基础地址（例如 `https://api.anthropic.com/v1`）。
    pub base_url: String,
    /// API 密钥。
    pub api_key: String,
    /// 默认模型名。
    pub default_model: String,
    /// Anthropic API 版本头。
    pub api_version: String,
}

/// Anthropic Provider 实现。
#[derive(Debug, Clone)]
pub struct AnthropicProvider {
    client: Client,
    config: AnthropicConfig,
}

impl AnthropicProvider {
    /// 创建 provider 实例。
    pub fn new(config: AnthropicConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/messages", self.config.base_url.trim_end_matches('/'))
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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
        let mut system_parts = Vec::new();
        let mut anthropic_messages = Vec::new();

        for message in messages {
            if message.role == "system" {
                system_parts.push(message.content);
                continue;
            }

            let role = match message.role.as_str() {
                "assistant" => "assistant",
                _ => "user",
            }
            .to_string();

            anthropic_messages.push(AnthropicMessage {
                role,
                content: vec![AnthropicRequestContentBlock::Text {
                    text: message.content,
                }],
            });
        }

        let request = AnthropicMessagesRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            system: if system_parts.is_empty() {
                None
            } else {
                Some(system_parts.join("\n\n"))
            },
            messages: anthropic_messages,
            temperature: Some(options.temperature),
            max_tokens: options.max_tokens.unwrap_or(1024),
            tools: if tools.is_empty() {
                None
            } else {
                Some(
                    tools
                        .into_iter()
                        .map(|tool| AnthropicToolDefinition {
                            name: tool.name,
                            description: tool.description,
                            input_schema: tool.parameters,
                        })
                        .collect(),
                )
            },
        };

        let response = self
            .client
            .post(self.endpoint())
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.api_version)
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

        let payload: AnthropicMessagesResponse = response
            .json()
            .await
            .map_err(|e| LlmError::InvalidResponse(e.to_string()))?;

        let mut text_chunks = Vec::new();
        let mut tool_calls = Vec::new();

        for block in payload.content {
            match block {
                AnthropicResponseContentBlock::Text { text } => {
                    if !text.is_empty() {
                        text_chunks.push(text);
                    }
                }
                AnthropicResponseContentBlock::ToolUse { name, input, .. } => {
                    tool_calls.push(ToolCall {
                        id: None,
                        name,
                        arguments: input,
                    });
                }
                AnthropicResponseContentBlock::Other => {}
            }
        }

        Ok(LlmResponse {
            content: text_chunks.join("\n"),
            reasoning: None,
            tool_calls,
        })
    }
}

#[derive(Debug, Serialize)]
struct AnthropicMessagesRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicToolDefinition>>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicRequestContentBlock>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicRequestContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
}

#[derive(Debug, Serialize)]
struct AnthropicToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    content: Vec<AnthropicResponseContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicResponseContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(rename = "id")]
        _id: String,
        name: String,
        input: Value,
    },
    #[serde(other)]
    Other,
}
