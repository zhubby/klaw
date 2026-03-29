use crate::{
    Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, LocalAttachmentPolicy,
    OutboundAttachment,
    manager::{ChannelKind, ManagedChannelDriver},
    media::{
        ArchiveMediaIngestContext, DEFAULT_INLINE_MEDIA_MAX_BYTES, attach_declared_media_metadata,
        build_media_reference, first_object_string_value, first_string_value,
        ingest_media_reference_bytes, resolve_metadata_value_candidates,
    },
    outbound::resolve_outbound_attachment,
    render::{OutputRenderStyle, render_agent_output},
};
use futures_util::{SinkExt, StreamExt};
use klaw_archive::open_default_archive_service;
use klaw_config::{DingtalkConfig, LocalAttachmentConfig};
use klaw_core::{MediaReference, MediaSourceKind};
use klaw_util::{default_data_dir, workspace_dir};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::watch;
use tokio::time::{self, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, info, trace, warn};

const DINGTALK_OPEN_API_BASE: &str = "https://api.dingtalk.com";
const DINGTALK_OAPI_BASE: &str = "https://oapi.dingtalk.com";
const CONNECTION_OPEN_PATH: &str = "/v1.0/gateway/connections/open";
const ACCESS_TOKEN_PATH: &str = "/v1.0/oauth2/accessToken";
const MESSAGE_FILE_DOWNLOAD_PATH: &str = "/v1.0/robot/messageFiles/download";
const OAPI_MEDIA_UPLOAD_PATH: &str = "/media/upload";
const OAPI_ASR_TRANSLATE_PATH: &str = "/topapi/asr/voice/translate";
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const WS_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
const EVENT_DEDUP_TTL: Duration = Duration::from_secs(60 * 60);
const EVENT_DEDUP_MAX_ENTRIES: usize = 20_000;
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const APPROVAL_APPROVE_ACTION: &str = "approve";
const APPROVAL_REJECT_ACTION: &str = "reject";
static RUSTLS_PROVIDER_INSTALLED: OnceLock<()> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DingtalkChannelConfig {
    pub account_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub bot_title: String,
    pub show_reasoning: bool,
    pub stream_output: bool,
    pub allowlist: Vec<String>,
    pub local_attachments: LocalAttachmentConfig,
    pub proxy: DingtalkProxyConfig,
}

impl Default for DingtalkChannelConfig {
    fn default() -> Self {
        Self {
            account_id: "default".to_string(),
            client_id: String::new(),
            client_secret: String::new(),
            bot_title: "Klaw".to_string(),
            show_reasoning: false,
            stream_output: false,
            allowlist: Vec::new(),
            local_attachments: LocalAttachmentConfig::default(),
            proxy: DingtalkProxyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DingtalkProxyConfig {
    pub enabled: bool,
    pub url: String,
}

impl Default for DingtalkProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DingtalkChannel {
    config: DingtalkChannelConfig,
    client: DingtalkApiClient,
    event_deduper: EventDeduper,
}

impl DingtalkChannel {
    pub fn from_app_config(
        config: DingtalkConfig,
        local_attachments: LocalAttachmentConfig,
    ) -> ChannelResult<Self> {
        Self::new(DingtalkChannelConfig {
            account_id: config.id,
            client_id: config.client_id,
            client_secret: config.client_secret,
            bot_title: config.bot_title,
            show_reasoning: config.show_reasoning,
            stream_output: config.stream_output,
            allowlist: config.allowlist,
            local_attachments,
            proxy: DingtalkProxyConfig {
                enabled: config.proxy.enabled,
                url: config.proxy.url,
            },
        })
    }

    pub fn new(config: DingtalkChannelConfig) -> ChannelResult<Self> {
        let client = DingtalkApiClient::new(&config.proxy)?;
        Ok(Self {
            config,
            client,
            event_deduper: EventDeduper::new(EVENT_DEDUP_TTL, EVENT_DEDUP_MAX_ENTRIES),
        })
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

    pub async fn run_until_shutdown(
        &mut self,
        runtime: &dyn ChannelRuntime,
        shutdown: &mut watch::Receiver<bool>,
    ) -> ChannelResult<()> {
        self.validate_config()?;
        ensure_rustls_crypto_provider();
        info!("dingtalk channel started");

        loop {
            if *shutdown.borrow() {
                info!("dingtalk shutdown requested before reconnect");
                return Ok(());
            }
            let ticket = match self
                .client
                .open_stream_connection(&self.config.client_id, &self.config.client_secret)
                .await
            {
                Ok(ticket) => ticket,
                Err(err) => {
                    warn!(error = %err, "failed to open dingtalk stream connection");
                    tokio::select! {
                        _ = time::sleep(RECONNECT_DELAY) => {}
                        changed = shutdown.changed() => {
                            if changed.is_ok() && *shutdown.borrow() {
                                info!("dingtalk shutdown requested while reconnect waiting");
                                return Ok(());
                            }
                        }
                    }
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
                    tokio::select! {
                        _ = time::sleep(RECONNECT_DELAY) => {}
                        changed = shutdown.changed() => {
                            if changed.is_ok() && *shutdown.borrow() {
                                info!("dingtalk shutdown requested while reconnect waiting");
                                return Ok(());
                            }
                        }
                    }
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
                    let mut keepalive_tick = time::interval(WS_KEEPALIVE_INTERVAL);
                    let mut cron_job: Option<Pin<Box<dyn Future<Output = ()> + '_>>> = None;
                    let mut runtime_job: Option<Pin<Box<dyn Future<Output = ()> + '_>>> = None;

                    loop {
                        tokio::select! {
                            changed = shutdown.changed() => {
                                if changed.is_ok() && *shutdown.borrow() {
                                    info!("dingtalk shutdown requested, closing websocket");
                                    if let Err(err) = ws.send(Message::Close(None)).await {
                                        warn!(error = %err, "failed to send dingtalk websocket close frame");
                                    }
                                    let _ = time::timeout(Duration::from_secs(1), ws.next()).await;
                                    return Ok(());
                                }
                            }
                            _ = async {
                                if let Some(job) = cron_job.as_mut() {
                                    job.await;
                                }
                            }, if cron_job.is_some() => {
                                cron_job = None;
                            }
                            _ = async {
                                if let Some(job) = runtime_job.as_mut() {
                                    job.await;
                                }
                            }, if runtime_job.is_some() => {
                                runtime_job = None;
                            }
                            _ = cron_tick.tick() => {
                                if cron_job.is_none() {
                                    cron_job = Some(Box::pin(runtime.on_cron_tick()));
                                }
                            }
                            _ = runtime_tick.tick() => {
                                if runtime_job.is_none() {
                                    runtime_job = Some(Box::pin(runtime.on_runtime_tick()));
                                }
                            }
                            _ = keepalive_tick.tick() => {
                                if let Err(err) = ws.send(Message::Ping(Vec::new().into())).await {
                                    warn!(error = %err, "failed to send dingtalk websocket keepalive ping");
                                    break;
                                }
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
                                        trace!("received dingtalk websocket pong");
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

            tokio::select! {
                _ = time::sleep(RECONNECT_DELAY) => {}
                changed = shutdown.changed() => {
                    if changed.is_ok() && *shutdown.borrow() {
                        info!("dingtalk shutdown requested while reconnect waiting");
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ManagedChannelDriver for DingtalkChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Dingtalk
    }

    fn instance_id(&self) -> &str {
        &self.config.account_id
    }

    async fn run_until_shutdown(
        &mut self,
        runtime: &dyn ChannelRuntime,
        shutdown: &mut watch::Receiver<bool>,
    ) -> ChannelResult<()> {
        DingtalkChannel::run_until_shutdown(self, runtime, shutdown).await
    }
}

pub async fn send_session_webhook_markdown_via_proxy(
    proxy: &DingtalkProxyConfig,
    session_webhook: &str,
    title: &str,
    text: &str,
) -> ChannelResult<()> {
    let client = DingtalkApiClient::new(proxy)?;
    client
        .send_session_webhook_markdown(session_webhook, title, text)
        .await
}

fn callback_runtime_metadata(
    session_webhook: Option<&str>,
    bot_title: &str,
) -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    if let Some(session_webhook) = session_webhook
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        metadata.insert(
            "channel.dingtalk.session_webhook".to_string(),
            serde_json::Value::String(session_webhook.to_string()),
        );
        metadata.insert(
            "channel.dingtalk.bot_title".to_string(),
            serde_json::Value::String(bot_title.to_string()),
        );
        metadata.insert(
            "channel.delivery_mode".to_string(),
            serde_json::Value::String("direct_reply".to_string()),
        );
    }
    metadata
}

#[async_trait::async_trait(?Send)]
impl Channel for DingtalkChannel {
    fn name(&self) -> &'static str {
        "dingtalk"
    }

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        self.run_until_shutdown(runtime, &mut shutdown_rx).await
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

        if let Some(card_callback) = parse_card_callback_event(&payload) {
            if let Some(event_id) = card_callback.event_id.as_deref() {
                if !self.event_deduper.insert_if_new(event_id) {
                    debug!(event_id, "ignoring duplicated dingtalk card callback");
                    return send_ack(ws, envelope, "").await;
                }
            }

            if !is_sender_allowed(&self.config.allowlist, &card_callback.sender_id) {
                warn!(
                    sender = card_callback.sender_id.as_str(),
                    "dingtalk sender blocked by allowlist"
                );
                return send_ack(ws, envelope, "").await;
            }

            let command = format!(
                "/{} {}",
                card_callback.action.as_command(),
                card_callback.approval_id
            );
            let session_key = format!(
                "dingtalk:{}:{}",
                self.config.account_id, card_callback.chat_id
            );
            let maybe_output = runtime
                .submit(ChannelRequest {
                    channel: self.name().to_string(),
                    input: command,
                    session_key,
                    chat_id: card_callback.chat_id.clone(),
                    media_references: Vec::new(),
                    metadata: callback_runtime_metadata(
                        card_callback.session_webhook.as_deref(),
                        &self.config.bot_title,
                    ),
                })
                .await;

            match maybe_output {
                Ok(Some(output)) => {
                    if let Some(webhook) = card_callback.session_webhook.as_deref() {
                        let body = render_agent_output(
                            &output,
                            self.config.show_reasoning,
                            OutputRenderStyle::Markdown,
                        );
                        if let Err(err) = self
                            .client
                            .send_session_webhook_markdown(webhook, &self.config.bot_title, &body)
                            .await
                        {
                            warn!(
                                chat_id = card_callback.chat_id.as_str(),
                                error = %err,
                                "failed to send dingtalk callback reply"
                            );
                        }
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    return Err(err);
                }
            }

            return send_ack(ws, envelope, "").await;
        }

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

        if !self.event_deduper.insert_if_new(inbound.event_id.as_str()) {
            debug!(
                event_id = inbound.event_id.as_str(),
                "ignoring duplicated dingtalk event"
            );
            return send_ack(ws, envelope, "").await;
        }

        let session_key = format!("dingtalk:{}:{}", self.config.account_id, inbound.chat_id);
        self.materialize_media_references(&session_key, &mut inbound)
            .await;
        let total_media = inbound.media_references.len();
        let inline_media = inbound
            .media_references
            .iter()
            .filter(|media| {
                media
                    .remote_url
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|value| !value.is_empty())
            })
            .count();
        if total_media > 0 {
            info!(
                event_id = inbound.event_id.as_str(),
                total_media, inline_media, "dingtalk inbound media materialized"
            );
        }

        let maybe_output = runtime
            .submit(ChannelRequest {
                channel: self.name().to_string(),
                input: inbound.text.clone(),
                session_key,
                chat_id: inbound.chat_id.clone(),
                media_references: inbound.media_references.clone(),
                metadata: callback_runtime_metadata(
                    Some(&inbound.session_webhook),
                    &self.config.bot_title,
                ),
            })
            .await;

        match maybe_output {
            Ok(Some(output)) => {
                if let Some(approval_id) = extract_approval_id_for_action_card(&output) {
                    let body = build_approval_action_card_body(&output, &approval_id);
                    if let Err(err) = self
                        .client
                        .send_session_webhook_action_card(
                            &inbound.session_webhook,
                            "审批请求",
                            &body,
                            &approval_id,
                        )
                        .await
                    {
                        warn!(
                            chat_id = inbound.chat_id.as_str(),
                            approval_id = approval_id.as_str(),
                            error = %err,
                            "failed to send dingtalk approval action card; fallback to markdown"
                        );
                        let markdown = render_agent_output(
                            &output,
                            self.config.show_reasoning,
                            OutputRenderStyle::Markdown,
                        );
                        if let Err(err) = self
                            .client
                            .send_session_webhook_markdown(
                                &inbound.session_webhook,
                                &self.config.bot_title,
                                &markdown,
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
                } else {
                    let body = render_agent_output(
                        &output,
                        self.config.show_reasoning,
                        OutputRenderStyle::Markdown,
                    );
                    if !body.trim().is_empty() {
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
                    self.send_attachments(&inbound.session_webhook, &inbound.chat_id, &output);
                }
            }
            Ok(None) => {}
            Err(err) => {
                return Err(err);
            }
        }

        send_ack(ws, envelope, "").await
    }

    async fn materialize_media_references(&self, session_key: &str, inbound: &mut InboundEvent) {
        if inbound.media_references.is_empty() {
            return;
        }
        let has_downloadable_media = inbound
            .media_references
            .iter()
            .any(|media| !resolve_download_code_candidates(&media.metadata).is_empty());
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
        let mut attempted_audio_asr = false;

        for media in &mut inbound.media_references {
            let download_candidates = resolve_download_code_candidates(&media.metadata);
            if download_candidates.is_empty() {
                continue;
            }
            let mut bytes: Option<Vec<u8>> = None;
            let mut selected_code_source: Option<&'static str> = None;
            let mut last_download_error: Option<String> = None;
            for (download_code, code_source) in download_candidates {
                match self
                    .client
                    .download_message_file(
                        &access_token,
                        inbound.robot_code.as_str(),
                        download_code.as_str(),
                    )
                    .await
                {
                    Ok(downloaded) => {
                        bytes = Some(downloaded);
                        selected_code_source = Some(code_source);
                        break;
                    }
                    Err(err) => {
                        last_download_error = Some(err.to_string());
                        warn!(
                            event_id = inbound.event_id.as_str(),
                            code_source,
                            error = %err,
                            "failed to download dingtalk media content with candidate code"
                        );
                    }
                }
            }
            let Some(bytes) = bytes else {
                if let Some(error) = last_download_error {
                    warn!(
                        event_id = inbound.event_id.as_str(),
                        error = error.as_str(),
                        "failed to download dingtalk media content after trying all candidate codes"
                    );
                }
                continue;
            };
            let code_source = selected_code_source.unwrap_or("unknown");
            media.metadata.insert(
                "dingtalk.download_code_source".to_string(),
                Value::String(code_source.to_string()),
            );
            if !attempted_audio_asr
                && inbound.audio_recognition.is_none()
                && matches!(inbound.msg_type.as_str(), "audio" | "voice")
            {
                attempted_audio_asr = true;
                match self.client.transcribe_audio(&access_token, &bytes).await {
                    Ok(transcript) => {
                        info!(
                            event_id = inbound.event_id.as_str(),
                            transcript_chars = transcript.chars().count(),
                            "dingtalk audio transcript generated via asr"
                        );
                        inbound.audio_recognition = Some(transcript.clone());
                        inbound.text = transcript;
                    }
                    Err(err) => {
                        warn!(
                            event_id = inbound.event_id.as_str(),
                            error = %err,
                            "dingtalk asr transcript failed, keeping fallback audio summary"
                        );
                    }
                }
            }

            match ingest_media_reference_bytes(
                &archive_service,
                ArchiveMediaIngestContext {
                    session_key,
                    channel: self.name(),
                    chat_id: inbound.chat_id.as_str(),
                    message_id: inbound.event_id.as_str(),
                },
                media,
                &bytes,
                DEFAULT_INLINE_MEDIA_MAX_BYTES,
                "dingtalk.inline_media",
                "dingtalk.inline_media_skipped_bytes",
            )
            .await
            {
                Ok(record) => {
                    info!(
                        event_id = inbound.event_id.as_str(),
                        archive_id = record.id.as_str(),
                        storage_rel_path = record.storage_rel_path.as_str(),
                        media_kind = record.media_kind.as_str(),
                        mime_type = record.mime_type.as_deref().unwrap_or("unknown"),
                        size_bytes = record.size_bytes,
                        "dingtalk media archived"
                    );
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

    fn send_attachments(&self, session_webhook: &str, chat_id: &str, output: &ChannelResponse) {
        if output.attachments.is_empty() {
            return;
        }
        let client = self.client.clone();
        let config = self.config.clone();
        let session_webhook = session_webhook.to_string();
        let chat_id = chat_id.to_string();
        let attachments = output.attachments.clone();
        let local_policy = match self.local_attachment_policy() {
            Ok(policy) => policy,
            Err(error) => {
                warn!(chat_id, error = %error, "failed to resolve dingtalk local attachment policy");
                return;
            }
        };
        tokio::task::spawn_local(async move {
            deliver_dingtalk_attachments(
                client,
                config,
                session_webhook,
                chat_id,
                attachments,
                local_policy,
            )
            .await;
        });
    }
}

async fn deliver_dingtalk_attachments(
    client: DingtalkApiClient,
    config: DingtalkChannelConfig,
    session_webhook: String,
    chat_id: String,
    attachments: Vec<OutboundAttachment>,
    local_policy: LocalAttachmentPolicy,
) {
    let archive_service = match open_default_archive_service().await {
        Ok(service) => service,
        Err(error) => {
            warn!(
                chat_id,
                error = %error,
                "failed to open archive service for dingtalk outbound attachments"
            );
            return;
        }
    };

    let access_token = match client
        .fetch_access_token(&config.client_id, &config.client_secret)
        .await
    {
        Ok(token) => token,
        Err(error) => {
            warn!(
                chat_id,
                error = %error,
                "failed to fetch dingtalk access token for outbound attachments"
            );
            return;
        }
    };

    for attachment in &attachments {
        let resolved =
            match resolve_outbound_attachment(&archive_service, &local_policy, attachment).await {
                Ok(resolved) => resolved,
                Err(error) => {
                    warn!(
                            chat_id,
                            source = ?attachment.source,
                            error = %error,
                            "failed to resolve dingtalk outbound attachment"
                    );
                    continue;
                }
            };
        debug!(
            chat_id,
            source = resolved.source_label.as_str(),
            kind = ?resolved.kind,
            filename = resolved.filename.as_str(),
            mime_type = resolved.mime_type.as_deref().unwrap_or("unknown"),
            size_bytes = resolved.bytes.len(),
            "resolved dingtalk outbound attachment"
        );

        let result = match resolved.kind {
            crate::OutboundAttachmentKind::Image => {
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    filename = resolved.filename.as_str(),
                    size_bytes = resolved.bytes.len(),
                    "uploading dingtalk image attachment"
                );
                let media_id = match client
                    .upload_media(
                        &access_token,
                        &resolved.bytes,
                        "image",
                        &resolved.filename,
                        resolved.mime_type.as_deref(),
                    )
                    .await
                {
                    Ok(media_id) => media_id,
                    Err(error) => {
                        warn!(
                            chat_id,
                            source = resolved.source_label.as_str(),
                            error = %error,
                            "failed to upload dingtalk image attachment"
                        );
                        continue;
                    }
                };
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    media_id = media_id.as_str(),
                    "uploaded dingtalk image attachment"
                );
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    media_id = media_id.as_str(),
                    "sending dingtalk image attachment message"
                );
                client
                    .send_session_webhook_image_markdown(
                        &session_webhook,
                        &config.bot_title,
                        &media_id,
                        resolved.caption.as_deref(),
                    )
                    .await
            }
            crate::OutboundAttachmentKind::File => {
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    filename = resolved.filename.as_str(),
                    size_bytes = resolved.bytes.len(),
                    "uploading dingtalk file attachment"
                );
                let media_id = match client
                    .upload_media(
                        &access_token,
                        &resolved.bytes,
                        "file",
                        &resolved.filename,
                        resolved.mime_type.as_deref(),
                    )
                    .await
                {
                    Ok(media_id) => media_id,
                    Err(error) => {
                        warn!(
                            chat_id,
                            source = resolved.source_label.as_str(),
                            error = %error,
                            "failed to upload dingtalk file attachment"
                        );
                        continue;
                    }
                };
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    media_id = media_id.as_str(),
                    "uploaded dingtalk file attachment"
                );
                debug!(
                    chat_id,
                    source = resolved.source_label.as_str(),
                    media_id = media_id.as_str(),
                    "sending dingtalk file attachment message"
                );
                client
                    .send_session_webhook_file(&session_webhook, &media_id)
                    .await
            }
        };

        if let Err(error) = result {
            warn!(
                chat_id,
                source = resolved.source_label.as_str(),
                error = %error,
                "failed to send dingtalk outbound attachment"
            );
        } else {
            debug!(
                chat_id,
                source = resolved.source_label.as_str(),
                "sent dingtalk outbound attachment"
            );
        }
    }
}

fn resolve_local_attachment_policy(
    config: &LocalAttachmentConfig,
) -> ChannelResult<LocalAttachmentPolicy> {
    let root = default_data_dir().ok_or_else(|| "failed to resolve home dir".to_string())?;
    let workspace = workspace_dir(&root);
    std::fs::create_dir_all(&workspace)?;
    let workspace_root = std::fs::canonicalize(&workspace)?;
    let allowlist = config
        .allowlist
        .iter()
        .map(|path| PathBuf::from(path.trim()))
        .collect();
    Ok(LocalAttachmentPolicy {
        workspace_root,
        allowlist,
        max_bytes: config.max_bytes,
    })
}

impl DingtalkChannel {
    fn local_attachment_policy(&self) -> ChannelResult<LocalAttachmentPolicy> {
        resolve_local_attachment_policy(&self.config.local_attachments)
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
    fn new(proxy: &DingtalkProxyConfig) -> ChannelResult<Self> {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .timeout(HTTP_REQUEST_TIMEOUT);
        if proxy.enabled {
            let proxy_url = proxy.url.trim();
            if proxy_url.is_empty() {
                return Err("dingtalk proxy.url is required when proxy.enabled=true".into());
            }
            builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
        }
        let http = builder.build()?;
        Ok(Self { http })
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
                "appKey": client_id,
                "appSecret": client_secret,
                // Keep compatibility fields for gateways that still accept clientId/clientSecret.
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
        robot_code: &str,
        download_code: &str,
    ) -> ChannelResult<Vec<u8>> {
        let url = format!("{DINGTALK_OPEN_API_BASE}{MESSAGE_FILE_DOWNLOAD_PATH}");
        let response = self
            .http
            .post(url)
            .header("x-acs-dingtalk-access-token", access_token)
            .json(&serde_json::json!({
                "downloadCode": download_code,
                "robotCode": robot_code,
            }))
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

        let body: Value = response.json().await?;
        let download_url = body
            .get("downloadUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!("missing downloadUrl in dingtalk download response body={body}")
            })?;
        let file_response = self.http.get(download_url).send().await?;
        if !file_response.status().is_success() {
            let status = file_response.status();
            let body = file_response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            return Err(format!(
                "dingtalk media file fetch failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        let bytes = file_response.bytes().await?;
        Ok(bytes.to_vec())
    }

    async fn transcribe_audio(
        &self,
        access_token: &str,
        audio_bytes: &[u8],
    ) -> ChannelResult<String> {
        let media_id = self
            .upload_media(
                access_token,
                audio_bytes,
                "voice",
                "voice.wav",
                Some("audio/wav"),
            )
            .await?;
        let url = format!(
            "{DINGTALK_OAPI_BASE}{OAPI_ASR_TRANSLATE_PATH}?access_token={}",
            urlencoding::encode(access_token)
        );
        let response = self
            .http
            .post(url)
            .json(&serde_json::json!({
                "media_id": media_id,
            }))
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk asr translate request failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        let errcode = body.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = body
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!(
                "dingtalk asr failed: errcode={errcode} errmsg={errmsg} body={body}"
            )
            .into());
        }
        body.get("result")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("missing result in dingtalk asr response body={body}").into())
    }

    async fn upload_media(
        &self,
        access_token: &str,
        bytes: &[u8],
        media_type: &str,
        filename: &str,
        mime_type: Option<&str>,
    ) -> ChannelResult<String> {
        debug!(
            media_type = media_type,
            filename = filename.trim(),
            mime_type = mime_type.unwrap_or("unknown"),
            size_bytes = bytes.len(),
            "calling dingtalk media upload"
        );
        let url = format!(
            "{DINGTALK_OAPI_BASE}{OAPI_MEDIA_UPLOAD_PATH}?access_token={}&type={}",
            urlencoding::encode(access_token),
            urlencoding::encode(media_type),
        );
        let mut part =
            reqwest::multipart::Part::bytes(bytes.to_vec()).file_name(filename.trim().to_string());
        if let Some(mime_type) = mime_type.map(str::trim).filter(|value| !value.is_empty()) {
            part = part.mime_str(mime_type)?;
        }
        let form = reqwest::multipart::Form::new().part("media", part);
        let response = self.http.post(url).multipart(form).send().await?;
        let status = response.status();
        let body: Value = response.json().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk media upload failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        let errcode = body.get("errcode").and_then(Value::as_i64).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = body
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!(
                "dingtalk media upload failed: errcode={errcode} errmsg={errmsg} body={body}"
            )
            .into());
        }
        let Some(media_id) = body
            .get("media_id")
            .or_else(|| body.get("mediaId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
        else {
            return Err(
                format!("missing media_id in dingtalk media upload response body={body}").into(),
            );
        };
        debug!(
            media_type = media_type,
            filename = filename.trim(),
            media_id = media_id.as_str(),
            "dingtalk media upload succeeded"
        );
        Ok(media_id)
    }

    fn ensure_session_webhook_success(body: &str, context: &str) -> ChannelResult<()> {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let Ok(payload) = serde_json::from_str::<Value>(trimmed) else {
            return Ok(());
        };
        let Some(errcode) = payload.get("errcode").and_then(Value::as_i64) else {
            return Ok(());
        };
        if errcode == 0 {
            return Ok(());
        }
        let errmsg = payload
            .get("errmsg")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        Err(format!(
            "dingtalk session webhook {context} failed: errcode={errcode} errmsg={errmsg} body={payload}"
        )
        .into())
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
        Self::ensure_session_webhook_success(&body, "markdown send")?;

        Ok(())
    }

    async fn send_session_webhook_image_markdown(
        &self,
        session_webhook: &str,
        title: &str,
        media_id: &str,
        caption: Option<&str>,
    ) -> ChannelResult<()> {
        let mut lines = Vec::new();
        if let Some(caption) = caption.map(str::trim).filter(|value| !value.is_empty()) {
            lines.push(caption.to_string());
        }
        lines.push(format!("![]({})", media_id.trim()));
        self.send_session_webhook_markdown(session_webhook, title, &lines.join("\n\n"))
            .await
    }

    async fn send_session_webhook_file(
        &self,
        session_webhook: &str,
        media_id: &str,
    ) -> ChannelResult<()> {
        let response = self
            .http
            .post(session_webhook)
            .json(&serde_json::json!({
                "msgtype": "file",
                "file": {
                    "mediaId": media_id,
                    "media_id": media_id,
                }
            }))
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk session webhook file send failed with HTTP {}: {}",
                status, body
            )
            .into());
        }
        Self::ensure_session_webhook_success(&body, "file send")?;

        Ok(())
    }

    async fn send_session_webhook_action_card(
        &self,
        session_webhook: &str,
        title: &str,
        text: &str,
        approval_id: &str,
    ) -> ChannelResult<()> {
        let approve_url = dingtalk_command_action_url(APPROVAL_APPROVE_ACTION, approval_id);
        let reject_url = dingtalk_command_action_url(APPROVAL_REJECT_ACTION, approval_id);
        let response = self
            .http
            .post(session_webhook)
            .json(&serde_json::json!({
                "msgtype": "actionCard",
                "actionCard": {
                    "title": title,
                    "text": text,
                    "btnOrientation": "1",
                    "btns": [
                        {
                            "title": "批准",
                            "actionURL": approve_url
                        },
                        {
                            "title": "拒绝",
                            "actionURL": reject_url
                        }
                    ]
                }
            }))
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(format!(
                "dingtalk session webhook actionCard send failed with HTTP {}: {}",
                status, body
            )
            .into());
        }
        Self::ensure_session_webhook_success(&body, "actionCard send")?;
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
    robot_code: String,
    msg_type: String,
    sender_id: String,
    session_webhook: String,
    text: String,
    audio_recognition: Option<String>,
    media_references: Vec<MediaReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalAction {
    Approve,
    Reject,
}

impl ApprovalAction {
    fn as_command(self) -> &'static str {
        match self {
            Self::Approve => APPROVAL_APPROVE_ACTION,
            Self::Reject => APPROVAL_REJECT_ACTION,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CardCallbackEvent {
    event_id: Option<String>,
    action: ApprovalAction,
    approval_id: String,
    sender_id: String,
    chat_id: String,
    session_webhook: Option<String>,
}

#[derive(Debug, Clone)]
struct EventDeduper {
    ttl: Duration,
    max_entries: usize,
    seen_at: HashMap<String, Instant>,
    order: VecDeque<(Instant, String)>,
}

impl EventDeduper {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries: max_entries.max(1),
            seen_at: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert_if_new(&mut self, event_id: &str) -> bool {
        self.prune();
        if self.seen_at.contains_key(event_id) {
            return false;
        }
        let now = Instant::now();
        let event_id = event_id.to_string();
        self.seen_at.insert(event_id.clone(), now);
        self.order.push_back((now, event_id));
        self.prune();
        true
    }

    fn prune(&mut self) {
        let now = Instant::now();
        while let Some((seen_at, event_id)) = self.order.front().cloned() {
            let expired = now.duration_since(seen_at) >= self.ttl;
            let overflowed = self.seen_at.len() > self.max_entries;
            if !expired && !overflowed {
                break;
            }
            self.order.pop_front();
            if self
                .seen_at
                .get(event_id.as_str())
                .is_some_and(|recorded_at| *recorded_at == seen_at)
            {
                self.seen_at.remove(event_id.as_str());
            }
        }
    }
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

fn dingtalk_command_action_url(action: &str, approval_id: &str) -> String {
    let command = format!("/{action} {approval_id}");
    format!(
        "dtmd://dingtalkclient/sendMessage?content={}",
        urlencoding::encode(&command)
    )
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
    let robot_code = value
        .get("robotCode")
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
    let audio_recognition = extract_audio_recognition_text(value);
    let text = extract_dingtalk_message_text(value, &msg_type, audio_recognition.as_deref())?;
    let media_references = extract_dingtalk_media_references(value, &msg_type, &event_id);

    Some(InboundEvent {
        event_id,
        chat_id,
        robot_code,
        msg_type,
        sender_id,
        session_webhook,
        text,
        audio_recognition,
        media_references,
    })
}

fn parse_card_callback_event(value: &Value) -> Option<CardCallbackEvent> {
    let action_raw = extract_callback_action_value(value)?;
    let (action, approval_id) = parse_approval_action_token(&action_raw)?;
    let sender_id = callback_sender_id(value);
    let chat_id = callback_chat_id(value, &sender_id);
    let session_webhook = callback_session_webhook(value);
    let event_id = callback_event_id(value);
    Some(CardCallbackEvent {
        event_id,
        action,
        approval_id,
        sender_id,
        chat_id,
        session_webhook,
    })
}

fn extract_callback_action_value(value: &Value) -> Option<String> {
    [
        "/value",
        "/actionValue",
        "/action/value",
        "/callbackData/value",
        "/callbackData/action",
        "/cardPrivateData/value",
        "/cardPrivateData/action",
        "/content/value",
        "/content/action",
    ]
    .iter()
    .find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn parse_approval_action_token(value: &str) -> Option<(ApprovalAction, String)> {
    let lowered = value.trim().to_ascii_lowercase();
    for (prefix, action) in [
        ("approve:", ApprovalAction::Approve),
        ("approve_", ApprovalAction::Approve),
        ("approve-", ApprovalAction::Approve),
        ("reject:", ApprovalAction::Reject),
        ("reject_", ApprovalAction::Reject),
        ("reject-", ApprovalAction::Reject),
    ] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            let approval_id = rest.trim().to_string();
            if !approval_id.is_empty() {
                return Some((action, approval_id));
            }
        }
    }
    None
}

fn callback_sender_id(value: &Value) -> String {
    [
        "/senderStaffId",
        "/staffId",
        "/senderId",
        "/userId",
        "/operatorStaffId",
        "/eventOperatorStaffId",
    ]
    .iter()
    .find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
    .unwrap_or_else(|| "unknown-user".to_string())
}

fn callback_chat_id(value: &Value, sender_id: &str) -> String {
    value
        .pointer("/conversationId")
        .or_else(|| value.pointer("/chatbotConversationId"))
        .or_else(|| value.pointer("/chatId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| sender_id.to_string())
}

fn callback_session_webhook(value: &Value) -> Option<String> {
    value
        .pointer("/sessionWebhook")
        .or_else(|| value.pointer("/conversation/sessionWebhook"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn callback_event_id(value: &Value) -> Option<String> {
    [
        "/eventId",
        "/event_id",
        "/msgId",
        "/messageId",
        "/callbackId",
        "/processQueryKey",
    ]
    .iter()
    .find_map(|pointer| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn extract_dingtalk_message_text(
    value: &Value,
    msg_type: &str,
    audio_recognition: Option<&str>,
) -> Option<String> {
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
            if let Some(recognition) = audio_recognition
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return Some(recognition.to_string());
            }
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
                .or_else(|| value.pointer("/photo/fileName"))
                .or_else(|| value.pointer("/content/fileName"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|name| format!("（{name}）"))
                .unwrap_or_default();
            format!("[DingTalk图片消息{title}]\n用户发送了一张图片。")
        }
        "video" => {
            let title = first_string_value(
                value,
                &["/video/fileName", "/video/videoName", "/content/fileName"],
            )
            .map(|name| format!("（{name}）"))
            .unwrap_or_default();
            format!("[DingTalk视频消息{title}]\n用户发送了一个视频。")
        }
        "file" | "document" | "doc" | "attachment" => {
            let title = first_string_value(
                value,
                &[
                    "/file/fileName",
                    "/document/fileName",
                    "/doc/fileName",
                    "/attachment/fileName",
                    "/content/fileName",
                ],
            )
            .map(|name| format!("（{name}）"))
            .unwrap_or_default();
            format!("[DingTalk文件消息{title}]\n用户发送了一个文件。")
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
                    if !is_supported_richtext_media_type(&block_type) {
                        return None;
                    }

                    let filename = block
                        .get("fileName")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    let mut item_metadata = metadata.clone();
                    item_metadata.insert("dingtalk.richtext_index".to_string(), Value::from(index));
                    item_metadata.insert(
                        "dingtalk.richtext_block_type".to_string(),
                        Value::String(block_type),
                    );
                    let mime_type =
                        first_object_string_value(block, &["mimeType", "mime_type", "contentType"]);
                    let file_extension =
                        first_object_string_value(block, &["fileType", "fileExt", "extension"]);
                    attach_declared_media_metadata(
                        &mut item_metadata,
                        mime_type.as_deref(),
                        file_extension.as_deref(),
                        "dingtalk.declared_mime_type",
                        "dingtalk.file_extension",
                    );
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
                    Some(build_media_reference(
                        MediaSourceKind::ChannelInbound,
                        event_id,
                        filename,
                        mime_type,
                        item_metadata,
                    ))
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
            if let Some(download_code) = value
                .pointer("/audio/downloadCode")
                .or_else(|| value.pointer("/voice/downloadCode"))
                .or_else(|| value.pointer("/content/downloadCode"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|code| !code.is_empty())
            {
                metadata.insert(
                    "dingtalk.download_code".to_string(),
                    Value::String(download_code.to_string()),
                );
            }
            if let Some(picture_download_code) = value
                .pointer("/audio/pictureDownloadCode")
                .or_else(|| value.pointer("/voice/pictureDownloadCode"))
                .or_else(|| value.pointer("/content/pictureDownloadCode"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|code| !code.is_empty())
            {
                metadata.insert(
                    "dingtalk.picture_download_code".to_string(),
                    Value::String(picture_download_code.to_string()),
                );
            }
            let filename = value
                .pointer("/audio/fileName")
                .or_else(|| value.pointer("/voice/fileName"))
                .or_else(|| value.pointer("/content/fileName"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let mime_type = first_string_value(
                value,
                &[
                    "/audio/mimeType",
                    "/audio/contentType",
                    "/voice/mimeType",
                    "/voice/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            );
            let file_extension = first_string_value(
                value,
                &[
                    "/audio/fileType",
                    "/audio/fileExt",
                    "/audio/extension",
                    "/voice/fileType",
                    "/voice/fileExt",
                    "/voice/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            );
            attach_declared_media_metadata(
                &mut metadata,
                mime_type.as_deref(),
                file_extension.as_deref(),
                "dingtalk.declared_mime_type",
                "dingtalk.file_extension",
            );
            vec![build_media_reference(
                MediaSourceKind::ChannelInbound,
                event_id,
                filename,
                mime_type,
                metadata,
            )]
        }
        "video" => build_media_references(
            &metadata,
            event_id,
            first_string_value(
                value,
                &["/video/fileName", "/video/videoName", "/content/fileName"],
            ),
            first_string_value(
                value,
                &[
                    "/video/downloadCode",
                    "/video/videoDownloadCode",
                    "/content/downloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/video/pictureDownloadCode",
                    "/video/coverDownloadCode",
                    "/content/pictureDownloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/video/mimeType",
                    "/video/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/video/fileType",
                    "/video/fileExt",
                    "/video/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            ),
        ),
        "picture" | "image" | "photo" => {
            let filename = value
                .pointer("/picture/fileName")
                .or_else(|| value.pointer("/image/fileName"))
                .or_else(|| value.pointer("/photo/fileName"))
                .or_else(|| value.pointer("/content/fileName"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let mime_type = first_string_value(
                value,
                &[
                    "/picture/mimeType",
                    "/picture/contentType",
                    "/image/mimeType",
                    "/image/contentType",
                    "/photo/mimeType",
                    "/photo/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            );
            let file_extension = first_string_value(
                value,
                &[
                    "/picture/fileType",
                    "/picture/fileExt",
                    "/picture/extension",
                    "/image/fileType",
                    "/image/fileExt",
                    "/image/extension",
                    "/photo/fileType",
                    "/photo/fileExt",
                    "/photo/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            );
            if let Some(download_code) = value
                .pointer("/picture/downloadCode")
                .or_else(|| value.pointer("/image/downloadCode"))
                .or_else(|| value.pointer("/content/downloadCode"))
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
                .or_else(|| value.pointer("/content/pictureDownloadCode"))
                .and_then(Value::as_str)
                .filter(|code| !code.trim().is_empty())
            {
                metadata.insert(
                    "dingtalk.picture_download_code".to_string(),
                    Value::String(picture_download_code.to_string()),
                );
            }
            attach_declared_media_metadata(
                &mut metadata,
                mime_type.as_deref(),
                file_extension.as_deref(),
                "dingtalk.declared_mime_type",
                "dingtalk.file_extension",
            );
            vec![build_media_reference(
                MediaSourceKind::ChannelInbound,
                event_id,
                filename,
                mime_type,
                metadata,
            )]
        }
        "file" | "document" | "doc" | "attachment" => build_media_references(
            &metadata,
            event_id,
            first_string_value(
                value,
                &[
                    "/file/fileName",
                    "/document/fileName",
                    "/doc/fileName",
                    "/attachment/fileName",
                    "/content/fileName",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/downloadCode",
                    "/document/downloadCode",
                    "/doc/downloadCode",
                    "/attachment/downloadCode",
                    "/content/downloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/pictureDownloadCode",
                    "/document/pictureDownloadCode",
                    "/doc/pictureDownloadCode",
                    "/attachment/pictureDownloadCode",
                    "/content/pictureDownloadCode",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/mimeType",
                    "/file/contentType",
                    "/document/mimeType",
                    "/document/contentType",
                    "/doc/mimeType",
                    "/doc/contentType",
                    "/attachment/mimeType",
                    "/attachment/contentType",
                    "/content/mimeType",
                    "/content/contentType",
                ],
            ),
            first_string_value(
                value,
                &[
                    "/file/fileType",
                    "/file/fileExt",
                    "/file/extension",
                    "/document/fileType",
                    "/document/fileExt",
                    "/document/extension",
                    "/doc/fileType",
                    "/doc/fileExt",
                    "/doc/extension",
                    "/attachment/fileType",
                    "/attachment/fileExt",
                    "/attachment/extension",
                    "/content/fileType",
                    "/content/fileExt",
                    "/content/extension",
                ],
            ),
        ),
        _ => Vec::new(),
    }
}

fn is_supported_richtext_media_type(block_type: &str) -> bool {
    matches!(
        block_type,
        "picture" | "image" | "video" | "file" | "document" | "doc" | "attachment"
    )
}

fn build_media_references(
    metadata: &BTreeMap<String, Value>,
    event_id: &str,
    filename: Option<String>,
    download_code: Option<String>,
    picture_download_code: Option<String>,
    mime_type: Option<String>,
    file_extension: Option<String>,
) -> Vec<MediaReference> {
    let mut metadata = metadata.clone();
    if let Some(download_code) = download_code {
        metadata.insert(
            "dingtalk.download_code".to_string(),
            Value::String(download_code),
        );
    }
    if let Some(picture_download_code) = picture_download_code {
        metadata.insert(
            "dingtalk.picture_download_code".to_string(),
            Value::String(picture_download_code),
        );
    }
    attach_declared_media_metadata(
        &mut metadata,
        mime_type.as_deref(),
        file_extension.as_deref(),
        "dingtalk.declared_mime_type",
        "dingtalk.file_extension",
    );
    vec![build_media_reference(
        MediaSourceKind::ChannelInbound,
        event_id,
        filename,
        mime_type,
        metadata,
    )]
}

fn extract_audio_recognition_text(value: &Value) -> Option<String> {
    value
        .pointer("/content/recognition")
        .or_else(|| value.pointer("/audio/recognition"))
        .or_else(|| value.pointer("/voice/recognition"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn resolve_download_code_candidates(
    metadata: &BTreeMap<String, Value>,
) -> Vec<(String, &'static str)> {
    resolve_metadata_value_candidates(
        metadata,
        &[
            ("dingtalk.download_code", "download_code"),
            ("dingtalk.picture_download_code", "picture_download_code"),
        ],
    )
}

fn is_sender_allowed(allowlist: &[String], sender_id: &str) -> bool {
    if allowlist.is_empty() {
        return true;
    }

    allowlist
        .iter()
        .any(|entry| entry == "*" || entry == sender_id)
}

fn extract_approval_id_for_action_card(output: &ChannelResponse) -> Option<String> {
    if let Some(approval_id) = output
        .metadata
        .get("approval.id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(approval_id.to_string());
    }
    if let Some(approval_id) = output
        .metadata
        .get("approval.signal")
        .and_then(Value::as_object)
        .and_then(|value| value.get("approval_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(approval_id.to_string());
    }
    extract_shell_approval_id(&output.content)
}

fn extract_approval_command_preview(output: &ChannelResponse) -> Option<String> {
    output
        .metadata
        .get("approval.signal")
        .and_then(Value::as_object)
        .and_then(|value| value.get("command_preview"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn build_approval_action_card_body(output: &ChannelResponse, approval_id: &str) -> String {
    let mut sections = vec!["### 需要审批".to_string()];
    if let Some(command_preview) = extract_approval_command_preview(output) {
        sections.push(format!(
            "**待执行命令**\n\n`{}`",
            escape_markdown_for_action_card(&command_preview)
        ));
    }
    let escaped_content = escape_markdown_for_action_card(&output.content);
    if !escaped_content.trim().is_empty() {
        sections.push(escaped_content);
    }
    sections.push(format!("审批单: `{approval_id}`"));
    sections.push("点击按钮后将发送审批指令。".to_string());
    sections.join("\n\n---\n\n")
}

fn extract_shell_approval_id(content: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        if let Some(token) = value
            .pointer("/approval/id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
        if let Some(token) = value
            .get("approval_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
        if let Some(token) = value
            .get("approvalId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
        if let Some(token) = value
            .pointer("/approvalId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            return Some(token.to_string());
        }
    }
    let marker = "approval_id=";
    if let Some(idx) = content.find(marker) {
        let rest = &content[idx + marker.len()..];
        let token = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            .collect::<String>();
        if !token.is_empty() {
            return Some(token);
        }
    }

    extract_uuid_like_approval_id(content)
}

fn extract_uuid_like_approval_id(content: &str) -> Option<String> {
    let lowered = content.to_ascii_lowercase();
    let hinted = lowered.contains("approval id")
        || lowered.contains("approval_id")
        || content.contains("审批 ID")
        || content.contains("审批id")
        || content.contains("审批单")
        || lowered.contains("批准id")
        || content.contains("批准 ID");
    if !hinted {
        return None;
    }

    content
        .split(|ch: char| ch.is_whitespace() || ",.;:，。；：()[]{}<>\"'`".contains(ch))
        .filter_map(normalize_uuid_token)
        .find(|token| is_uuid_like(token))
}

fn normalize_uuid_token(token: &str) -> Option<String> {
    let trimmed = token.trim_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | '.'
                    | ';'
                    | ':'
                    | '，'
                    | '。'
                    | '；'
                    | '：'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '"'
                    | '\''
                    | '`'
            )
    });
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed
        .chars()
        .map(|ch| match ch {
            '–' | '—' | '−' => '-',
            _ => ch,
        })
        .collect::<String>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn is_uuid_like(token: &str) -> bool {
    let segments = token.split('-').collect::<Vec<_>>();
    if segments.len() != 5 {
        return false;
    }
    let expected = [8, 4, 4, 4, 12];
    segments.iter().zip(expected).all(|(segment, len)| {
        segment.len() == len && segment.chars().all(|ch| ch.is_ascii_hexdigit())
    })
}

fn escape_markdown_for_action_card(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::{
        ApprovalAction, CardCallbackEvent, DingtalkApiClient, EventDeduper, InboundEvent,
        build_approval_action_card_body, extract_approval_command_preview,
        extract_approval_id_for_action_card, extract_dingtalk_media_references,
        extract_dingtalk_message_text, extract_shell_approval_id, is_sender_allowed,
        parse_card_callback_event, parse_inbound_event, parse_stream_data, resolve_chat_id,
        resolve_download_code_candidates,
    };
    use crate::{
        ChannelResponse,
        render::{OutputRenderStyle, render_agent_output},
    };
    use std::collections::BTreeMap;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn parse_inbound_text_event_reads_dingtalk_shape() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_1",
            "sessionWebhook": "https://example/session",
            "msgId": "mid_1",
            "robotCode": "robot_1",
            "senderStaffId": "staff_1",
            "text": { "content": "hello" }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse");
        assert_eq!(
            parsed,
            InboundEvent {
                event_id: "mid_1".to_string(),
                chat_id: "cid_1".to_string(),
                robot_code: "robot_1".to_string(),
                msg_type: "text".to_string(),
                sender_id: "staff_1".to_string(),
                session_webhook: "https://example/session".to_string(),
                text: "hello".to_string(),
                audio_recognition: None,
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
            "robotCode": "robot_2",
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
    fn parse_inbound_picture_event_reads_content_download_codes() {
        let payload = serde_json::json!({
            "conversationType": "1",
            "sessionWebhook": "https://example/session4",
            "msgId": "mid_4",
            "robotCode": "robot_4",
            "senderStaffId": "staff_4",
            "msgtype": "picture",
            "content": {
                "downloadCode": "d-code-1",
                "pictureDownloadCode": "p-code-1"
            }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse picture content");
        assert_eq!(parsed.media_references.len(), 1);
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.download_code")
                .and_then(serde_json::Value::as_str),
            Some("d-code-1")
        );
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.picture_download_code")
                .and_then(serde_json::Value::as_str),
            Some("p-code-1")
        );
    }

    #[test]
    fn parse_inbound_richtext_event_extracts_text_and_pictures() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_3",
            "sessionWebhook": "https://example/session3",
            "msgId": "mid_3",
            "robotCode": "robot_3",
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
    fn parse_inbound_video_event_extracts_media_reference() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_video",
            "sessionWebhook": "https://example/session-video",
            "msgId": "mid_video",
            "robotCode": "robot_video",
            "senderStaffId": "staff_video",
            "msgtype": "video",
            "video": {
                "fileName": "demo.mp4",
                "downloadCode": "video-code-1",
                "mimeType": "video/mp4",
                "fileType": "mp4"
            }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse video");
        assert!(parsed.text.contains("视频消息"));
        assert_eq!(parsed.media_references.len(), 1);
        assert_eq!(
            parsed.media_references[0].filename.as_deref(),
            Some("demo.mp4")
        );
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.download_code")
                .and_then(serde_json::Value::as_str),
            Some("video-code-1")
        );
        assert_eq!(
            parsed.media_references[0].mime_type.as_deref(),
            Some("video/mp4")
        );
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.file_extension")
                .and_then(serde_json::Value::as_str),
            Some("mp4")
        );
    }

    #[test]
    fn parse_inbound_file_event_extracts_media_reference() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_file",
            "sessionWebhook": "https://example/session-file",
            "msgId": "mid_file",
            "robotCode": "robot_file",
            "senderStaffId": "staff_file",
            "msgtype": "file",
            "file": {
                "fileName": "report.xlsx",
                "downloadCode": "file-code-1",
                "contentType": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "extension": "xlsx"
            }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse file");
        assert!(parsed.text.contains("文件消息"));
        assert_eq!(parsed.media_references.len(), 1);
        assert_eq!(
            parsed.media_references[0].filename.as_deref(),
            Some("report.xlsx")
        );
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.download_code")
                .and_then(serde_json::Value::as_str),
            Some("file-code-1")
        );
        assert_eq!(
            parsed.media_references[0].mime_type.as_deref(),
            Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
        );
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.file_extension")
                .and_then(serde_json::Value::as_str),
            Some("xlsx")
        );
    }

    #[test]
    fn parse_inbound_richtext_event_extracts_non_image_attachments() {
        let payload = serde_json::json!({
            "conversationType": 2,
            "conversationId": "cid_rich_file",
            "sessionWebhook": "https://example/session-rich-file",
            "msgId": "mid_rich_file",
            "robotCode": "robot_rich_file",
            "senderStaffId": "staff_rich_file",
            "msgtype": "richText",
            "content": {
                "richText": [
                    {
                        "type": "file",
                        "fileName": "slides.pdf",
                        "downloadCode": "file-rich-code",
                        "mimeType": "application/pdf",
                        "fileType": "pdf"
                    },
                    {
                        "type": "video",
                        "fileName": "walkthrough.mp4",
                        "downloadCode": "video-rich-code",
                        "mimeType": "video/mp4"
                    }
                ]
            }
        });

        let parsed = parse_inbound_event(&payload).expect("should parse richText file/video");
        assert_eq!(parsed.media_references.len(), 2);
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.richtext_block_type")
                .and_then(serde_json::Value::as_str),
            Some("file")
        );
        assert_eq!(
            parsed.media_references[1]
                .metadata
                .get("dingtalk.richtext_block_type")
                .and_then(serde_json::Value::as_str),
            Some("video")
        );
        assert_eq!(
            parsed.media_references[0].mime_type.as_deref(),
            Some("application/pdf")
        );
        assert_eq!(
            parsed.media_references[0]
                .metadata
                .get("dingtalk.file_extension")
                .and_then(serde_json::Value::as_str),
            Some("pdf")
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
    fn event_deduper_rejects_duplicates_within_ttl() {
        let mut deduper = EventDeduper::new(Duration::from_millis(30), 10);
        assert!(deduper.insert_if_new("evt-1"));
        assert!(!deduper.insert_if_new("evt-1"));
    }

    #[test]
    fn event_deduper_expires_entries_after_ttl() {
        let mut deduper = EventDeduper::new(Duration::from_millis(5), 10);
        assert!(deduper.insert_if_new("evt-1"));
        thread::sleep(Duration::from_millis(8));
        assert!(deduper.insert_if_new("evt-1"));
    }

    #[test]
    fn event_deduper_evicts_oldest_when_capacity_reached() {
        let mut deduper = EventDeduper::new(Duration::from_secs(5), 2);
        assert!(deduper.insert_if_new("evt-1"));
        assert!(deduper.insert_if_new("evt-2"));
        assert!(deduper.insert_if_new("evt-3"));
        assert!(deduper.insert_if_new("evt-1"));
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
                metadata: BTreeMap::new(),
                attachments: Vec::new(),
            },
            true,
            OutputRenderStyle::Markdown,
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
    fn session_webhook_success_accepts_errcode_zero_json() {
        DingtalkApiClient::ensure_session_webhook_success(
            r#"{"errcode":0,"errmsg":"ok"}"#,
            "markdown send",
        )
        .expect("errcode 0 should succeed");
    }

    #[test]
    fn session_webhook_success_rejects_non_zero_errcode_json() {
        let err = DingtalkApiClient::ensure_session_webhook_success(
            r#"{"errcode":310000,"errmsg":"invalid session"}"#,
            "markdown send",
        )
        .expect_err("non-zero errcode should fail");
        assert!(err.to_string().contains("errcode=310000"));
        assert!(err.to_string().contains("invalid session"));
    }

    #[test]
    fn non_text_messages_fall_back_to_summary() {
        let payload = serde_json::json!({
            "audio": { "duration": 8 }
        });
        let text = extract_dingtalk_message_text(&payload, "audio", None).expect("audio fallback");
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

    #[test]
    fn resolve_download_code_prefers_download_code() {
        let metadata = BTreeMap::from([
            (
                "dingtalk.download_code".to_string(),
                serde_json::json!("download-code"),
            ),
            (
                "dingtalk.picture_download_code".to_string(),
                serde_json::json!("picture-code"),
            ),
        ]);
        let resolved = resolve_download_code_candidates(&metadata)
            .into_iter()
            .next()
            .expect("should resolve");
        assert_eq!(resolved.0, "download-code");
        assert_eq!(resolved.1, "download_code");
    }

    #[test]
    fn audio_message_prefers_recognition_text() {
        let payload = serde_json::json!({
            "content": { "recognition": "这是一段语音转文字" },
            "audio": { "duration": 5 }
        });
        let text = extract_dingtalk_message_text(&payload, "audio", Some("这是一段语音转文字"))
            .expect("audio recognition");
        assert_eq!(text, "这是一段语音转文字");
    }

    #[test]
    fn extract_shell_approval_id_from_text() {
        let content =
            "approval required: approval_id=123e4567-e89b-12d3-a456-426614174000; retry later";
        let approval_id = extract_shell_approval_id(content).expect("approval id");
        assert_eq!(approval_id, "123e4567-e89b-12d3-a456-426614174000");
    }

    #[test]
    fn extract_shell_approval_id_from_json_payload() {
        let content = serde_json::json!({
            "action": "request",
            "approval": { "id": "approval-json-1" }
        })
        .to_string();
        let approval_id = extract_shell_approval_id(&content).expect("approval id");
        assert_eq!(approval_id, "approval-json-1");
    }

    #[test]
    fn extract_shell_approval_id_from_json_approval_id_camel_case() {
        let content = serde_json::json!({
            "action": "request",
            "approvalId": "approval-json-2"
        })
        .to_string();
        let approval_id = extract_shell_approval_id(&content).expect("approval id");
        assert_eq!(approval_id, "approval-json-2");
    }

    #[test]
    fn extract_shell_approval_id_from_natural_language() {
        let content =
            "我已经请求了批准。审批 ID 是 e4d1e3bf-2d00-49da-b091-23e818a83483。请您批准该操作。";
        let approval_id = extract_shell_approval_id(content).expect("approval id");
        assert_eq!(approval_id, "e4d1e3bf-2d00-49da-b091-23e818a83483");
    }

    #[test]
    fn extract_shell_approval_id_from_natural_language_approve_wording() {
        let content =
            "我已经请求批准来执行浏览器自动化任务。批准ID: 3a24e1d4-9c94-4ee1-ac16-1f750ca78acf";
        let approval_id = extract_shell_approval_id(content).expect("approval id");
        assert_eq!(approval_id, "3a24e1d4-9c94-4ee1-ac16-1f750ca78acf");
    }

    #[test]
    fn extract_approval_id_for_action_card_prefers_structured_metadata() {
        let output = ChannelResponse {
            content: "approval required: approval_id=from-content".to_string(),
            reasoning: None,
            metadata: BTreeMap::from([(
                "approval.id".to_string(),
                serde_json::json!("from-metadata"),
            )]),
            attachments: Vec::new(),
        };
        let approval_id = extract_approval_id_for_action_card(&output).expect("approval id");
        assert_eq!(approval_id, "from-metadata");
    }

    #[test]
    fn extract_approval_id_for_action_card_falls_back_to_content() {
        let output = ChannelResponse {
            content: "approval required: approval_id=from-content".to_string(),
            reasoning: None,
            metadata: BTreeMap::new(),
            attachments: Vec::new(),
        };
        let approval_id = extract_approval_id_for_action_card(&output).expect("approval id");
        assert_eq!(approval_id, "from-content");
    }

    #[test]
    fn extract_approval_command_preview_reads_structured_metadata() {
        let output = ChannelResponse {
            content: "approval required".to_string(),
            reasoning: None,
            metadata: BTreeMap::from([(
                "approval.signal".to_string(),
                serde_json::json!({
                    "approval_id": "approval-1",
                    "command_preview": "pip3 install pymupdf -q"
                }),
            )]),
            attachments: Vec::new(),
        };
        let preview = extract_approval_command_preview(&output).expect("command preview");
        assert_eq!(preview, "pip3 install pymupdf -q");
    }

    #[test]
    fn build_approval_action_card_body_includes_command_preview() {
        let output = ChannelResponse {
            content: "This shell command requires approval.".to_string(),
            reasoning: None,
            metadata: BTreeMap::from([(
                "approval.signal".to_string(),
                serde_json::json!({
                    "approval_id": "approval-1",
                    "command_preview": "python3 -c \"print(1)\""
                }),
            )]),
            attachments: Vec::new(),
        };
        let body = build_approval_action_card_body(&output, "approval-1");
        assert!(body.contains("待执行命令"));
        assert!(body.contains("python3 -c \"print(1)\""));
        assert!(body.contains("审批单: `approval-1`"));
    }

    #[test]
    fn parse_card_callback_event_reads_approve_token() {
        let payload = serde_json::json!({
            "eventId": "evt-1",
            "conversationId": "cid-1",
            "sessionWebhook": "https://example/session",
            "senderStaffId": "staff-1",
            "value": "approve_approval-123"
        });
        let parsed = parse_card_callback_event(&payload).expect("callback");
        assert_eq!(
            parsed,
            CardCallbackEvent {
                event_id: Some("evt-1".to_string()),
                action: ApprovalAction::Approve,
                approval_id: "approval-123".to_string(),
                sender_id: "staff-1".to_string(),
                chat_id: "cid-1".to_string(),
                session_webhook: Some("https://example/session".to_string()),
            }
        );
    }

    #[test]
    fn parse_card_callback_event_reads_reject_token_from_nested_payload() {
        let payload = serde_json::json!({
            "userId": "staff-2",
            "callbackData": {
                "action": "reject:approval-99"
            }
        });
        let parsed = parse_card_callback_event(&payload).expect("callback");
        assert_eq!(parsed.action, ApprovalAction::Reject);
        assert_eq!(parsed.approval_id, "approval-99");
        assert_eq!(parsed.chat_id, "staff-2");
        assert_eq!(parsed.sender_id, "staff-2");
        assert!(parsed.session_webhook.is_none());
    }
}
