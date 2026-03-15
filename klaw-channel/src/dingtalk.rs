use crate::{Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use klaw_archive::{
    open_default_archive_service, ArchiveIngestInput, ArchiveService, ArchiveSourceKind,
};
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
const ACCESS_TOKEN_PATH: &str = "/v1.0/oauth2/accessToken";
const MESSAGE_FILE_DOWNLOAD_PATH: &str = "/v1.0/robot/messageFiles/download";
const INLINE_MEDIA_MAX_BYTES: usize = 8 * 1024 * 1024;
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
        debug!(payload = ?payload, "received dingtalk raw subscription event");

        let Some(mut inbound) = parse_inbound_event(&payload) else {
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

        let session_key = format!("dingtalk:{}:{}", self.config.account_id, inbound.chat_id);
        self.materialize_media_references(&session_key, &mut inbound)
            .await;

        let maybe_output = runtime
            .submit(ChannelRequest {
                channel: self.name().to_string(),
                input: inbound.text.clone(),
                session_key,
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

    async fn materialize_media_references(&self, session_key: &str, inbound: &mut InboundEvent) {
        if inbound.media_references.is_empty() {
            return;
        }
        let has_downloadable_media = inbound.media_references.iter().any(|media| {
            media
                .metadata
                .get("dingtalk.download_code")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
        });
        if !has_downloadable_media {
            return;
        }

        let archive_service = match open_default_archive_service().await {
            Ok(service) => service,
            Err(err) => {
                warn!(
                    event_id = inbound.event_id.as_str(),
                    error = %err,
                    "failed to open archive service for dingtalk media ingestion"
                );
                return;
            }
        };

        let access_token = match self
            .client
            .fetch_access_token(&self.config.client_id, &self.config.client_secret)
            .await
        {
            Ok(token) => token,
            Err(err) => {
                warn!(
                    event_id = inbound.event_id.as_str(),
                    error = %err,
                    "failed to fetch dingtalk access token for media download"
                );
                return;
            }
        };

        for media in &mut inbound.media_references {
            let download_code = media
                .metadata
                .get("dingtalk.download_code")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            let Some(download_code) = download_code else {
                continue;
            };

            let bytes = match self
                .client
                .download_message_file(&access_token, &download_code)
                .await
            {
                Ok(bytes) => bytes,
                Err(err) => {
                    warn!(
                        event_id = inbound.event_id.as_str(),
                        error = %err,
                        "failed to download dingtalk media content"
                    );
                    continue;
                }
            };

            let metadata =
                serde_json::to_value(media.metadata.clone()).unwrap_or_else(|_| Value::Null);
            let ingest_input = ArchiveIngestInput {
                source_kind: ArchiveSourceKind::from(media.source_kind),
                filename: media.filename.clone(),
                declared_mime_type: media.mime_type.clone(),
                session_key: Some(session_key.to_string()),
                channel: Some(self.name().to_string()),
                chat_id: Some(inbound.chat_id.clone()),
                message_id: Some(inbound.event_id.clone()),
                metadata,
            };

            match archive_service.ingest_bytes(ingest_input, &bytes).await {
                Ok(record) => {
                    media.message_id = Some(inbound.event_id.clone());
                    if media.filename.is_none() {
                        media.filename = record.original_filename.clone();
                    }
                    if media.mime_type.is_none() {
                        media.mime_type = record.mime_type.clone();
                    }
                    if bytes.len() <= INLINE_MEDIA_MAX_BYTES {
                        let mime_for_inline = media
                            .mime_type
                            .clone()
                            .or_else(|| record.mime_type.clone())
                            .unwrap_or_else(|| "application/octet-stream".to_string());
                        media.remote_url = Some(format!(
                            "data:{mime_for_inline};base64,{}",
                            BASE64_STANDARD.encode(&bytes)
                        ));
                        media
                            .metadata
                            .insert("dingtalk.inline_media".to_string(), Value::Bool(true));
                    } else {
                        media
                            .metadata
                            .insert("dingtalk.inline_media".to_string(), Value::Bool(false));
                        media.metadata.insert(
                            "dingtalk.inline_media_skipped_bytes".to_string(),
                            Value::from(bytes.len() as i64),
                        );
                    }
                    media
                        .metadata
                        .insert("archive.id".to_string(), Value::String(record.id.clone()));
                    media.metadata.insert(
                        "archive.storage_rel_path".to_string(),
                        Value::String(record.storage_rel_path),
                    );
                    media.metadata.insert(
                        "archive.size_bytes".to_string(),
                        Value::from(record.size_bytes),
                    );
                    if let Some(mime_type) = record.mime_type {
                        media
                            .metadata
                            .insert("archive.mime_type".to_string(), Value::String(mime_type));
                    }
                }
                Err(err) => {
                    warn!(
                        event_id = inbound.event_id.as_str(),
                        error = %err,
                        "failed to ingest dingtalk media into archive"
                    );
                }
            }
        }
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

    async fn fetch_access_token(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> ChannelResult<String> {
        let url = format!("{DINGTALK_OPEN_API_BASE}{ACCESS_TOKEN_PATH}");
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "clientId": client_id,
                "clientSecret": client_secret,
            }))
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk access token request failed: HTTP {} body={}",
                status, body
            )
            .into());
        }

        body.get("accessToken")
            .or_else(|| body.get("access_token"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| "missing accessToken from dingtalk access token response".into())
    }

    async fn download_message_file(
        &self,
        access_token: &str,
        download_code: &str,
    ) -> ChannelResult<Vec<u8>> {
        let url = format!(
            "{DINGTALK_OPEN_API_BASE}{MESSAGE_FILE_DOWNLOAD_PATH}?downloadCode={}",
            urlencoding::encode(download_code)
        );
        let response = self
            .http
            .get(url)
            .header("x-acs-dingtalk-access-token", access_token)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(format!(
                "dingtalk media download failed: HTTP {} body={}",
                status, body
            )
            .into());
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
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
    if msg_type == "richtext" || msg_type == "rich_text" {
        if let Some(rich_blocks) = value.pointer("/content/richText").and_then(Value::as_array) {
            let rich_text = rich_blocks
                .iter()
                .filter_map(|block| {
                    let block_type = block
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(|ty| ty.to_ascii_lowercase())
                        .unwrap_or_default();
                    if block_type != "text" {
                        return None;
                    }
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
                .join("");
            let normalized_text = rich_text.trim();
            if !normalized_text.is_empty() {
                return Some(normalized_text.to_string());
            }

            let picture_count = rich_blocks
                .iter()
                .filter(|block| {
                    block
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(|ty| ty.eq_ignore_ascii_case("picture"))
                        .unwrap_or(false)
                })
                .count();
            if picture_count > 0 {
                return Some(format!(
                    "[DingTalk富文本消息]\n用户发送了 {picture_count} 张图片。请结合图片内容回答用户。"
                ));
            }
        }
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
        "richtext" | "rich_text" => {
            let Some(rich_blocks) = value.pointer("/content/richText").and_then(Value::as_array)
            else {
                return Vec::new();
            };

            rich_blocks
                .iter()
                .enumerate()
                .filter_map(|(index, block)| {
                    let block_type = block
                        .get("type")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .map(|ty| ty.to_ascii_lowercase())
                        .unwrap_or_default();
                    if block_type != "picture" && block_type != "image" {
                        return None;
                    }

                    let filename = block
                        .get("fileName")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    let mut item_metadata = metadata.clone();
                    item_metadata.insert("dingtalk.richtext_index".to_string(), Value::from(index));
                    if let Some(download_code) = block
                        .get("downloadCode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|code| !code.is_empty())
                    {
                        item_metadata.insert(
                            "dingtalk.download_code".to_string(),
                            Value::String(download_code.to_string()),
                        );
                    }
                    if let Some(picture_download_code) = block
                        .get("pictureDownloadCode")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|code| !code.is_empty())
                    {
                        item_metadata.insert(
                            "dingtalk.picture_download_code".to_string(),
                            Value::String(picture_download_code.to_string()),
                        );
                    }
                    Some(MediaReference {
                        source_kind: MediaSourceKind::ChannelInbound,
                        filename,
                        mime_type: None,
                        remote_url: None,
                        bytes: None,
                        message_id: Some(event_id.to_string()),
                        metadata: item_metadata,
                    })
                })
                .collect()
        }
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
            if let Some(picture_download_code) = value
                .pointer("/picture/pictureDownloadCode")
                .or_else(|| value.pointer("/image/pictureDownloadCode"))
                .and_then(Value::as_str)
                .filter(|code| !code.trim().is_empty())
            {
                metadata.insert(
                    "dingtalk.picture_download_code".to_string(),
                    Value::String(picture_download_code.to_string()),
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
    fn parse_inbound_richtext_event_extracts_text_and_pictures() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_3",
            "sessionWebhook": "https://example/session3",
            "msgId": "mid_3",
            "senderStaffId": "staff_3",
            "msgtype": "richText",
            "content": {
                "richText": [
                    { "type": "picture", "downloadCode": "code-1", "pictureDownloadCode": "pcode-1" },
                    { "type": "text", "text": "\n" },
                    { "type": "picture", "downloadCode": "code-2", "pictureDownloadCode": "pcode-2" },
                    { "type": "text", "text": "\n这是啥" }
                ]
            }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse richText");
        assert_eq!(parsed.text, "这是啥");
        assert_eq!(parsed.media_references.len(), 2);
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.download_code")
                .and_then(serde_json::Value::as_str),
            Some("code-1")
        );
        assert_eq!(
            parsed.media_references[1]
                .metadata
                .get("dingtalk.picture_download_code")
                .and_then(serde_json::Value::as_str),
            Some("pcode-2")
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
