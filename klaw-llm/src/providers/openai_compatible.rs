use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, header::USER_AGENT};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    ChatOptions, LlmAuditPayload, LlmAuditStatus, LlmError, LlmMessage, LlmProvider, LlmResponse,
    LlmStreamEvent, LlmUsage, LlmUsageSource, ToolCall, ToolDefinition,
    estimate::estimate_chat_usage,
};
use std::time::{SystemTime, UNIX_EPOCH};

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
    /// 是否启用 provider 原生 stream API。
    pub stream: bool,
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
    ) -> Result<(R, Value), LlmError> {
        let request_json = serde_json::to_value(request)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        let request_model = model_from_request_json(&request_json);
        let requested_at_ms = now_ms();
        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .header(USER_AGENT, "openclaw/0.3.0")
            .json(request)
            .send()
            .await
            .map_err(|e| {
                LlmError::request_failed(e.to_string()).with_audit(self.build_failed_audit(
                    request_json.clone(),
                    requested_at_ms,
                    None,
                    request_model.clone(),
                    "request_failed",
                    e.to_string(),
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let responded_at_ms = now_ms();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(LlmError::provider_unavailable(format!(
                "endpoint={}, http_status={status}, body={body}",
                self.endpoint()
            ))
            .with_audit(self.build_failed_audit(
                request_json,
                requested_at_ms,
                Some(responded_at_ms),
                request_model.clone(),
                "provider_unavailable",
                format!("http_status={status}, body={body}"),
            )));
        }

        let payload_value = response.json::<Value>().await.map_err(|e| {
            LlmError::invalid_response(e.to_string()).with_audit(self.build_failed_audit(
                request_json.clone(),
                requested_at_ms,
                Some(now_ms()),
                request_model.clone(),
                "invalid_response",
                e.to_string(),
            ))
        })?;
        let typed = serde_json::from_value(payload_value.clone()).map_err(|e| {
            LlmError::invalid_response(e.to_string()).with_audit(self.build_failed_audit(
                request_json,
                requested_at_ms,
                Some(now_ms()),
                request_model,
                "invalid_response",
                e.to_string(),
            ))
        })?;
        Ok((typed, payload_value))
    }

    async fn execute_sse<T: Serialize, F>(
        &self,
        request: &T,
        mut on_event: F,
    ) -> Result<(), LlmError>
    where
        F: FnMut(Value) -> Result<(), LlmError> + Send,
    {
        let request_json = serde_json::to_value(request)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        let requested_at_ms = now_ms();
        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .header(USER_AGENT, "openclaw/0.3.0")
            .json(request)
            .send()
            .await
            .map_err(|e| {
                LlmError::request_failed(e.to_string()).with_audit(self.build_failed_audit(
                    request_json.clone(),
                    requested_at_ms,
                    None,
                    model_from_request_json(&request_json),
                    "request_failed",
                    e.to_string(),
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let responded_at_ms = now_ms();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(LlmError::provider_unavailable(format!(
                "endpoint={}, http_status={status}, body={body}",
                self.endpoint()
            ))
            .with_audit(self.build_failed_audit(
                request_json,
                requested_at_ms,
                Some(responded_at_ms),
                model_from_request_json(&serde_json::to_value(request).unwrap_or(Value::Null)),
                "provider_unavailable",
                format!("http_status={status}, body={body}"),
            )));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|err| LlmError::stream_failed(err.to_string()))?;
            let text = std::str::from_utf8(&chunk)
                .map_err(|err| LlmError::stream_failed(err.to_string()))?;
            buffer.push_str(text);
            while let Some(event_end) = buffer.find("\n\n") {
                let raw_event = buffer[..event_end].to_string();
                buffer.drain(..event_end + 2);
                if let Some(event) = parse_sse_event(&raw_event)? {
                    on_event(event)?;
                }
            }
        }

        if !buffer.trim().is_empty() {
            if let Some(event) = parse_sse_event(&buffer)? {
                on_event(event)?;
            }
        }
        Ok(())
    }

    fn build_audit(
        &self,
        model: &str,
        status: LlmAuditStatus,
        request_body: Value,
        response_body: Option<Value>,
        requested_at_ms: i64,
        responded_at_ms: Option<i64>,
        error_code: Option<&str>,
        error_message: Option<String>,
        provider_request_id: Option<String>,
        provider_response_id: Option<String>,
    ) -> LlmAuditPayload {
        LlmAuditPayload {
            provider: self.name().to_string(),
            model: model.to_string(),
            wire_api: self.wire_api().unwrap_or(self.name()).to_string(),
            status,
            error_code: error_code.map(ToString::to_string),
            error_message,
            provider_request_id,
            provider_response_id,
            request_body,
            response_body,
            requested_at_ms,
            responded_at_ms,
        }
    }

    fn build_failed_audit(
        &self,
        request_body: Value,
        requested_at_ms: i64,
        responded_at_ms: Option<i64>,
        model: String,
        error_code: &str,
        error_message: String,
    ) -> LlmAuditPayload {
        self.build_audit(
            &model,
            LlmAuditStatus::Failed,
            request_body,
            None,
            requested_at_ms,
            responded_at_ms,
            Some(error_code),
            Some(error_message),
            None,
            None,
        )
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
            stream: None,
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
        let request_json = serde_json::to_value(&request)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        let requested_at_ms = now_ms();
        let (payload, payload_json): (OpenAiChatCompletionResponse, Value) =
            self.execute_json(&request).await?;

        let first = payload
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::invalid_response("no choices in response".to_string()))?;

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
            audit: None,
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
        response.audit = Some(
            self.build_audit(
                model.unwrap_or(&self.config.default_model),
                LlmAuditStatus::Success,
                request_json,
                Some(payload_json),
                requested_at_ms,
                Some(now_ms()),
                None,
                None,
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_request_id.clone()),
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_response_id.clone()),
            ),
        );
        Ok(response)
    }

    async fn chat_with_chat_completions_stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
        stream: UnboundedSender<LlmStreamEvent>,
    ) -> Result<LlmResponse, LlmError> {
        let original_messages = messages.clone();
        let original_tools = tools.clone();
        let request = OpenAiChatCompletionRequest {
            model: model.unwrap_or(&self.config.default_model).to_string(),
            temperature: options.temperature,
            max_tokens: options.max_tokens,
            stream: Some(true),
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
        let mut state = OpenAiChatCompletionStreamState::default();
        let request_json = serde_json::to_value(&request)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        let requested_at_ms = now_ms();
        self.execute_sse(&request, &mut |event| {
            apply_chat_completions_stream_event(&mut state, &stream, event)
        })
        .await?;
        let mut response = state.into_response();
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
        let response_json = serde_json::to_value(&response)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        response.audit = Some(
            self.build_audit(
                model.unwrap_or(&self.config.default_model),
                LlmAuditStatus::Success,
                request_json,
                Some(response_json),
                requested_at_ms,
                Some(now_ms()),
                None,
                None,
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_request_id.clone()),
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_response_id.clone()),
            ),
        );
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

        let request_json = serde_json::to_value(&request)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        let requested_at_ms = now_ms();
        let (payload, payload_json): (OpenAiResponsesResponse, Value) =
            self.execute_json(&request).await?;
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
        response.audit = Some(
            self.build_audit(
                model.unwrap_or(&self.config.default_model),
                LlmAuditStatus::Success,
                request_json,
                Some(payload_json),
                requested_at_ms,
                Some(now_ms()),
                None,
                None,
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_request_id.clone()),
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_response_id.clone()),
            ),
        );
        Ok(response)
    }

    async fn chat_with_responses_stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
        stream: UnboundedSender<LlmStreamEvent>,
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
            stream: Some(true),
            tools: map_responses_tools(tools),
        };
        let mut completed: Option<OpenAiResponsesResponse> = None;
        let request_json = serde_json::to_value(&request)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        let requested_at_ms = now_ms();
        self.execute_sse(&request, &mut |event| {
            apply_responses_stream_event(&stream, &mut completed, event)
        })
        .await?;
        let payload = completed.ok_or_else(|| {
            LlmError::invalid_response("responses stream ended without response.completed")
        })?;
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
        let response_json = serde_json::to_value(&response)
            .map_err(|err| LlmError::invalid_response(err.to_string()))?;
        response.audit = Some(
            self.build_audit(
                model.unwrap_or(&self.config.default_model),
                LlmAuditStatus::Success,
                request_json,
                Some(response_json),
                requested_at_ms,
                Some(now_ms()),
                None,
                None,
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_request_id.clone()),
                response
                    .usage
                    .as_ref()
                    .and_then(|usage| usage.provider_response_id.clone()),
            ),
        );
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

fn model_from_request_json(request_json: &Value) -> String {
    request_json
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
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

    async fn chat_stream(
        &self,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
        model: Option<&str>,
        options: ChatOptions,
        stream: Option<UnboundedSender<LlmStreamEvent>>,
    ) -> Result<LlmResponse, LlmError> {
        let Some(stream) = stream else {
            return self.chat(messages, tools, model, options).await;
        };
        if !self.config.stream {
            return self.chat(messages, tools, model, options).await;
        }
        match self.config.wire_api {
            OpenAiWireApi::ChatCompletions => {
                self.chat_with_chat_completions_stream(messages, tools, model, options, stream)
                    .await
            }
            OpenAiWireApi::Responses => {
                self.chat_with_responses_stream(messages, tools, model, options, stream)
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
        audit: None,
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

fn parse_sse_event(raw_event: &str) -> Result<Option<Value>, LlmError> {
    let mut data_lines = Vec::new();
    for line in raw_event.lines() {
        if let Some(value) = line.strip_prefix("data:") {
            let trimmed = value.trim();
            if trimmed == "[DONE]" {
                return Ok(None);
            }
            data_lines.push(trimmed.to_string());
        }
    }
    if data_lines.is_empty() {
        return Ok(None);
    }
    let payload = data_lines.join("\n");
    serde_json::from_str(&payload)
        .map(Some)
        .map_err(|err| LlmError::invalid_response(format!("invalid stream event payload: {err}")))
}

#[derive(Debug, Default)]
struct OpenAiChatCompletionStreamState {
    content: String,
    reasoning: String,
    tool_calls: Vec<OpenAiToolCallStreamState>,
    usage: Option<LlmUsage>,
}

#[derive(Debug, Default)]
struct OpenAiToolCallStreamState {
    id: Option<String>,
    name: String,
    arguments: String,
}

impl OpenAiChatCompletionStreamState {
    fn into_response(self) -> LlmResponse {
        let usage = self.usage;
        LlmResponse {
            content: self.content,
            reasoning: (!self.reasoning.trim().is_empty()).then_some(self.reasoning),
            tool_calls: self
                .tool_calls
                .into_iter()
                .filter(|call| !call.name.trim().is_empty())
                .map(|call| ToolCall {
                    id: call.id,
                    name: call.name,
                    arguments: decode_tool_arguments(call.arguments),
                })
                .collect(),
            usage_source: usage.as_ref().map(|_| LlmUsageSource::ProviderReported),
            usage,
            audit: None,
        }
    }
}

fn apply_chat_completions_stream_event(
    state: &mut OpenAiChatCompletionStreamState,
    stream: &UnboundedSender<LlmStreamEvent>,
    event: Value,
) -> Result<(), LlmError> {
    if let Some(usage) = event
        .get("usage")
        .cloned()
        .filter(|value| !value.is_null())
        .map(serde_json::from_value::<OpenAiChatCompletionUsage>)
        .transpose()
        .map_err(|err| LlmError::invalid_response(err.to_string()))?
    {
        state.usage = Some(map_chat_completion_usage(usage));
    }

    let Some(choice) = event
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
    else {
        return Ok(());
    };
    let Some(delta) = choice.get("delta") else {
        return Ok(());
    };

    if let Some(content) = delta.get("content").and_then(Value::as_str) {
        state.content.push_str(content);
        let _ = stream.send(LlmStreamEvent::ContentDelta(content.to_string()));
    }
    if let Some(reasoning) = delta
        .get("reasoning_content")
        .or_else(|| delta.get("reasoning"))
        .and_then(Value::as_str)
    {
        state.reasoning.push_str(reasoning);
        let _ = stream.send(LlmStreamEvent::ReasoningDelta(reasoning.to_string()));
    }
    if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let index = tool_call
                .get("index")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(0);
            while state.tool_calls.len() <= index {
                state.tool_calls.push(OpenAiToolCallStreamState::default());
            }
            let state_call = &mut state.tool_calls[index];
            if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                state_call.id = Some(id.to_string());
            }
            if let Some(function) = tool_call.get("function") {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    state_call.name.push_str(name);
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    state_call.arguments.push_str(arguments);
                }
            }
        }
    }
    Ok(())
}

fn apply_responses_stream_event(
    stream: &UnboundedSender<LlmStreamEvent>,
    completed: &mut Option<OpenAiResponsesResponse>,
    event: Value,
) -> Result<(), LlmError> {
    match event.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta") => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                let _ = stream.send(LlmStreamEvent::ContentDelta(delta.to_string()));
            }
        }
        Some("response.reasoning_summary_text.delta") | Some("response.reasoning_text.delta") => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                let _ = stream.send(LlmStreamEvent::ReasoningDelta(delta.to_string()));
            }
        }
        Some("response.completed") => {
            let response = event
                .get("response")
                .cloned()
                .ok_or_else(|| {
                    LlmError::invalid_response("response.completed missing response payload")
                })
                .and_then(|value| {
                    serde_json::from_value::<OpenAiResponsesResponse>(value)
                        .map_err(|err| LlmError::invalid_response(err.to_string()))
                })?;
            *completed = Some(response);
        }
        Some("error") => {
            let message = event
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown stream error");
            return Err(LlmError::stream_failed(message.to_string()));
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct OpenAiChatCompletionRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
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
            stream: false,
        });

        assert_eq!(
            provider.endpoint(),
            "https://coding.dashscope.aliyuncs.com/v1"
        );
    }

    #[test]
    fn parse_sse_event_reads_json_payload() {
        let event = parse_sse_event(
            "event: message\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
        )
        .expect("event should parse")
        .expect("event should not be done");
        assert_eq!(
            event.get("type").and_then(Value::as_str),
            Some("response.output_text.delta")
        );
        assert_eq!(event.get("delta").and_then(Value::as_str), Some("hi"));
    }

    #[test]
    fn apply_chat_completions_stream_event_accumulates_tool_calls() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut state = OpenAiChatCompletionStreamState::default();
        apply_chat_completions_stream_event(
            &mut state,
            &tx,
            serde_json::json!({
                "choices": [{
                    "delta": {
                        "content": "hel",
                        "reasoning_content": "plan ",
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_1",
                            "function": {
                                "name": "web_fetch",
                                "arguments": "{\"url\":\"https://"
                            }
                        }]
                    }
                }]
            }),
        )
        .expect("stream event should apply");
        apply_chat_completions_stream_event(
            &mut state,
            &tx,
            serde_json::json!({
                "choices": [{
                    "delta": {
                        "content": "lo",
                        "reasoning": "done",
                        "tool_calls": [{
                            "index": 0,
                            "function": {
                                "arguments": "example.com\"}"
                            }
                        }]
                    }
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 4,
                    "total_tokens": 14
                }
            }),
        )
        .expect("stream event should apply");

        let first = rx.try_recv().expect("first delta should be emitted");
        let second = rx.try_recv().expect("second delta should be emitted");
        let third = rx.try_recv().expect("third delta should be emitted");
        let fourth = rx.try_recv().expect("fourth delta should be emitted");
        assert_eq!(first, LlmStreamEvent::ContentDelta("hel".to_string()));
        assert_eq!(second, LlmStreamEvent::ReasoningDelta("plan ".to_string()));
        assert_eq!(third, LlmStreamEvent::ContentDelta("lo".to_string()));
        assert_eq!(fourth, LlmStreamEvent::ReasoningDelta("done".to_string()));

        let response = state.into_response();
        assert_eq!(response.content, "hello");
        assert_eq!(response.reasoning.as_deref(), Some("plan done"));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "web_fetch");
        assert_eq!(
            response.tool_calls[0].arguments,
            serde_json::json!({"url":"https://example.com"})
        );
        assert_eq!(response.usage.expect("usage should exist").total_tokens, 14);
    }

    #[test]
    fn apply_responses_stream_event_captures_completed_payload() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut completed = None;
        apply_responses_stream_event(
            &tx,
            &mut completed,
            serde_json::json!({
                "type": "response.output_text.delta",
                "delta": "hello"
            }),
        )
        .expect("delta event should apply");
        apply_responses_stream_event(
            &tx,
            &mut completed,
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_1",
                    "output": [{
                        "type": "message",
                        "content": [{"type":"output_text","text":"hello"}]
                    }],
                    "usage": {
                        "input_tokens": 3,
                        "output_tokens": 1,
                        "total_tokens": 4
                    }
                }
            }),
        )
        .expect("completed event should apply");

        assert_eq!(
            rx.try_recv().expect("delta should be emitted"),
            LlmStreamEvent::ContentDelta("hello".to_string())
        );
        let response = parse_responses_payload(completed.expect("completed payload should exist"));
        assert_eq!(response.content, "hello");
        assert_eq!(response.usage.expect("usage should exist").total_tokens, 4);
    }
}
