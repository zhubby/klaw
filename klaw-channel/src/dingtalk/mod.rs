mod attachments;
mod client;
mod config;
mod error;
mod parsing;

#[cfg(test)]
mod tests;

use self::attachments::deliver_dingtalk_attachments;
use self::client::{DingtalkApiClient, build_ai_card_card_data};
use self::config::resolve_local_attachment_policy;
pub use self::config::{DingtalkChannelConfig, DingtalkProxyConfig};
pub use self::error::{DingtalkApiError, is_session_webhook_session_not_found_error};
use self::parsing::{
    EventDeduper, InboundEvent, StreamEnvelope, build_im_card_action_buttons,
    build_im_card_action_card_body, is_sender_allowed, parse_card_callback_event,
    parse_inbound_event, parse_stream_data, resolve_channel_card, resolve_download_code_candidates,
};
use crate::{
    Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, ChannelStreamEvent,
    ChannelStreamWriter, LocalAttachmentPolicy,
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
use uuid::Uuid;

const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const WS_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
const WS_WATCHDOG_INTERVAL: Duration = Duration::from_secs(5);
const WS_STALL_TIMEOUT: Duration = Duration::from_secs(35);
const EVENT_DEDUP_TTL: Duration = Duration::from_secs(60 * 60);
const EVENT_DEDUP_MAX_ENTRIES: usize = 20_000;
const DINGTALK_STREAM_UPDATE_INTERVAL: Duration = Duration::from_millis(400);
const DINGTALK_STREAM_MAX_CONTENT_BYTES: usize = 1024;

/// Returns `true` when shutdown was requested during the delay.
async fn wait_reconnect_delay_or_shutdown(
    shutdown: &mut watch::Receiver<bool>,
    delay: Duration,
) -> bool {
    tokio::select! {
        _ = time::sleep(delay) => false,
        changed = shutdown.changed() => {
            if changed.is_ok() && *shutdown.borrow() {
                info!("dingtalk shutdown requested while reconnect waiting");
                true
            } else {
                false
            }
        }
    }
}

fn runtime_metadata(
    session_webhook: Option<&str>,
    bot_title: &str,
    isolated_turn: bool,
) -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    if isolated_turn {
        metadata.insert(
            "agent.isolated_turn".to_string(),
            serde_json::Value::Bool(true),
        );
    }
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

#[derive(Debug, Clone)]
struct DingtalkStreamWriter {
    client: DingtalkApiClient,
    client_id: String,
    client_secret: String,
    robot_code: String,
    chat_id: String,
    template_id: String,
    content_key: String,
    reasoning_key: String,
    show_reasoning: bool,
    out_track_id: String,
    last_content: Option<String>,
    last_reasoning: Option<String>,
    last_update_at: Option<Instant>,
    card_sent: bool,
    stream_failed: bool,
    saw_special_card: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DingtalkStreamSnapshotParts {
    content: String,
    reasoning: String,
}

impl DingtalkStreamWriter {
    fn new(
        client: DingtalkApiClient,
        client_id: String,
        client_secret: String,
        robot_code: String,
        chat_id: String,
        template_id: String,
        content_key: String,
        reasoning_key: String,
        show_reasoning: bool,
    ) -> Self {
        Self {
            client,
            client_id,
            client_secret,
            robot_code,
            chat_id,
            template_id,
            content_key,
            reasoning_key,
            show_reasoning,
            out_track_id: Uuid::new_v4().to_string(),
            last_content: None,
            last_reasoning: None,
            last_update_at: None,
            card_sent: false,
            stream_failed: false,
            saw_special_card: false,
        }
    }

    fn enabled(&self) -> bool {
        !self.template_id.trim().is_empty()
    }

    fn stream_reasoning_enabled(&self) -> bool {
        self.show_reasoning
            && !self.reasoning_key.trim().is_empty()
            && self.reasoning_key.trim() != self.content_key.trim()
    }

    fn stream_snapshot_parts(&self, output: &ChannelResponse) -> DingtalkStreamSnapshotParts {
        let content = render_agent_output(output, false, OutputRenderStyle::Markdown);
        let reasoning = self
            .show_reasoning
            .then(|| {
                output
                    .reasoning
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();
        DingtalkStreamSnapshotParts { content, reasoning }
    }

    fn should_stream_reasoning_update(
        &self,
        parts: &DingtalkStreamSnapshotParts,
        force: bool,
        finalize: bool,
    ) -> bool {
        if !self.stream_reasoning_enabled() {
            return false;
        }
        if parts.reasoning.is_empty() && self.last_reasoning.is_none() {
            return false;
        }
        self.last_reasoning.as_deref() != Some(parts.reasoning.as_str()) || force || finalize
    }

    fn should_stream_content_update(
        &self,
        parts: &DingtalkStreamSnapshotParts,
        force: bool,
        finalize: bool,
    ) -> bool {
        if parts.content.is_empty() && self.last_content.is_none() {
            return false;
        }
        self.last_content.as_deref() != Some(parts.content.as_str()) || force || finalize
    }

    fn stream_api_content(content: &str) -> String {
        if content.is_empty() {
            return " ".to_string();
        }
        let fence_count = content
            .lines()
            .filter(|line| line.trim_start().starts_with("```"))
            .count();
        if fence_count % 2 == 1 {
            format!("{content}\n```")
        } else {
            content.to_string()
        }
    }

    fn validate_stream_api_content(key: &str, content: &str) -> Result<(), String> {
        let content_bytes = content.as_bytes().len();
        if content_bytes > DINGTALK_STREAM_MAX_CONTENT_BYTES {
            return Err(format!(
                "streaming content for key '{}' exceeds dingtalk streaming limit: {} > {} bytes",
                key.trim(),
                content_bytes,
                DINGTALK_STREAM_MAX_CONTENT_BYTES
            ));
        }
        Ok(())
    }

    fn content_finalize_flag(will_stream_reasoning: bool, finalize: bool) -> bool {
        finalize && !will_stream_reasoning
    }

    fn build_initial_card_data(&self) -> serde_json::Value {
        // Preserve the pre-reasoning AI card creation shape. DingTalk accepts
        // subsequent streaming updates by key; adding extra initial stream keys
        // can make the primary content stream fail with an opaque 500.
        build_ai_card_card_data(&self.content_key, "")
    }

    async fn begin(&mut self) {
        if !self.enabled() || self.stream_failed || self.card_sent {
            return;
        }
        debug!(
            chat_id = self.chat_id.as_str(),
            out_track_id = self.out_track_id.as_str(),
            template_id = self.template_id.as_str(),
            content_key = self.content_key.as_str(),
            "starting dingtalk ai card stream"
        );
        let access_token = match self
            .client
            .fetch_access_token(&self.client_id, &self.client_secret)
            .await
        {
            Ok(token) => token,
            Err(err) => {
                self.stream_failed = true;
                warn!(
                    chat_id = self.chat_id.as_str(),
                    error = %err,
                    "failed to fetch dingtalk access token for ai card stream"
                );
                return;
            }
        };
        let card_data = self.build_initial_card_data();
        if let Err(err) = self
            .client
            .create_and_deliver_ai_card(
                &access_token,
                &self.template_id,
                &self.robot_code,
                &self.chat_id,
                &self.out_track_id,
                card_data,
            )
            .await
        {
            self.stream_failed = true;
            warn!(
                chat_id = self.chat_id.as_str(),
                error = %err,
                "failed to create dingtalk ai card stream; will fallback to markdown"
            );
            return;
        }
        self.card_sent = true;
        self.last_update_at = Some(Instant::now());
        debug!(
            chat_id = self.chat_id.as_str(),
            out_track_id = self.out_track_id.as_str(),
            "dingtalk ai card stream started"
        );
    }

    async fn finish(&mut self, output: &ChannelResponse) {
        if resolve_channel_card(output).is_some() {
            self.saw_special_card = true;
            return;
        }
        let parts = self.stream_snapshot_parts(output);
        let _ = self.flush_stream_parts(parts, true, true).await;
    }

    async fn flush_stream_parts(
        &mut self,
        parts: DingtalkStreamSnapshotParts,
        force: bool,
        finalize: bool,
    ) -> ChannelResult<()> {
        if !self.enabled() {
            return Ok(());
        }
        if self.stream_failed {
            return Ok(());
        }
        let stream_reasoning = self.stream_reasoning_enabled();
        let should_stream_content = self.should_stream_content_update(&parts, force, finalize);
        let should_stream_reasoning = self.should_stream_reasoning_update(&parts, force, finalize);
        if parts.content.trim().is_empty()
            && (!stream_reasoning || parts.reasoning.trim().is_empty())
            && self.last_content.is_none()
            && self.last_reasoning.is_none()
        {
            return Ok(());
        }
        if !should_stream_content && !should_stream_reasoning {
            return Ok(());
        }
        let should_flush = finalize
            || force
            || !self.card_sent
            || self
                .last_update_at
                .is_none_or(|instant| instant.elapsed() >= DINGTALK_STREAM_UPDATE_INTERVAL);
        if should_stream_content || !parts.content.is_empty() {
            self.last_content = Some(parts.content.clone());
        }
        if should_stream_reasoning || !parts.reasoning.is_empty() {
            self.last_reasoning = Some(parts.reasoning.clone());
        }
        if !should_flush {
            return Ok(());
        }

        if !self.card_sent {
            self.begin().await;
        }
        if self.stream_failed || !self.card_sent {
            return Ok(());
        }

        debug!(
            chat_id = self.chat_id.as_str(),
            out_track_id = self.out_track_id.as_str(),
            force,
            finalize,
            content_chars = parts.content.chars().count(),
            reasoning_chars = parts.reasoning.chars().count(),
            "updating dingtalk ai card stream snapshot"
        );
        let access_token = self
            .client
            .fetch_access_token(&self.client_id, &self.client_secret)
            .await?;
        if should_stream_content {
            let content = Self::stream_api_content(&parts.content);
            Self::validate_stream_api_content(&self.content_key, &content)?;
            let content_finalize = Self::content_finalize_flag(should_stream_reasoning, finalize);
            self.client
                .stream_ai_card(
                    &access_token,
                    &self.out_track_id,
                    &self.content_key,
                    &content,
                    content_finalize,
                    false,
                )
                .await
                .map_err(|err| {
                    format!(
                        "dingtalk ai card streaming update failed for key '{}' ({} bytes): {err}",
                        self.content_key.trim(),
                        content.as_bytes().len()
                    )
                })?;
        }
        if should_stream_reasoning {
            let reasoning = Self::stream_api_content(&parts.reasoning);
            Self::validate_stream_api_content(&self.reasoning_key, &reasoning)?;
            self.client
                .stream_ai_card(
                    &access_token,
                    &self.out_track_id,
                    &self.reasoning_key,
                    &reasoning,
                    finalize,
                    false,
                )
                .await
                .map_err(|err| {
                    format!(
                        "dingtalk ai card streaming update failed for key '{}' ({} bytes): {err}",
                        self.reasoning_key.trim(),
                        reasoning.as_bytes().len()
                    )
                })?;
        }
        self.last_update_at = Some(Instant::now());
        Ok(())
    }

    async fn write_snapshot(&mut self, output: ChannelResponse) -> ChannelResult<()> {
        if resolve_channel_card(&output).is_some() {
            self.saw_special_card = true;
            return Ok(());
        }

        let parts = self.stream_snapshot_parts(&output);
        match self.flush_stream_parts(parts, false, false).await {
            Ok(()) => Ok(()),
            Err(err) => {
                self.stream_failed = true;
                warn!(
                    chat_id = self.chat_id.as_str(),
                    error = %err,
                    "failed to stream dingtalk ai card; will fallback to markdown"
                );
                Ok(())
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ChannelStreamWriter for DingtalkStreamWriter {
    async fn write(&mut self, event: ChannelStreamEvent) -> ChannelResult<()> {
        match event {
            ChannelStreamEvent::Snapshot(output) => self.write_snapshot(output).await,
            ChannelStreamEvent::Clear => {
                self.last_content = None;
                self.last_reasoning = None;
                Ok(())
            }
        }
    }
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
            stream_template_id: config.stream_template_id,
            stream_content_key: config.stream_content_key,
            stream_reasoning_key: config.stream_reasoning_key,
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
        if self.config.stream_output && self.config.stream_template_id.trim().is_empty() {
            return Err("dingtalk stream_template_id is required when stream_output=true".into());
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
                    if wait_reconnect_delay_or_shutdown(shutdown, RECONNECT_DELAY).await {
                        return Ok(());
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
                    if wait_reconnect_delay_or_shutdown(shutdown, RECONNECT_DELAY).await {
                        return Ok(());
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

            if wait_reconnect_delay_or_shutdown(shutdown, RECONNECT_DELAY).await {
                return Ok(());
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

pub async fn send_proactive_markdown_via_proxy(
    proxy: &DingtalkProxyConfig,
    client_id: &str,
    client_secret: &str,
    chat_id: &str,
    title: &str,
    text: &str,
) -> ChannelResult<()> {
    let client = DingtalkApiClient::new(proxy)?;
    let access_token = client.fetch_access_token(client_id, client_secret).await?;
    client
        .send_proactive_markdown(&access_token, client_id, chat_id, title, text)
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
            stream_template_id: self.config.stream_template_id.clone(),
            stream_content_key: self.config.stream_content_key.clone(),
            stream_reasoning_key: self.config.stream_reasoning_key.clone(),
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
    async fn submit_request(
        &self,
        runtime: &dyn ChannelRuntime,
        request: ChannelRequest,
        inbound: &InboundEvent,
    ) -> ChannelResult<(Option<ChannelResponse>, Option<DingtalkStreamWriter>)> {
        if !self.config.stream_output || self.config.stream_template_id.trim().is_empty() {
            return Ok((runtime.submit(request).await?, None));
        }

        let mut writer = DingtalkStreamWriter::new(
            self.client.clone(),
            self.config.client_id.clone(),
            self.config.client_secret.clone(),
            inbound.robot_code.clone(),
            inbound.chat_id.clone(),
            self.config.stream_template_id.clone(),
            self.config.stream_content_key.clone(),
            self.config.stream_reasoning_key.clone(),
            self.config.show_reasoning,
        );
        debug!(
            chat_id = inbound.chat_id.as_str(),
            session_webhook = inbound.session_webhook.as_str(),
            template_id = self.config.stream_template_id.as_str(),
            content_key = self.config.stream_content_key.as_str(),
            reasoning_key = self.config.stream_reasoning_key.as_str(),
            "submitting dingtalk request with ai card streaming enabled"
        );
        writer.begin().await;
        let output = runtime.submit_streaming(request, &mut writer).await?;
        if let Some(ref output) = output {
            writer.finish(output).await;
        }
        Ok((output, Some(writer)))
    }

    async fn send_output(
        &self,
        inbound: &InboundEvent,
        output: &ChannelResponse,
        stream_writer: Option<&DingtalkStreamWriter>,
    ) {
        if let Some(card) = resolve_channel_card(output) {
            let body = build_im_card_action_card_body(&card);
            let buttons = build_im_card_action_buttons(&card);
            if let Err(err) = self
                .client
                .send_session_webhook_generic_action_card(
                    &inbound.session_webhook,
                    card.title_or("卡片消息"),
                    &body,
                    &buttons,
                )
                .await
            {
                warn!(
                    chat_id = inbound.chat_id.as_str(),
                    error = %err,
                    "failed to send dingtalk action card; fallback to markdown"
                );
                let markdown = render_agent_output(
                    output,
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
            return;
        }

        let body = render_agent_output(
            output,
            self.config.show_reasoning,
            OutputRenderStyle::Markdown,
        );
        let streamed_successfully = stream_writer.is_some_and(|writer| {
            writer.enabled()
                && writer.card_sent
                && !writer.stream_failed
                && !writer.saw_special_card
        });
        if !streamed_successfully
            && !body.trim().is_empty()
            && let Err(err) = self
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
        self.send_attachments(&inbound.session_webhook, &inbound.chat_id, output);
    }

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
            if let Some(event_id) = card_callback.event_id.as_deref()
                && !self.event_deduper.insert_if_new(event_id)
            {
                debug!(event_id, "ignoring duplicated dingtalk card callback");
                return send_ack(ws, envelope, "").await;
            }

            if !is_sender_allowed(&self.config.allowlist, &card_callback.sender_id) {
                warn!(
                    sender = card_callback.sender_id.as_str(),
                    "dingtalk sender blocked by allowlist"
                );
                return send_ack(ws, envelope, "").await;
            }

            let Some(verb) = card_callback.action.approval_verb() else {
                return send_ack(ws, envelope, "").await;
            };
            let command = format!("/{verb} {}", card_callback.approval_id);
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
                    metadata: runtime_metadata(
                        card_callback.session_webhook.as_deref(),
                        &self.config.bot_title,
                        true,
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

        let (maybe_output, stream_writer) = self
            .submit_request(
                runtime,
                ChannelRequest {
                    channel: self.name().to_string(),
                    input: inbound.text.clone(),
                    session_key,
                    chat_id: inbound.chat_id.clone(),
                    media_references: inbound.media_references.clone(),
                    metadata: runtime_metadata(
                        Some(&inbound.session_webhook),
                        &self.config.bot_title,
                        false,
                    ),
                },
                &inbound,
            )
            .await?;

        if let Some(output) = maybe_output {
            self.send_output(&inbound, &output, stream_writer.as_ref())
                .await;
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
