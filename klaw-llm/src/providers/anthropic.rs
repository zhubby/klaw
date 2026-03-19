use async_trait::async_trait;
use reqwest::{header::USER_AGENT, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    estimate::estimate_chat_usage, ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse,
    LlmUsage, LlmUsageSource, ToolCall, ToolDefinition,
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
    /// 可选的本地 tokenizer.json 路径。
    pub tokenizer_path: Option<String>,
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

    fn wire_api(&self) -> Option<&str> {
        Some("messages")
    }

    fn tokenizer_path(&self) -> Option<&str> {
        self.config.tokenizer_path.as_deref()
    }

    async fn chat(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let original_messages = messages.clone();
        let original_tools = tools.clone();
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

        let usage = payload.usage.as_ref().map(|usage| LlmUsage {
            input_tokens: usage.input_tokens.max(0) as u64,
            output_tokens: usage.output_tokens.max(0) as u64,
            total_tokens: (usage.input_tokens.max(0) + usage.output_tokens.max(0)) as u64,
            cached_input_tokens: usage.cache_creation_input_tokens.map(|value| value as u64),
            reasoning_tokens: None,
            provider_request_id: None,
            provider_response_id: payload.id.clone(),
        });
        let mut response = LlmResponse {
            content: text_chunks.join("\n"),
            reasoning: None,
            tool_calls,
            usage,
            usage_source: payload
                .usage
                .as_ref()
                .map(|_| LlmUsageSource::ProviderReported),
        };
        if response.usage.is_none() {
            response.usage = Some(estimate_chat_usage(
                self.name(),
                model.unwrap_or(&self.config.default_model),
                self.wire_api().unwrap_or("messages"),
                self.config.tokenizer_path.as_deref(),
                &original_messages,
                &original_tools,
                &response.content,
                None,
                &response.tool_calls,
            ));
            response.usage_source = Some(LlmUsageSource::EstimatedLocal);
        }
        Ok(response)
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
    #[serde(default)]
    id: Option<String>,
    content: Vec<AnthropicResponseContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: i64,
    output_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: Option<i64>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_response_usage_deserializes() {
        let payload: AnthropicMessagesResponse = serde_json::from_value(serde_json::json!({
            "id": "msg_123",
            "content": [
                {
                    "type": "text",
                    "text": "done"
                }
            ],
            "usage": {
                "input_tokens": 19,
                "output_tokens": 6,
                "cache_creation_input_tokens": 4
            }
        }))
        .expect("anthropic response should deserialize");

        assert_eq!(payload.id.as_deref(), Some("msg_123"));
        let usage = payload.usage.expect("usage should be present");
        assert_eq!(usage.input_tokens, 19);
        assert_eq!(usage.output_tokens, 6);
        assert_eq!(usage.cache_creation_input_tokens, Some(4));
    }
}
