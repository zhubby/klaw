use async_trait::async_trait;
use reqwest::{header::USER_AGENT, Client};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::{
    estimate::estimate_chat_usage, ChatOptions, LlmError, LlmMessage, LlmProvider, LlmResponse,
    LlmUsage, LlmUsageSource, ToolCall, ToolDefinition,
};

/// OpenAI wire API 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAiWireApi {
    ChatCompletions,
    Responses,
}

impl OpenAiWireApi {
    pub const VARIANTS: [&str; 2] = ["chat_completions", "responses"];

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "chat_completions" => Some(Self::ChatCompletions),
            "responses" => Some(Self::Responses),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
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
    /// 可选的本地 tokenizer.json 路径。
    pub tokenizer_path: Option<String>,
    /// 是否启用系统代理。false 时强制直连（no_proxy）。
    pub proxy: bool,
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
            client: build_http_client(config.proxy),
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
        let original_messages = messages.clone();
        let original_tools = tools.clone();
        let request = OpenAiChatCompletionRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            temperature: options.temperature,
            max_tokens: options.max_tokens,
            user: options.user,
            messages: messages
                .into_iter()
                .map(|m| OpenAiMessage {
                    role: m.role.clone(),
                    content: map_chat_message_content(&m),
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

        let content = first
            .message
            .content
            .map(extract_chat_message_text)
            .unwrap_or_default();
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

        let usage = payload
            .usage
            .as_ref()
            .map(|value| map_chat_completion_usage(value.clone()));
        let mut response = LlmResponse {
            content,
            reasoning,
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
                self.wire_api().unwrap_or("chat_completions"),
                self.config.tokenizer_path.as_deref(),
                &original_messages,
                &original_tools,
                &response.content,
                response.reasoning.as_deref(),
                &response.tool_calls,
            ));
            response.usage_source = Some(LlmUsageSource::EstimatedLocal);
        }
        Ok(response)
    }

    async fn chat_with_responses(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let original_messages = messages.clone();
        let original_tools = tools.clone();
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
        let mut response = parse_responses_payload(payload);
        if response.usage.is_none() {
            response.usage = Some(estimate_chat_usage(
                self.name(),
                model.unwrap_or(&self.config.default_model),
                self.wire_api().unwrap_or("responses"),
                self.config.tokenizer_path.as_deref(),
                &original_messages,
                &original_tools,
                &response.content,
                response.reasoning.as_deref(),
                &response.tool_calls,
            ));
            response.usage_source = Some(LlmUsageSource::EstimatedLocal);
        }
        Ok(response)
    }
}

fn build_http_client(proxy_enabled: bool) -> Client {
    let builder = if proxy_enabled {
        Client::builder()
    } else {
        Client::builder().no_proxy()
    };
    match builder.build() {
        Ok(client) => client,
        Err(_) => Client::new(),
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

    fn wire_api(&self) -> Option<&str> {
        Some(match self.config.wire_api {
            OpenAiWireApi::ChatCompletions => "chat_completions",
            OpenAiWireApi::Responses => "responses",
        })
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
                let content = build_responses_message_content(&message);
                if content.is_empty() {
                    continue;
                }
                input.push(OpenAiResponsesInputItem::Message(
                    OpenAiResponsesInputMessage {
                        role: "user".to_string(),
                        content,
                    },
                ));
            }
            continue;
        }

        if !message.content.trim().is_empty() {
            let content = build_responses_message_content(&message);
            if content.is_empty() {
                continue;
            }
            input.push(OpenAiResponsesInputItem::Message(
                OpenAiResponsesInputMessage {
                    role: message.role.clone(),
                    content,
                },
            ));
        } else if !message.media.is_empty() {
            let content = build_responses_message_content(&message);
            if content.is_empty() {
                continue;
            }
            input.push(OpenAiResponsesInputItem::Message(
                OpenAiResponsesInputMessage {
                    role: message.role.clone(),
                    content,
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

fn map_chat_message_content(message: &LlmMessage) -> Option<OpenAiChatMessageContent> {
    let has_media = !message.media.is_empty();
    if message.role == "assistant"
        && message.tool_calls.is_some()
        && message.content.is_empty()
        && !has_media
    {
        return None;
    }
    if !has_media {
        return Some(OpenAiChatMessageContent::Text(message.content.clone()));
    }

    let mut parts = Vec::new();
    let text = message.content.trim();
    if !text.is_empty() {
        parts.push(OpenAiChatMessageContentPart::Text {
            text: text.to_string(),
        });
    }
    for media in &message.media {
        let url = media.url.trim();
        if url.is_empty() {
            continue;
        }
        parts.push(OpenAiChatMessageContentPart::ImageUrl {
            image_url: OpenAiChatImageUrl {
                url: url.to_string(),
            },
        });
    }

    if parts.is_empty() {
        None
    } else {
        Some(OpenAiChatMessageContent::Parts(parts))
    }
}

fn build_responses_message_content(
    message: &LlmMessage,
) -> Vec<OpenAiResponsesInputMessageContent> {
    let mut content = Vec::new();
    let text = message.content.trim();
    if !text.is_empty() {
        content.push(OpenAiResponsesInputMessageContent::InputText {
            text: text.to_string(),
        });
    }
    for media in &message.media {
        let url = media.url.trim();
        if url.is_empty() {
            continue;
        }
        content.push(OpenAiResponsesInputMessageContent::InputImage {
            image_url: url.to_string(),
        });
    }
    content
}

fn parse_responses_payload(payload: OpenAiResponsesResponse) -> LlmResponse {
    let usage = payload
        .usage
        .as_ref()
        .map(|value| map_responses_usage(value, payload.id.clone()));
    let usage_source = if usage.is_some() {
        Some(LlmUsageSource::ProviderReported)
    } else {
        None
    };
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
        usage,
        usage_source,
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

fn map_chat_completion_usage(usage: OpenAiChatCompletionUsage) -> LlmUsage {
    LlmUsage {
        input_tokens: usage.prompt_tokens.max(0) as u64,
        output_tokens: usage.completion_tokens.max(0) as u64,
        total_tokens: usage.total_tokens.max(0) as u64,
        cached_input_tokens: usage
            .prompt_tokens_details
            .and_then(|details| details.cached_tokens)
            .map(|value| value.max(0) as u64),
        reasoning_tokens: usage
            .completion_tokens_details
            .and_then(|details| details.reasoning_tokens)
            .map(|value| value.max(0) as u64),
        provider_request_id: None,
        provider_response_id: None,
    }
}

fn map_responses_usage(usage: &OpenAiResponsesUsage, response_id: Option<String>) -> LlmUsage {
    let input_tokens = usage.input_tokens.unwrap_or(0).max(0) as u64;
    let output_tokens = usage.output_tokens.unwrap_or(0).max(0) as u64;
    let total_tokens = usage
        .total_tokens
        .map(|value| value.max(0) as u64)
        .unwrap_or(input_tokens.saturating_add(output_tokens));
    LlmUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        cached_input_tokens: usage
            .input_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens)
            .map(|value| value.max(0) as u64),
        reasoning_tokens: usage
            .output_tokens_details
            .as_ref()
            .and_then(|details| details.reasoning_tokens)
            .map(|value| value.max(0) as u64),
        provider_request_id: None,
        provider_response_id: response_id,
    }
}

fn extract_chat_message_text(content: OpenAiChatMessageContent) -> String {
    match content {
        OpenAiChatMessageContent::Text(text) => text,
        OpenAiChatMessageContent::Parts(parts) => parts
            .into_iter()
            .filter_map(|part| match part {
                OpenAiChatMessageContentPart::Text { text } => Some(text),
                OpenAiChatMessageContentPart::ImageUrl { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
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
    content: Option<OpenAiChatMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum OpenAiChatMessageContent {
    Text(String),
    Parts(Vec<OpenAiChatMessageContentPart>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum OpenAiChatMessageContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAiChatImageUrl },
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiChatImageUrl {
    url: String,
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
    content: Vec<OpenAiResponsesInputMessageContent>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum OpenAiResponsesInputMessageContent {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "input_image")]
    InputImage { image_url: String },
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
    #[serde(default)]
    usage: Option<OpenAiChatCompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    output: Vec<OpenAiResponsesOutputItem>,
    #[serde(default)]
    output_text: Option<String>,
    #[serde(default)]
    usage: Option<OpenAiResponsesUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiChatCompletionUsage {
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAiPromptTokensDetails>,
    #[serde(default)]
    completion_tokens_details: Option<OpenAiCompletionTokensDetails>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiPromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiCompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesUsage {
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
    #[serde(default)]
    total_tokens: Option<i64>,
    #[serde(default)]
    input_tokens_details: Option<OpenAiResponsesInputTokensDetails>,
    #[serde(default)]
    output_tokens_details: Option<OpenAiResponsesOutputTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesInputTokensDetails {
    #[serde(default)]
    cached_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponsesOutputTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<i64>,
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
                media: Vec::new(),
                tool_calls: None,
                tool_call_id: None,
            },
            LlmMessage {
                role: "assistant".to_string(),
                content: String::new(),
                media: Vec::new(),
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
                media: Vec::new(),
                tool_calls: None,
                tool_call_id: Some("call_123".to_string()),
            },
        ]);

        let value = serde_json::to_value(input).unwrap_or(Value::Null);
        let expected = serde_json::json!([
            {
                "role":"user",
                "content":[{"type":"input_text","text":"hello"}]
            },
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
            id: None,
            output: Vec::new(),
            output_text: None,
            usage: None,
        });

        let result = parse_responses_payload(payload);

        assert_eq!(result.content, "final answer");
        assert_eq!(result.reasoning.as_deref(), Some("step-a\nstep-b"));
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.usage.as_ref().map(|usage| usage.input_tokens), None);
        assert_eq!(result.tool_calls[0].id.as_deref(), Some("call_9"));
        assert_eq!(result.tool_calls[0].name, "web_fetch");
        assert_eq!(
            result.tool_calls[0].arguments,
            serde_json::json!({"url":"https://example.com"})
        );
    }

    #[test]
    fn parse_responses_payload_extracts_usage() {
        let payload: OpenAiResponsesResponse = serde_json::from_value(serde_json::json!({
            "id": "resp_123",
            "output": [],
            "usage": {
                "input_tokens": 12,
                "output_tokens": 5,
                "total_tokens": 17,
                "input_tokens_details": {
                    "cached_tokens": 3
                },
                "output_tokens_details": {
                    "reasoning_tokens": 2
                }
            }
        }))
        .expect("responses payload should deserialize");

        let result = parse_responses_payload(payload);
        let usage = result.usage.expect("usage should be present");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.total_tokens, 17);
        assert_eq!(usage.cached_input_tokens, Some(3));
        assert_eq!(usage.reasoning_tokens, Some(2));
        assert_eq!(usage.provider_response_id.as_deref(), Some("resp_123"));
    }

    #[test]
    fn chat_completion_usage_mapping_extracts_cached_and_reasoning_tokens() {
        let payload: OpenAiChatCompletionResponse = serde_json::from_value(serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "done"
                }
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 7,
                "total_tokens": 27,
                "prompt_tokens_details": {
                    "cached_tokens": 4
                },
                "completion_tokens_details": {
                    "reasoning_tokens": 1
                }
            }
        }))
        .expect("chat completion payload should deserialize");

        let usage = payload
            .usage
            .map(map_chat_completion_usage)
            .expect("usage should be present");
        assert_eq!(usage.input_tokens, 20);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(usage.total_tokens, 27);
        assert_eq!(usage.cached_input_tokens, Some(4));
        assert_eq!(usage.reasoning_tokens, Some(1));
    }

    #[test]
    fn build_responses_input_includes_input_image_blocks() {
        let input = build_responses_input(vec![LlmMessage {
            role: "user".to_string(),
            content: "what is in this image?".to_string(),
            media: vec![crate::LlmMedia {
                mime_type: Some("image/png".to_string()),
                url: "data:image/png;base64,Zm9v".to_string(),
            }],
            tool_calls: None,
            tool_call_id: None,
        }]);

        let value = serde_json::to_value(input).unwrap_or(Value::Null);
        let expected = serde_json::json!([
            {
                "role":"user",
                "content":[
                    {"type":"input_text","text":"what is in this image?"},
                    {"type":"input_image","image_url":"data:image/png;base64,Zm9v"}
                ]
            }
        ]);
        assert_eq!(value, expected);
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
            tokenizer_path: None,
            proxy: false,
            wire_api: OpenAiWireApi::Responses,
        });

        assert_eq!(
            provider.endpoint(),
            "https://coding.dashscope.aliyuncs.com/v1"
        );
    }
}
