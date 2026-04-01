mod attachments;
mod client;
mod config;
mod parsing;

#[cfg(test)]
mod tests;

use self::attachments::deliver_dingtalk_attachments;
use self::client::DingtalkApiClient;
use self::config::resolve_local_attachment_policy;
pub use self::config::{DingtalkChannelConfig, DingtalkProxyConfig};
use self::parsing::{
    EventDeduper, InboundEvent, StreamEnvelope, build_approval_action_card_body,
    extract_approval_id_for_action_card, is_sender_allowed, parse_card_callback_event,
    parse_inbound_event, parse_stream_data, resolve_download_code_candidates,
};
use crate::{
    Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, LocalAttachmentPolicy,
    manager::{
        ChannelInstanceConfig, ChannelKind, ChannelSupervisorReporter, ManagedChannelDriver,
    },
    media::{
        ArchiveMediaIngestContext, DEFAULT_INLINE_MEDIA_MAX_BYTES, ingest_media_reference_bytes,
    },
    render::{OutputRenderStyle, render_agent_output},
};
use futures_util::{SinkExt, StreamExt};
use klaw_archive::open_default_archive_service;
use klaw_config::{DingtalkConfig, LocalAttachmentConfig};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio::time::{self, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{debug, info, trace, warn};

const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const WS_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
const WS_WATCHDOG_INTERVAL: Duration = Duration::from_secs(5);
const WS_STALL_TIMEOUT: Duration = Duration::from_secs(35);
const EVENT_DEDUP_TTL: Duration = Duration::from_secs(60 * 60);
const EVENT_DEDUP_MAX_ENTRIES: usize = 20_000;

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
        reporter: ChannelSupervisorReporter,
    ) -> ChannelResult<()> {
        self.validate_config()?;
        DingtalkApiClient::ensure_rustls_crypto_provider();
        info!("dingtalk channel started");
        reporter.mark_running("dingtalk channel initialized");
        let mut reconnect_attempt = 0_u32;

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
                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                    let message = format!("failed to open dingtalk stream connection: {err}");
                    reporter.mark_reconnecting(reconnect_attempt, message.clone());
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
                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                    let message = format!(
                        "dingtalk stream connection timed out after {}s",
                        CONNECT_TIMEOUT.as_secs()
                    );
                    reporter.mark_reconnecting(reconnect_attempt, message);
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
                    reconnect_attempt = 0;
                    reporter.mark_running("dingtalk websocket connected");
                    info!(
                        ws_endpoint = ws_url.as_str(),
                        handshake_status = response.status().as_u16(),
                        "dingtalk stream connection established"
                    );
                    let mut cron_tick = time::interval(runtime.cron_tick_interval());
                    let mut runtime_tick = time::interval(runtime.runtime_tick_interval());
                    let mut keepalive_tick = time::interval(WS_KEEPALIVE_INTERVAL);
                    let mut watchdog_tick = time::interval(WS_WATCHDOG_INTERVAL);
                    let mut cron_job: Option<Pin<Box<dyn Future<Output = ()> + '_>>> = None;
                    let mut runtime_job: Option<Pin<Box<dyn Future<Output = ()> + '_>>> = None;
                    let mut last_activity_at = Instant::now();

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
                                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                                    reporter.mark_reconnecting(
                                        reconnect_attempt,
                                        format!("dingtalk websocket keepalive ping failed: {err}"),
                                    );
                                    warn!(error = %err, "failed to send dingtalk websocket keepalive ping");
                                    break;
                                }
                            }
                            _ = watchdog_tick.tick() => {
                                let idle_for = last_activity_at.elapsed();
                                if idle_for >= WS_STALL_TIMEOUT {
                                    let message = format!(
                                        "dingtalk websocket stalled after {}s without activity",
                                        idle_for.as_secs()
                                    );
                                    reporter.mark_degraded(message.clone());
                                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                                    reporter.mark_reconnecting(reconnect_attempt, message.clone());
                                    warn!(idle_secs = idle_for.as_secs(), "{}", message);
                                    break;
                                }
                            }
                            message = ws.next() => {
                                let Some(message) = message else {
                                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                                    reporter.mark_reconnecting(
                                        reconnect_attempt,
                                        "dingtalk stream connection closed by remote".to_string(),
                                    );
                                    warn!("dingtalk stream connection closed by remote");
                                    break;
                                };

                                match message {
                                    Ok(Message::Text(text)) => {
                                        last_activity_at = Instant::now();
                                        reporter.record_activity("dingtalk inbound event received");
                                        if let Err(err) = self
                                            .handle_text_message(runtime, &mut ws, text.as_str())
                                            .await
                                        {
                                            warn!(error = %err, "failed to process dingtalk message");
                                        }
                                    }
                                    Ok(Message::Ping(payload)) => {
                                        last_activity_at = Instant::now();
                                        reporter.record_activity("dingtalk ping received");
                                        if let Err(err) = ws.send(Message::Pong(payload)).await {
                                            reconnect_attempt = reconnect_attempt.saturating_add(1);
                                            reporter.mark_reconnecting(
                                                reconnect_attempt,
                                                format!("failed to send websocket pong: {err}"),
                                            );
                                            warn!(error = %err, "failed to send websocket pong");
                                            break;
                                        }
                                    }
                                    Ok(Message::Pong(_)) => {
                                        last_activity_at = Instant::now();
                                        reporter.record_activity("dingtalk pong received");
                                        trace!("received dingtalk websocket pong");
                                    }
                                    Ok(Message::Close(frame)) => {
                                        reconnect_attempt = reconnect_attempt.saturating_add(1);
                                        reporter.mark_reconnecting(
                                            reconnect_attempt,
                                            format!("dingtalk stream connection closed: {frame:?}"),
                                        );
                                        info!(close_frame = ?frame, "dingtalk stream connection closed");
                                        break;
                                    }
                                    Ok(Message::Binary(_)) | Ok(Message::Frame(_)) => {}
                                    Err(err) => {
                                        reconnect_attempt = reconnect_attempt.saturating_add(1);
                                        reporter.mark_reconnecting(
                                            reconnect_attempt,
                                            format!("dingtalk stream receive failed: {err}"),
                                        );
                                        warn!(error = %err, "dingtalk stream receive failed");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                    reporter.mark_reconnecting(
                        reconnect_attempt,
                        format!("dingtalk stream connect failed: {err}"),
                    );
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
        reporter: ChannelSupervisorReporter,
    ) -> ChannelResult<()> {
        DingtalkChannel::run_until_shutdown(self, runtime, shutdown, reporter).await
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

#[async_trait::async_trait(?Send)]
impl Channel for DingtalkChannel {
    fn name(&self) -> &'static str {
        "dingtalk"
    }

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let config = ChannelInstanceConfig::Dingtalk(DingtalkConfig {
            id: self.config.account_id.clone(),
            enabled: true,
            client_id: self.config.client_id.clone(),
            client_secret: self.config.client_secret.clone(),
            bot_title: self.config.bot_title.clone(),
            show_reasoning: self.config.show_reasoning,
            stream_output: self.config.stream_output,
            allowlist: self.config.allowlist.clone(),
            proxy: klaw_config::DingtalkProxyConfig {
                enabled: self.config.proxy.enabled,
                url: self.config.proxy.url.clone(),
            },
        });
        let reporter = ChannelSupervisorReporter::new(
            config.key(),
            config,
            Arc::new(Mutex::new(BTreeMap::new())),
        );
        self.run_until_shutdown(runtime, &mut shutdown_rx, reporter)
            .await
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
                    .and_then(serde_json::Value::as_str)
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

        let Some(mut inbound) = parse_inbound_event(&payload, &self.config.bot_title) else {
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
                serde_json::Value::String(code_source.to_string()),
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

    fn local_attachment_policy(&self) -> ChannelResult<LocalAttachmentPolicy> {
        resolve_local_attachment_policy(&self.config.local_attachments)
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

    let mut headers = std::collections::HashMap::new();
    headers.insert("messageId", message_id);
    headers.insert("contentType", "application/json".to_string());

    let ack = self::parsing::StreamAck {
        code: 200,
        headers,
        message: "OK",
        data,
    };

    ws.send(Message::Text(serde_json::to_string(&ack)?.into()))
        .await?;
    Ok(())
}
