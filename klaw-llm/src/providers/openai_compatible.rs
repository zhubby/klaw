use async_trait::async_trait;
use reqwest::{header::USER_AGENT, Client};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolCall, ToolDefinition,
};

/// OpenAI wire API 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiWireApi {
    ChatCompletions,
    Responses,
}

impl OpenAiWireApi {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "chat_completions" => Some(Self::ChatCompletions),
            "responses" => Some(Self::Responses),
            _ => None,
        }
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
    /// 底层 wire API。
    pub wire_api: OpenAiWireApi,
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
        match self.config.wire_api {
            OpenAiWireApi::ChatCompletions => {
                let normalized_base = normalize_openai_base_url(&self.config.base_url);
                format!("{}/chat/completions", normalized_base.trim_end_matches('/'))
            }
            OpenAiWireApi::Responses => self.config.base_url.trim_end_matches('/').to_string(),
        }
    }

    async fn execute_json<T: Serialize, R: DeserializeOwned>(
        &self,
        request: &T,
    ) -> Result<R, LlmError> {
        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .header(USER_AGENT, "openclaw/0.3.0")
            .json(request)
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
                "endpoint={}, http_status={status}, body={body}",
                self.endpoint()
            )));
        }

        response
            .json()
            .await
            .map_err(|e| LlmError::InvalidResponse(e.to_string()))
    }

    async fn chat_with_chat_completions(
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
            user: options.user,
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
            tools: map_chat_completion_tools(tools),
        };

        let payload: OpenAiChatCompletionResponse = self.execute_json(&request).await?;

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
                let args = decode_tool_arguments(call.function.arguments);
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

    async fn chat_with_responses(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let request = OpenAiResponsesRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            input: build_responses_input(messages),
            temperature: Some(options.temperature),
            max_output_tokens: options.max_output_tokens.or(options.max_tokens),
            instructions: options.instructions,
            previous_response_id: options.previous_response_id,
            store: options.store,
            metadata: options.metadata,
            include: options.include,
            parallel_tool_calls: options.parallel_tool_calls,
            tool_choice: options.tool_choice,
            text: options.text,
            reasoning: options.reasoning,
            truncation: options.truncation,
            user: options.user,
            service_tier: options.service_tier,
            stream: Some(false),
            tools: map_responses_tools(tools),
        };

        let payload: OpenAiResponsesResponse = self.execute_json(&request).await?;
        Ok(parse_responses_payload(payload))
    }
}

fn normalize_openai_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/').to_string();
    if trimmed == "https://api.openai.com" || trimmed == "http://api.openai.com" {
        return format!("{trimmed}/v1");
    }
    trimmed
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
        match self.config.wire_api {
            OpenAiWireApi::ChatCompletions => {
                self.chat_with_chat_completions(messages, tools, model, options)
                    .await
            }
            OpenAiWireApi::Responses => {
                self.chat_with_responses(messages, tools, model, options)
                    .await
            }
        }
    }
}

fn map_chat_completion_tools(tools: Vec<ToolDefinition>) -> Option<Vec<OpenAiToolDefinition>> {
    if tools.is_empty() {
        return None;
    }

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
}

fn map_responses_tools(tools: Vec<ToolDefinition>) -> Option<Vec<OpenAiResponsesToolDefinition>> {
    if tools.is_empty() {
        return None;
    }

    Some(
        tools
            .into_iter()
            .map(|tool| OpenAiResponsesToolDefinition {
                r#type: "function".to_string(),
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
                strict: None,
            })
            .collect(),
    )
}

fn build_responses_input(messages: Vec<LlmMessage>) -> Vec<OpenAiResponsesInputItem> {
    let mut input = Vec::new();

    for message in messages {
        if message.role == "tool" {
            if let Some(call_id) = message.tool_call_id {
                input.push(OpenAiResponsesInputItem::FunctionCallOutput(
                    OpenAiResponsesFunctionCallOutputInput {
                        r#type: "function_call_output".to_string(),
                        call_id,
                        output: message.content,
                    },
                ));
                continue;
            }

            if !message.content.trim().is_empty() {
                input.push(OpenAiResponsesInputItem::Message(
                    OpenAiResponsesInputMessage {
                        role: "user".to_string(),
                        content: message.content,
                    },
                ));
            }
            continue;
        }

        if !message.content.trim().is_empty() {
            input.push(OpenAiResponsesInputItem::Message(
                OpenAiResponsesInputMessage {
                    role: message.role.clone(),
                    content: message.content,
                },
            ));
        }

        if message.role == "assistant" {
            if let Some(tool_calls) = message.tool_calls {
                for (index, call) in tool_calls.into_iter().enumerate() {
                    input.push(OpenAiResponsesInputItem::FunctionCall(
                        OpenAiResponsesFunctionCallInput {
                            r#type: "function_call".to_string(),
                            call_id: call.id.unwrap_or_else(|| format!("call_{}", index + 1)),
                            name: call.name,
                            arguments: call.arguments.to_string(),
                        },
                    ));
                }
            }
        }
    }

    input
}

fn parse_responses_payload(payload: OpenAiResponsesResponse) -> LlmResponse {
    let mut content_chunks = Vec::new();
    let mut reasoning_chunks = Vec::new();
    let mut tool_calls = Vec::new();

    for item in payload.output {
        match item {
            OpenAiResponsesOutputItem::Message { content } => {
                for block in content {
                    match block {
                        OpenAiResponsesMessageContent::OutputText { text }
                        | OpenAiResponsesMessageContent::InputText { text } => {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                content_chunks.push(trimmed.to_string());
                            }
                        }
                        OpenAiResponsesMessageContent::Refusal { refusal } => {
                            let trimmed = refusal.trim();
                            if !trimmed.is_empty() {
                                content_chunks.push(trimmed.to_string());
                            }
                        }
                        OpenAiResponsesMessageContent::Other => {}
                    }
                }
            }
            OpenAiResponsesOutputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => {
                tool_calls.push(ToolCall {
                    id: call_id,
                    name,
                    arguments: decode_responses_arguments(arguments),
                });
            }
            OpenAiResponsesOutputItem::Reasoning { summary, text } => {
                if let Some(value) = text {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        reasoning_chunks.push(trimmed.to_string());
                    }
                }

                for line in summary {
                    if let Some(value) = line.text {
                        let trimmed = value.trim();
                        if !trimmed.is_empty() {
                            reasoning_chunks.push(trimmed.to_string());
                        }
                    }
                }
            }
            OpenAiResponsesOutputItem::Other => {}
        }
    }

    if content_chunks.is_empty() {
        if let Some(output_text) = payload.output_text {
            let trimmed = output_text.trim();
            if !trimmed.is_empty() {
                content_chunks.push(trimmed.to_string());
            }
        }
    }

    let reasoning = if reasoning_chunks.is_empty() {
        None
    } else {
        Some(reasoning_chunks.join("\n"))
    };

    LlmResponse {
        content: content_chunks.join("\n"),
        reasoning,
        tool_calls,
    }
}

fn decode_tool_arguments(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::String(raw))
}

fn decode_responses_arguments(raw: Value) -> Value {
    match raw {
        Value::String(raw_text) => decode_tool_arguments(raw_text),
        value => value,
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
    user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolDefinition>>,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesRequest {
    model: String,
    input: Vec<OpenAiResponsesInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiResponsesToolDefinition>>,
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
    parameters: Value,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesToolDefinition {
    #[serde(rename = "type")]
    r#type: String,
    name: String,
    description: String,
    parameters: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    strict: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum OpenAiResponsesInputItem {
    Message(OpenAiResponsesInputMessage),
    FunctionCall(OpenAiResponsesFunctionCallInput),
    FunctionCallOutput(OpenAiResponsesFunctionCallOutputInput),
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesInputMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesFunctionCallInput {
    #[serde(rename = "type")]
    r#type: String,
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesFunctionCallOutputInput {
    #[serde(rename = "type")]
    r#type: String,
    call_id: String,
    output: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatCompletionResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesResponse {
    #[serde(default)]
    output: Vec<OpenAiResponsesOutputItem>,
    #[serde(default)]
    output_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OpenAiResponsesOutputItem {
    #[serde(rename = "message")]
    Message {
        #[serde(default)]
        content: Vec<OpenAiResponsesMessageContent>,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        #[serde(default)]
        call_id: Option<String>,
        name: String,
        #[serde(default)]
        arguments: Value,
    },
    #[serde(rename = "reasoning")]
    Reasoning {
        #[serde(default)]
        summary: Vec<OpenAiResponsesReasoningSummary>,
        #[serde(default)]
        text: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OpenAiResponsesMessageContent {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "refusal")]
    Refusal { refusal: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesReasoningSummary {
    #[serde(default)]
    text: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_responses_input_preserves_function_call_roundtrip_items() {
        let input = build_responses_input(vec![
            LlmMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            LlmMessage {
                role: "assistant".to_string(),
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: Some("call_123".to_string()),
                    name: "web_search".to_string(),
                    arguments: serde_json::json!({"q":"klaw"}),
                }]),
                tool_call_id: None,
            },
            LlmMessage {
                role: "tool".to_string(),
                content: "tool-result".to_string(),
                tool_calls: None,
                tool_call_id: Some("call_123".to_string()),
            },
        ]);

        let value = serde_json::to_value(input).unwrap_or(Value::Null);
        let expected = serde_json::json!([
            {"role":"user","content":"hello"},
            {
                "type":"function_call",
                "call_id":"call_123",
                "name":"web_search",
                "arguments":"{\"q\":\"klaw\"}"
            },
            {
                "type":"function_call_output",
                "call_id":"call_123",
                "output":"tool-result"
            }
        ]);

        assert_eq!(value, expected);
    }

    #[test]
    fn parse_responses_payload_extracts_text_reasoning_and_tool_calls() {
        let payload: OpenAiResponsesResponse = serde_json::from_value(serde_json::json!({
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{"text": "step-a"}, {"text": "step-b"}]
                },
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "final answer"}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_9",
                    "name": "web_fetch",
                    "arguments": "{\"url\":\"https://example.com\"}"
                }
            ]
        }))
        .unwrap_or(OpenAiResponsesResponse {
            output: Vec::new(),
            output_text: None,
        });

        let result = parse_responses_payload(payload);

        assert_eq!(result.content, "final answer");
        assert_eq!(result.reasoning.as_deref(), Some("step-a\nstep-b"));
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id.as_deref(), Some("call_9"));
        assert_eq!(result.tool_calls[0].name, "web_fetch");
        assert_eq!(
            result.tool_calls[0].arguments,
            serde_json::json!({"url":"https://example.com"})
        );
    }

    #[test]
    fn normalize_openai_base_url_appends_v1_for_openai_root() {
        assert_eq!(
            normalize_openai_base_url("https://api.openai.com"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_openai_base_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn responses_endpoint_uses_configured_base_url_verbatim() {
        let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
            base_url: "https://coding.dashscope.aliyuncs.com/v1/".to_string(),
            api_key: "test-key".to_string(),
            default_model: "test-model".to_string(),
            wire_api: OpenAiWireApi::Responses,
        });

        assert_eq!(
            provider.endpoint(),
            "https://coding.dashscope.aliyuncs.com/v1"
        );
    }
}
