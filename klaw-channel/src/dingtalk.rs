use crate::{Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime};
use futures_util::{SinkExt, StreamExt};
use klaw_core::{MediaReference, MediaSourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;
use tokio::time::{self, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, info, warn};

const DINGTALK_OPEN_API_BASE: &str = "https://api.dingtalk.com";
const CONNECTION_OPEN_PATH: &str = "/v1.0/gateway/connections/open";
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
static RUSTLS_PROVIDER_INSTALLED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DingtalkChannelConfig {
    pub account_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub bot_title: String,
    pub show_reasoning: bool,
    pub allowlist: Vec<String>,
}

impl Default for DingtalkChannelConfig {
    fn default() -> Self {
        Self {
            account_id: "default".to_string(),
            client_id: String::new(),
            client_secret: String::new(),
            bot_title: "Klaw".to_string(),
            show_reasoning: false,
            allowlist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DingtalkChannel {
    config: DingtalkChannelConfig,
    client: DingtalkApiClient,
    seen_event_ids: HashSet<String>,
}

impl DingtalkChannel {
    pub fn new(config: DingtalkChannelConfig) -> Self {
        Self {
            config,
            client: DingtalkApiClient::new(),
            seen_event_ids: HashSet::new(),
        }
    }

    fn validate_config(&self) -> ChannelResult<()> {
        if self.config.client_id.trim().is_empty() {
            return Err("dingtalk client_id is required".into());
        }
        if self.config.client_secret.trim().is_empty() {
            return Err("dingtalk client_secret is required".into());
        }
        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
impl Channel for DingtalkChannel {
    fn name(&self) -> &'static str {
        "dingtalk"
    }

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
        self.validate_config()?;
        ensure_rustls_crypto_provider();
        info!("dingtalk channel started");

        loop {
            let ticket = match self
                .client
                .open_stream_connection(&self.config.client_id, &self.config.client_secret)
                .await
            {
                Ok(ticket) => ticket,
                Err(err) => {
                    warn!(error = %err, "failed to open dingtalk stream connection");
                    time::sleep(RECONNECT_DELAY).await;
                    continue;
                }
            };

            let ws_url = DingtalkApiClient::build_ws_url(&ticket.endpoint, &ticket.ticket);
            let connect = time::timeout(CONNECT_TIMEOUT, connect_async(ws_url.as_str())).await;
            let connect_result = match connect {
                Ok(result) => result,
                Err(_) => {
                    warn!(
                        ws_endpoint = ws_url.as_str(),
                        timeout_secs = CONNECT_TIMEOUT.as_secs(),
                        "dingtalk stream connection timed out"
                    );
                    time::sleep(RECONNECT_DELAY).await;
                    continue;
                }
            };

            match connect_result {
                Ok((mut ws, response)) => {
                    info!(
                        ws_endpoint = ws_url.as_str(),
                        handshake_status = response.status().as_u16(),
                        "dingtalk stream connection established"
                    );
                    let mut cron_tick = time::interval(runtime.cron_tick_interval());
                    let mut runtime_tick = time::interval(runtime.runtime_tick_interval());

                    loop {
                        tokio::select! {
                            _ = cron_tick.tick() => {
                                runtime.on_cron_tick().await;
                            }
                            _ = runtime_tick.tick() => {
                                runtime.on_runtime_tick().await;
                            }
                            message = ws.next() => {
                                let Some(message) = message else {
                                    warn!("dingtalk stream connection closed by remote");
                                    break;
                                };

                                match message {
                                    Ok(Message::Text(text)) => {
                                        if let Err(err) = self
                                            .handle_text_message(runtime, &mut ws, text.as_str())
                                            .await
                                        {
                                            warn!(error = %err, "failed to process dingtalk message");
                                        }
                                    }
                                    Ok(Message::Ping(payload)) => {
                                        if let Err(err) = ws.send(Message::Pong(payload)).await {
                                            warn!(error = %err, "failed to send websocket pong");
                                            break;
                                        }
                                    }
                                    Ok(Message::Pong(_)) => {
                                        debug!("received dingtalk websocket pong");
                                    }
                                    Ok(Message::Close(frame)) => {
                                        info!(close_frame = ?frame, "dingtalk stream connection closed");
                                        break;
                                    }
                                    Ok(Message::Binary(_)) | Ok(Message::Frame(_)) => {}
                                    Err(err) => {
                                        warn!(error = %err, "dingtalk stream receive failed");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        ws_endpoint = ws_url.as_str(),
                        error = %err,
                        "dingtalk stream connect failed"
                    );
                }
            }

            time::sleep(RECONNECT_DELAY).await;
        }
    }
}

impl DingtalkChannel {
    async fn handle_text_message(
        &mut self,
        runtime: &dyn ChannelRuntime,
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        text: &str,
    ) -> ChannelResult<()> {
        let envelope: StreamEnvelope = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(err) => {
                warn!(error = %err, payload = text, "invalid dingtalk stream envelope");
                return Ok(());
            }
        };

        match envelope.message_type.as_str() {
            "SYSTEM" => {
                self.handle_system_message(ws, &envelope).await?;
            }
            "EVENT" | "CALLBACK" => {
                self.handle_callback_message(runtime, ws, &envelope).await?;
            }
            other => {
                debug!(
                    message_type = other,
                    "ignoring unsupported dingtalk stream message type"
                );
            }
        }

        Ok(())
    }

    async fn handle_system_message(
        &self,
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        envelope: &StreamEnvelope,
    ) -> ChannelResult<()> {
        let topic = envelope
            .headers
            .get("topic")
            .map(String::as_str)
            .unwrap_or("");
        if topic != "ping" {
            return Ok(());
        }

        let opaque = parse_stream_data(&envelope.data)
            .and_then(|value| {
                value
                    .get("opaque")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default();

        send_ack(
            ws,
            envelope,
            &serde_json::json!({ "opaque": opaque }).to_string(),
        )
        .await
    }

    async fn handle_callback_message(
        &mut self,
        runtime: &dyn ChannelRuntime,
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        envelope: &StreamEnvelope,
    ) -> ChannelResult<()> {
        let Some(payload) = parse_stream_data(&envelope.data) else {
            return send_ack(ws, envelope, "").await;
        };

        let Some(inbound) = parse_inbound_event(&payload) else {
            return send_ack(ws, envelope, "").await;
        };

        if !is_sender_allowed(&self.config.allowlist, &inbound.sender_id) {
            warn!(
                sender = inbound.sender_id.as_str(),
                "dingtalk sender blocked by allowlist"
            );
            return send_ack(ws, envelope, "").await;
        }

        if !self.seen_event_ids.insert(inbound.event_id.clone()) {
            debug!(
                event_id = inbound.event_id.as_str(),
                "ignoring duplicated dingtalk event"
            );
            return send_ack(ws, envelope, "").await;
        }

        let maybe_output = runtime
            .submit(ChannelRequest {
                channel: self.name().to_string(),
                input: inbound.text.clone(),
                session_key: format!("dingtalk:{}:{}", self.config.account_id, inbound.chat_id),
                chat_id: inbound.chat_id.clone(),
                media_references: inbound.media_references.clone(),
            })
            .await;

        match maybe_output {
            Ok(Some(output)) => {
                let body = render_agent_output(&output, self.config.show_reasoning);
                if let Err(err) = self
                    .client
                    .send_session_webhook_markdown(
                        &inbound.session_webhook,
                        &self.config.bot_title,
                        &body,
                    )
                    .await
                {
                    warn!(
                        chat_id = inbound.chat_id.as_str(),
                        error = %err,
                        "failed to send dingtalk reply"
                    );
                }
            }
            Ok(None) => {}
            Err(err) => {
                warn!(
                    chat_id = inbound.chat_id.as_str(),
                    error = %err,
                    "dingtalk runtime submit failed"
                );
            }
        }

        send_ack(ws, envelope, "").await
    }
}

#[derive(Debug, Clone)]
struct DingtalkApiClient {
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
struct StreamConnectionTicket {
    endpoint: String,
    ticket: String,
}

impl DingtalkApiClient {
    fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
        }
    }

    async fn open_stream_connection(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> ChannelResult<StreamConnectionTicket> {
        let url = format!("{DINGTALK_OPEN_API_BASE}{CONNECTION_OPEN_PATH}");
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "clientId": client_id,
                "clientSecret": client_secret,
                "subscriptions": [
                    {
                        "type": "CALLBACK",
                        "topic": "/v1.0/im/bot/messages/get"
                    }
                ],
                "ua": "klaw/dingtalk"
            }))
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "open dingtalk stream connection failed: HTTP {} body={}",
                status, body
            )
            .into());
        }

        let endpoint = body
            .get("endpoint")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or("missing endpoint from dingtalk stream response")?
            .to_string();

        let ticket = body
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or("missing ticket from dingtalk stream response")?
            .to_string();

        Ok(StreamConnectionTicket { endpoint, ticket })
    }

    async fn send_session_webhook_markdown(
        &self,
        session_webhook: &str,
        title: &str,
        text: &str,
    ) -> ChannelResult<()> {
        let response = self
            .http
            .post(session_webhook)
            .json(&serde_json::json!({
                "msgtype": "markdown",
                "markdown": {
                    "title": title,
                    "text": text,
                }
            }))
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk session webhook send failed with HTTP {}: {}",
                status, body
            )
            .into());
        }

        Ok(())
    }

    fn build_ws_url(endpoint: &str, ticket: &str) -> String {
        if endpoint.contains('?') {
            format!("{endpoint}&ticket={}", urlencoding::encode(ticket))
        } else {
            format!("{endpoint}?ticket={}", urlencoding::encode(ticket))
        }
    }
}

fn ensure_rustls_crypto_provider() {
    if RUSTLS_PROVIDER_INSTALLED.get().is_some() {
        return;
    }

    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_ok()
    {
        let _ = RUSTLS_PROVIDER_INSTALLED.set(());
    }
}

#[derive(Debug, Deserialize)]
struct StreamEnvelope {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    data: Value,
}

#[derive(Debug, Serialize)]
struct StreamAck<'a> {
    code: i32,
    headers: HashMap<&'a str, String>,
    message: &'a str,
    data: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
struct InboundEvent {
    event_id: String,
    chat_id: String,
    sender_id: String,
    session_webhook: String,
    text: String,
    media_references: Vec<MediaReference>,
}

async fn send_ack(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    envelope: &StreamEnvelope,
    data: &str,
) -> ChannelResult<()> {
    let message_id = envelope
        .headers
        .get("messageId")
        .cloned()
        .unwrap_or_default();

    let mut headers = HashMap::new();
    headers.insert("messageId", message_id);
    headers.insert("contentType", "application/json".to_string());

    let ack = StreamAck {
        code: 200,
        headers,
        message: "OK",
        data,
    };

    ws.send(Message::Text(serde_json::to_string(&ack)?.into()))
        .await?;
    Ok(())
}

fn parse_stream_data(data: &Value) -> Option<Value> {
    match data {
        Value::String(raw) => serde_json::from_str(raw).ok(),
        Value::Object(_) => Some(data.clone()),
        _ => None,
    }
}

fn resolve_chat_id(data: &Value, sender_id: &str) -> String {
    let is_private_chat = data
        .get("conversationType")
        .and_then(|value| {
            value
                .as_str()
                .map(|v| v == "1")
                .or_else(|| value.as_i64().map(|v| v == 1))
        })
        .unwrap_or(true);

    if is_private_chat {
        sender_id.to_string()
    } else {
        data.get("conversationId")
            .and_then(Value::as_str)
            .unwrap_or(sender_id)
            .to_string()
    }
}

fn parse_inbound_event(value: &Value) -> Option<InboundEvent> {
    let sender_id = value
        .get("senderStaffId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let chat_id = resolve_chat_id(value, &sender_id);
    let session_webhook = value
        .get("sessionWebhook")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let event_id = value
        .get("msgId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let msg_type = value
        .get("msgtype")
        .or_else(|| value.get("msgType"))
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "text".to_string());
    let text = extract_dingtalk_message_text(value, &msg_type)?;
    let media_references = extract_dingtalk_media_references(value, &msg_type, &event_id);

    Some(InboundEvent {
        event_id,
        chat_id,
        sender_id,
        session_webhook,
        text,
        media_references,
    })
}

fn extract_dingtalk_message_text(value: &Value, msg_type: &str) -> Option<String> {
    if msg_type == "text" {
        return value
            .pointer("/text/content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
    }

    let fallback = match msg_type {
        "audio" | "voice" => {
            let duration = value
                .pointer("/audio/duration")
                .or_else(|| value.pointer("/voice/duration"))
                .and_then(Value::as_i64)
                .map(|seconds| format!("，时长约 {seconds} 秒"))
                .unwrap_or_default();
            format!(
                "[DingTalk语音消息]\n用户发送了一条语音消息{duration}。当前通道暂不支持直接转写原始语音内容，请引导用户补充文字摘要。"
            )
        }
        "picture" | "image" | "photo" => {
            let title = value
                .pointer("/picture/fileName")
                .or_else(|| value.pointer("/image/fileName"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|name| format!("（{name}）"))
                .unwrap_or_default();
            format!(
                "[DingTalk图片消息{title}]\n用户发送了一张图片。当前通道暂不支持自动下载原图，请引导用户补充图片内容的文字描述。"
            )
        }
        other => format!(
            "[DingTalk非文本消息]\n用户发送了类型为 `{other}` 的消息。当前通道仅保证文本可直接处理，请引导用户补充文字内容。"
        ),
    };

    Some(fallback)
}

fn extract_dingtalk_media_references(
    value: &Value,
    msg_type: &str,
    event_id: &str,
) -> Vec<MediaReference> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "dingtalk.msg_type".to_string(),
        Value::String(msg_type.to_string()),
    );

    match msg_type {
        "audio" | "voice" => {
            if let Some(duration) = value
                .pointer("/audio/duration")
                .or_else(|| value.pointer("/voice/duration"))
                .and_then(Value::as_i64)
            {
                metadata.insert(
                    "dingtalk.duration_seconds".to_string(),
                    Value::from(duration),
                );
            }
            vec![MediaReference {
                source_kind: MediaSourceKind::ChannelInbound,
                filename: None,
                mime_type: None,
                remote_url: None,
                bytes: None,
                message_id: Some(event_id.to_string()),
                metadata,
            }]
        }
        "picture" | "image" | "photo" => {
            let filename = value
                .pointer("/picture/fileName")
                .or_else(|| value.pointer("/image/fileName"))
                .or_else(|| value.pointer("/photo/fileName"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if let Some(download_code) = value
                .pointer("/picture/downloadCode")
                .or_else(|| value.pointer("/image/downloadCode"))
                .and_then(Value::as_str)
                .filter(|code| !code.trim().is_empty())
            {
                metadata.insert(
                    "dingtalk.download_code".to_string(),
                    Value::String(download_code.to_string()),
                );
            }
            vec![MediaReference {
                source_kind: MediaSourceKind::ChannelInbound,
                filename,
                mime_type: None,
                remote_url: None,
                bytes: None,
                message_id: Some(event_id.to_string()),
                metadata,
            }]
        }
        _ => Vec::new(),
    }
}

fn is_sender_allowed(allowlist: &[String], sender_id: &str) -> bool {
    if allowlist.is_empty() {
        return true;
    }

    allowlist
        .iter()
        .any(|entry| entry == "*" || entry == sender_id)
}

fn render_agent_output(output: &ChannelResponse, show_reasoning: bool) -> String {
    let mut content = output.content.trim().to_string();
    if show_reasoning {
        if let Some(reasoning) = output.reasoning.as_ref() {
            let reasoning = reasoning
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            if !reasoning.is_empty() {
                if !content.is_empty() {
                    content.push_str("\n\n");
                }
                content.push_str("**Reasoning**\n");
                content.push_str(&reasoning);
            }
        }
    }
    content
}

#[cfg(test)]
mod tests {
    use super::{
        extract_dingtalk_media_references, extract_dingtalk_message_text, is_sender_allowed,
        parse_inbound_event, parse_stream_data, render_agent_output, resolve_chat_id,
        DingtalkApiClient, InboundEvent,
    };
    use crate::ChannelResponse;

    #[test]
    fn parse_inbound_text_event_reads_dingtalk_shape() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_1",
            "sessionWebhook": "https://example/session",
            "msgId": "mid_1",
            "senderStaffId": "staff_1",
            "text": { "content": "hello" }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse");
        assert_eq!(
            parsed,
            InboundEvent {
                event_id: "mid_1".to_string(),
                chat_id: "cid_1".to_string(),
                sender_id: "staff_1".to_string(),
                session_webhook: "https://example/session".to_string(),
                text: "hello".to_string(),
                media_references: Vec::new(),
            }
        );
    }

    #[test]
    fn parse_inbound_picture_event_as_fallback_text() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_2",
            "sessionWebhook": "https://example/session2",
            "msgId": "mid_2",
            "senderStaffId": "staff_2",
            "msgtype": "picture",
            "picture": { "fileName": "screen.png" }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse picture");
        assert_eq!(parsed.chat_id, "cid_2");
        assert!(parsed.text.contains("图片消息"));
        assert_eq!(parsed.media_references.len(), 1);
        assert_eq!(
            parsed.media_references[0].filename.as_deref(),
            Some("screen.png")
        );
    }

    #[test]
    fn parse_stream_data_supports_string_payload() {
        let frame_data = serde_json::json!("{\"text\":{\"content\":\"hello\"}}");
        let parsed = parse_stream_data(&frame_data).expect("should parse");
        assert_eq!(
            parsed.get("text").and_then(|v| v.get("content")),
            Some(&serde_json::json!("hello"))
        );
    }

    #[test]
    fn resolve_chat_id_handles_private_chat() {
        let data = serde_json::json!({
            "conversationType": "1",
            "conversationId": "cid-group",
        });
        assert_eq!(resolve_chat_id(&data, "staff-1"), "staff-1");
    }

    #[test]
    fn sender_allowlist_supports_wildcard() {
        assert!(is_sender_allowed(&["*".to_string()], "staff-1"));
        assert!(!is_sender_allowed(&["staff-2".to_string()], "staff-1"));
    }

    #[test]
    fn render_reasoning_when_enabled() {
        let output = render_agent_output(
            &ChannelResponse {
                content: "done".to_string(),
                reasoning: Some("step1\nstep2".to_string()),
            },
            true,
        );
        assert!(output.contains("done"));
        assert!(output.contains("**Reasoning**"));
        assert!(output.contains("> step1"));
    }

    #[test]
    fn build_ws_url_appends_ticket() {
        assert_eq!(
            DingtalkApiClient::build_ws_url("wss://example/ws", "abc=="),
            "wss://example/ws?ticket=abc%3D%3D"
        );
        assert_eq!(
            DingtalkApiClient::build_ws_url("wss://example/ws?v=1", "abc=="),
            "wss://example/ws?v=1&ticket=abc%3D%3D"
        );
    }

    #[test]
    fn non_text_messages_fall_back_to_summary() {
        let payload = serde_json::json!({
            "audio": { "duration": 8 }
        });
        let text = extract_dingtalk_message_text(&payload, "audio").expect("audio fallback");
        assert!(text.contains("语音消息"));
    }

    #[test]
    fn audio_message_exposes_media_placeholder() {
        let payload = serde_json::json!({
            "audio": { "duration": 8 }
        });
        let media = extract_dingtalk_media_references(&payload, "audio", "msg-a1");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].message_id.as_deref(), Some("msg-a1"));
        assert_eq!(
            media[0]
                .metadata
                .get("dingtalk.duration_seconds")
                .and_then(serde_json::Value::as_i64),
            Some(8)
        );
    }
}
