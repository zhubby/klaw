mod client;
mod render;
mod types;

use self::client::{BotCommand, EditMessageTextRequest, SendMessageRequest, TelegramApiClient};
use self::render::{build_im_card_message, render_telegram_response};
use self::types::{
    EventDeduper, TelegramBotProfile, TelegramCallbackInbound, TelegramInbound, TelegramUpdate,
    is_sender_allowed,
};
use crate::{
    Channel, ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime, ChannelStreamEvent,
    ChannelStreamWriter, LocalAttachmentPolicy, OutboundAttachment,
    manager::{
        ChannelInstanceConfig, ChannelKind, ChannelSupervisorReporter, ManagedChannelDriver,
    },
    media::{
        ArchiveMediaIngestContext, DEFAULT_INLINE_MEDIA_MAX_BYTES, ingest_media_reference_bytes,
    },
    outbound::resolve_outbound_attachment,
};
use klaw_archive::open_default_archive_service;
use klaw_config::{LocalAttachmentConfig, TelegramConfig, TelegramProxyConfig};
use klaw_core::OutboundMessage;
use klaw_util::{default_data_dir, workspace_dir};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

const TELEGRAM_RECONNECT_DELAY: Duration = Duration::from_secs(3);
const UPDATE_DEDUP_TTL: Duration = Duration::from_secs(60 * 60);
const UPDATE_DEDUP_MAX_ENTRIES: usize = 20_000;
type TelegramPollResult = Result<Vec<TelegramUpdate>, String>;

fn callback_runtime_metadata() -> BTreeMap<String, serde_json::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "channel.delivery_mode".to_string(),
        serde_json::Value::String("direct_reply".to_string()),
    );
    metadata
}

#[derive(Debug, Clone)]
pub struct TelegramChannel {
    config: TelegramChannelConfig,
    client: TelegramApiClient,
    update_deduper: EventDeduper,
    bot_profile: Option<TelegramBotProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramChannelConfig {
    pub account_id: String,
    pub bot_token: String,
    pub show_reasoning: bool,
    pub stream_output: bool,
    pub allowlist: Vec<String>,
    pub local_attachments: LocalAttachmentConfig,
    pub proxy: TelegramProxyConfig,
}

impl Default for TelegramChannelConfig {
    fn default() -> Self {
        Self {
            account_id: "default".to_string(),
            bot_token: String::new(),
            show_reasoning: false,
            stream_output: false,
            allowlist: Vec::new(),
            local_attachments: LocalAttachmentConfig::default(),
            proxy: TelegramProxyConfig::default(),
        }
    }
}

impl TelegramChannel {
    pub fn from_app_config(
        config: TelegramConfig,
        local_attachments: LocalAttachmentConfig,
    ) -> ChannelResult<Self> {
        Self::new(TelegramChannelConfig {
            account_id: config.id,
            bot_token: config.bot_token,
            show_reasoning: config.show_reasoning,
            stream_output: config.stream_output,
            allowlist: config.allowlist,
            local_attachments,
            proxy: config.proxy,
        })
    }

    pub fn new(config: TelegramChannelConfig) -> ChannelResult<Self> {
        let client = TelegramApiClient::new(&config.bot_token, &config.proxy)?;
        Ok(Self {
            config,
            client,
            update_deduper: EventDeduper::new(UPDATE_DEDUP_TTL, UPDATE_DEDUP_MAX_ENTRIES),
            bot_profile: None,
        })
    }

    fn validate_config(&self) -> ChannelResult<()> {
        if self.config.bot_token.trim().is_empty() {
            return Err("telegram bot_token is required".into());
        }
        Ok(())
    }

    async fn register_bot_commands(&self) -> ChannelResult<()> {
        self.client
            .set_my_commands(vec![
                BotCommand::new("start", "Start a new session context"),
                BotCommand::new("help", "Show help and available commands"),
                BotCommand::new("new", "Start a new session context"),
                BotCommand::new("model_provider", "List or switch model providers"),
                BotCommand::new("model", "Show or update current model"),
                BotCommand::new("approve", "Approve a pending tool action"),
                BotCommand::new("reject", "Reject a pending tool action"),
            ])
            .await
    }

    fn build_client(&self) -> ChannelResult<TelegramApiClient> {
        TelegramApiClient::new(&self.config.bot_token, &self.config.proxy)
    }

    pub async fn run_until_shutdown(
        &mut self,
        runtime: &dyn ChannelRuntime,
        shutdown: &mut watch::Receiver<bool>,
        reporter: ChannelSupervisorReporter,
    ) -> ChannelResult<()> {
        self.validate_config()?;
        info!(
            account_id = self.config.account_id.as_str(),
            "telegram channel started"
        );
        reporter.mark_running("telegram channel initialized");
        if let Err(err) = self.register_bot_commands().await {
            warn!(
                account_id = self.config.account_id.as_str(),
                error = %err,
                "failed to register telegram bot commands"
            );
        } else {
            info!(
                account_id = self.config.account_id.as_str(),
                "telegram bot commands registered"
            );
        }
        self.bot_profile = match self.client.get_me().await {
            Ok(user) => {
                let profile = TelegramBotProfile::from_user(user);
                if profile.is_none() {
                    warn!(
                        account_id = self.config.account_id.as_str(),
                        "telegram getMe returned bot profile without username; group mentions disabled"
                    );
                }
                profile
            }
            Err(err) => {
                warn!(
                    account_id = self.config.account_id.as_str(),
                    error = %err,
                    "failed to fetch telegram bot profile; group mention routing disabled"
                );
                None
            }
        };

        let (updates_tx, mut updates_rx) = mpsc::unbounded_channel::<TelegramPollResult>();
        let mut poll_shutdown = shutdown.clone();
        let mut client = self.client.clone();
        let account_id = self.config.account_id.clone();
        let bot_token = self.config.bot_token.clone();
        let proxy = self.config.proxy.clone();
        let poll_reporter = reporter.clone();
        let poll_task = tokio::spawn(async move {
            let mut offset: Option<i64> = None;
            let mut reconnect_attempt = 0_u32;
            loop {
                if *poll_shutdown.borrow() {
                    break;
                }

                match client
                    .get_updates(offset)
                    .await
                    .map_err(|err| err.to_string())
                {
                    Ok(updates) => {
                        reconnect_attempt = 0;
                        poll_reporter.record_activity("telegram polling request succeeded");
                        if let Some(last_update_id) = updates.last().map(|update| update.update_id)
                        {
                            offset = Some(last_update_id + 1);
                        }
                        if updates_tx.send(Ok(updates)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        reconnect_attempt = reconnect_attempt.saturating_add(1);
                        poll_reporter.mark_reconnecting(
                            reconnect_attempt,
                            format!("failed to fetch telegram updates: {error}"),
                        );
                        match TelegramApiClient::new(&bot_token, &proxy) {
                            Ok(rebuilt_client) => {
                                client = rebuilt_client;
                                info!(
                                    account_id = account_id.as_str(),
                                    "telegram polling client rebuilt after fetch failure"
                                );
                            }
                            Err(rebuild_error) => {
                                warn!(
                                    account_id = account_id.as_str(),
                                    error = %rebuild_error,
                                    "failed to rebuild telegram polling client"
                                );
                            }
                        }
                        if updates_tx.send(Err(error)).is_err() {
                            break;
                        }
                        tokio::select! {
                            _ = time::sleep(TELEGRAM_RECONNECT_DELAY) => {}
                            changed = poll_shutdown.changed() => {
                                if changed.is_ok() && *poll_shutdown.borrow() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            info!(
                account_id = account_id.as_str(),
                "telegram polling task stopped"
            );
        });

        let mut cron_tick = time::interval(runtime.cron_tick_interval());
        let mut runtime_tick = time::interval(runtime.runtime_tick_interval());
        let mut cron_job: Option<Pin<Box<dyn Future<Output = ()> + '_>>> = None;
        let mut runtime_job: Option<Pin<Box<dyn Future<Output = ()> + '_>>> = None;

        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_ok() && *shutdown.borrow() {
                        poll_task.abort();
                        info!(account_id = self.config.account_id.as_str(), "telegram shutdown requested");
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
                maybe_updates = updates_rx.recv() => {
                    let Some(updates) = maybe_updates else {
                        warn!(account_id = self.config.account_id.as_str(), "telegram polling task channel closed");
                        return Ok(());
                    };
                    match updates {
                        Ok(updates) => {
                            for update in updates {
                                if !self.update_deduper.insert_if_new(&update.update_id.to_string()) {
                                    debug!(update_id = update.update_id, "ignoring duplicated telegram update");
                                    continue;
                                }
                                if let Err(err) = self.handle_update(runtime, update).await {
                                    warn!(error = %err, "failed to process telegram update");
                                }
                            }
                        }
                        Err(err) => warn!(error = %err, "failed to fetch telegram updates"),
                    }
                }
            }
        }
    }

    async fn handle_update(
        &self,
        runtime: &dyn ChannelRuntime,
        update: TelegramUpdate,
    ) -> ChannelResult<()> {
        if let Some(callback_query) = update.callback_query {
            return self.handle_callback_query(runtime, callback_query).await;
        }

        let Some(message) = update.message else {
            debug!(
                update_id = update.update_id,
                "ignoring unsupported telegram update"
            );
            return Ok(());
        };
        let Some(from) = message.from.as_ref() else {
            debug!(
                update_id = update.update_id,
                "ignoring telegram message without sender"
            );
            return Ok(());
        };
        if from.is_bot {
            debug!(
                update_id = update.update_id,
                sender_id = from.id,
                "ignoring telegram bot message"
            );
            return Ok(());
        }

        let sender_id = from.id.to_string();
        if !is_sender_allowed(&self.config.allowlist, &sender_id) {
            warn!(
                sender = sender_id.as_str(),
                "telegram sender blocked by allowlist"
            );
            return Ok(());
        }

        let Some(mut inbound) =
            TelegramInbound::from_message(update.update_id, message, self.bot_profile.as_ref())
        else {
            return Ok(());
        };
        let session_key = format!("telegram:{}:{}", self.config.account_id, inbound.chat_id);
        self.materialize_media_references(&session_key, &mut inbound)
            .await;

        let maybe_output = self
            .submit_request(
                runtime,
                ChannelRequest {
                    channel: self.name().to_string(),
                    input: inbound.text.clone(),
                    session_key,
                    chat_id: inbound.chat_id.clone(),
                    media_references: inbound.media_references.clone(),
                    metadata: {
                        let mut metadata = callback_runtime_metadata();
                        metadata.extend(inbound.metadata.clone());
                        metadata
                    },
                },
                Some(inbound.chat_id.as_str()),
            )
            .await;

        match maybe_output {
            Ok(Some(output)) => {
                if !self.config.stream_output {
                    self.send_output(&inbound.chat_id, &output).await?;
                }
            }
            Ok(None) => {}
            Err(err) => return Err(err),
        }

        Ok(())
    }

    async fn handle_callback_query(
        &self,
        runtime: &dyn ChannelRuntime,
        query: types::TelegramCallbackQuery,
    ) -> ChannelResult<()> {
        if query.from.is_bot {
            return Ok(());
        }
        let sender_id = query.from.id.to_string();
        if !is_sender_allowed(&self.config.allowlist, &sender_id) {
            warn!(
                sender = sender_id.as_str(),
                "telegram callback sender blocked by allowlist"
            );
            return Ok(());
        }
        let Some(inbound) = TelegramCallbackInbound::from_callback(query) else {
            return Ok(());
        };
        let session_key = format!("telegram:{}:{}", self.config.account_id, inbound.chat_id);
        let maybe_output = self
            .submit_request(
                runtime,
                ChannelRequest {
                    channel: self.name().to_string(),
                    input: inbound.command.clone(),
                    session_key,
                    chat_id: inbound.chat_id.clone(),
                    media_references: Vec::new(),
                    metadata: callback_runtime_metadata(),
                },
                Some(inbound.chat_id.as_str()),
            )
            .await;

        match maybe_output {
            Ok(Some(output)) => {
                self.client
                    .answer_callback_query(&inbound.callback_id, "Processed")
                    .await?;
                if !self.config.stream_output {
                    self.send_output(&inbound.chat_id, &output).await?;
                }
            }
            Ok(None) => {
                self.client
                    .answer_callback_query(&inbound.callback_id, "No response")
                    .await?;
            }
            Err(err) => {
                let _ = self
                    .client
                    .answer_callback_query(&inbound.callback_id, "Failed")
                    .await;
                return Err(err);
            }
        }

        Ok(())
    }

    async fn send_output(&self, chat_id: &str, output: &ChannelResponse) -> ChannelResult<()> {
        let card = crate::im_card::resolve_im_card(output);
        let request = if let Some(card) = card.as_ref() {
            let mut request = SendMessageRequest::html(chat_id, &build_im_card_message(card));
            if let Some(markup) = types::TelegramInlineKeyboardMarkup::from_im_card(card) {
                request = request.with_reply_markup(markup);
            }
            request
        } else {
            SendMessageRequest::html(
                chat_id,
                &render_telegram_response(output, self.config.show_reasoning),
            )
        };
        let _ = match self.client.send_message(request.clone()).await {
            Ok(message) => message,
            Err(error) => {
                warn!(
                    chat_id,
                    error = %error,
                    "telegram send_message failed, rebuilding client and retrying"
                );
                let rebuilt_client = self.build_client()?;
                rebuilt_client.send_message(request).await?
            }
        };
        if card.is_none() {
            self.send_attachments(chat_id, output);
        }
        Ok(())
    }

    fn send_attachments(&self, chat_id: &str, output: &ChannelResponse) {
        if output.attachments.is_empty() {
            return;
        }
        let client = self.client.clone();
        let chat_id = chat_id.to_string();
        let attachments = output.attachments.clone();
        let local_policy = match self.local_attachment_policy() {
            Ok(policy) => policy,
            Err(error) => {
                warn!(chat_id, error = %error, "failed to resolve telegram local attachment policy");
                return;
            }
        };
        tokio::task::spawn_local(async move {
            deliver_telegram_attachments(client, chat_id, attachments, local_policy).await;
        });
    }

    async fn submit_request(
        &self,
        runtime: &dyn ChannelRuntime,
        request: ChannelRequest,
        chat_id: Option<&str>,
    ) -> ChannelResult<Option<ChannelResponse>> {
        if self.config.stream_output
            && let Some(chat_id) = chat_id
        {
            let mut writer = TelegramStreamWriter::new(
                self.client.clone(),
                self.config.bot_token.clone(),
                self.config.proxy.clone(),
                self.config.local_attachments.clone(),
                chat_id.to_string(),
                self.config.show_reasoning,
            );
            let output = runtime.submit_streaming(request, &mut writer).await?;
            if let Some(ref output) = output {
                writer.finish(output).await?;
            }
            return Ok(output);
        }
        runtime.submit(request).await
    }

    async fn materialize_media_references(&self, session_key: &str, inbound: &mut TelegramInbound) {
        if inbound.media_references.is_empty() {
            return;
        }

        let archive_service = match open_default_archive_service().await {
            Ok(service) => service,
            Err(err) => {
                warn!(update_id = inbound.update_id, error = %err, "failed to open archive service for telegram media ingestion");
                return;
            }
        };

        for media in &mut inbound.media_references {
            let Some(file_id) = media
                .metadata
                .get("telegram.file_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
            else {
                continue;
            };

            let file = match self.client.get_file(&file_id).await {
                Ok(file) => file,
                Err(err) => {
                    warn!(update_id = inbound.update_id, file_id, error = %err, "failed to resolve telegram file path");
                    continue;
                }
            };
            let Some(file_path) = file
                .file_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                warn!(
                    update_id = inbound.update_id,
                    file_id, "telegram file path missing"
                );
                continue;
            };

            let bytes = match self.client.download_file(file_path).await {
                Ok(bytes) => bytes,
                Err(err) => {
                    warn!(update_id = inbound.update_id, file_id, error = %err, "failed to download telegram media content");
                    continue;
                }
            };
            media.metadata.insert(
                "telegram.file_path".to_string(),
                Value::String(file_path.to_string()),
            );

            if let Err(err) = ingest_media_reference_bytes(
                &archive_service,
                ArchiveMediaIngestContext {
                    session_key,
                    channel: self.name(),
                    chat_id: inbound.chat_id.as_str(),
                    message_id: inbound.message_id.as_str(),
                },
                media,
                &bytes,
                DEFAULT_INLINE_MEDIA_MAX_BYTES,
                "telegram.inline_media",
                "telegram.inline_media_skipped_bytes",
            )
            .await
            {
                warn!(update_id = inbound.update_id, file_id = file_id.as_str(), error = %err, "failed to ingest telegram media into archive");
            }
        }
    }
}

async fn deliver_telegram_attachments(
    client: TelegramApiClient,
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
                "failed to open archive service for telegram outbound attachments"
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
                        "failed to resolve telegram outbound attachment"
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
            "resolved telegram outbound attachment"
        );

        let result = match resolved.kind {
            crate::OutboundAttachmentKind::Image => {
                client
                    .send_photo_bytes(
                        &chat_id,
                        &resolved.filename,
                        &resolved.bytes,
                        resolved.caption.as_deref(),
                    )
                    .await
            }
            crate::OutboundAttachmentKind::File => {
                client
                    .send_document_bytes(
                        &chat_id,
                        &resolved.filename,
                        &resolved.bytes,
                        resolved.caption.as_deref(),
                    )
                    .await
            }
        };

        if let Err(error) = result {
            warn!(
                chat_id,
                source = resolved.source_label.as_str(),
                error = %error,
                "failed to send telegram outbound attachment"
            );
        } else {
            debug!(
                chat_id,
                source = resolved.source_label.as_str(),
                "sent telegram outbound attachment"
            );
        }
    }
}

pub async fn dispatch_background_outbound(
    config: &TelegramConfig,
    output: &OutboundMessage,
) -> ChannelResult<()> {
    let client = TelegramApiClient::new(&config.bot_token, &config.proxy)?;
    let response = ChannelResponse {
        content: output.content.clone(),
        reasoning: output
            .metadata
            .get("reasoning")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
            .filter(|value| !value.trim().is_empty()),
        metadata: output.metadata.clone(),
        attachments: Vec::new(),
    };
    let card = crate::im_card::resolve_im_card(&response);
    let request = if let Some(card) = card.as_ref() {
        let mut request = SendMessageRequest::html(&output.chat_id, &build_im_card_message(card));
        if let Some(markup) = types::TelegramInlineKeyboardMarkup::from_im_card(card) {
            request = request.with_reply_markup(markup);
        }
        request
    } else {
        SendMessageRequest::html(
            &output.chat_id,
            &render_telegram_response(&response, config.show_reasoning),
        )
    };
    let _ = client.send_message(request).await?;
    Ok(())
}

const TELEGRAM_STREAM_UPDATE_INTERVAL: Duration = Duration::from_millis(150);

struct TelegramStreamWriter {
    client: TelegramApiClient,
    bot_token: String,
    proxy: TelegramProxyConfig,
    local_attachments: LocalAttachmentConfig,
    chat_id: String,
    show_reasoning: bool,
    message_id: Option<i64>,
    last_rendered: Option<String>,
    last_update_at: Option<Instant>,
}

impl TelegramStreamWriter {
    fn new(
        client: TelegramApiClient,
        bot_token: String,
        proxy: TelegramProxyConfig,
        local_attachments: LocalAttachmentConfig,
        chat_id: String,
        show_reasoning: bool,
    ) -> Self {
        Self {
            client,
            bot_token,
            proxy,
            local_attachments,
            chat_id,
            show_reasoning,
            message_id: None,
            last_rendered: None,
            last_update_at: None,
        }
    }

    fn rebuild_client(&mut self) -> ChannelResult<()> {
        self.client = TelegramApiClient::new(&self.bot_token, &self.proxy)?;
        Ok(())
    }

    async fn send_message_with_retry(
        &mut self,
        request: SendMessageRequest,
    ) -> ChannelResult<types::TelegramMessage> {
        match self.client.send_message(request.clone()).await {
            Ok(message) => Ok(message),
            Err(error) => {
                warn!(
                    chat_id = self.chat_id.as_str(),
                    error = %error,
                    "telegram stream send_message failed, rebuilding client and retrying"
                );
                self.rebuild_client()?;
                self.client.send_message(request).await
            }
        }
    }

    async fn edit_message_with_retry(
        &mut self,
        request: EditMessageTextRequest,
    ) -> ChannelResult<types::TelegramMessage> {
        match self.client.edit_message_text(request.clone()).await {
            Ok(message) => Ok(message),
            Err(error) => {
                warn!(
                    chat_id = self.chat_id.as_str(),
                    error = %error,
                    "telegram stream edit_message_text failed, rebuilding client and retrying"
                );
                self.rebuild_client()?;
                self.client.edit_message_text(request).await
            }
        }
    }

    async fn finish(&mut self, output: &ChannelResponse) -> ChannelResult<()> {
        let card = crate::im_card::resolve_im_card(output);
        let approval_markup = card
            .as_ref()
            .and_then(types::TelegramInlineKeyboardMarkup::from_im_card);
        let text = if let Some(card) = card.as_ref() {
            build_im_card_message(card)
        } else {
            render_telegram_response(output, self.show_reasoning)
        };
        if text.trim().is_empty() {
            return Ok(());
        }
        self.last_rendered = Some(text.clone());
        match self.message_id {
            Some(message_id) => {
                let mut request = EditMessageTextRequest::html(&self.chat_id, message_id, &text);
                if let Some(markup) = approval_markup {
                    request = request.with_reply_markup(markup);
                }
                let _ = self.edit_message_with_retry(request).await?;
            }
            None => {
                let mut request = SendMessageRequest::html(&self.chat_id, &text);
                if let Some(markup) = approval_markup {
                    request = request.with_reply_markup(markup);
                }
                let message = self.send_message_with_retry(request).await?;
                self.message_id = Some(message.message_id);
            }
        }
        self.last_update_at = Some(Instant::now());
        if card.is_none() {
            self.send_attachments(output);
        }
        Ok(())
    }

    fn send_attachments(&mut self, output: &ChannelResponse) {
        if output.attachments.is_empty() {
            return;
        }
        let client = self.client.clone();
        let chat_id = self.chat_id.clone();
        let attachments = output.attachments.clone();
        let local_policy = match self.local_attachment_policy() {
            Ok(policy) => policy,
            Err(error) => {
                warn!(chat_id = self.chat_id.as_str(), error = %error, "failed to resolve telegram local attachment policy");
                return;
            }
        };
        tokio::task::spawn_local(async move {
            deliver_telegram_attachments(client, chat_id, attachments, local_policy).await;
        });
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

impl TelegramChannel {
    fn local_attachment_policy(&self) -> ChannelResult<LocalAttachmentPolicy> {
        resolve_local_attachment_policy(&self.config.local_attachments)
    }
}

impl TelegramStreamWriter {
    fn local_attachment_policy(&self) -> ChannelResult<LocalAttachmentPolicy> {
        resolve_local_attachment_policy(&self.local_attachments)
    }
}

#[async_trait::async_trait(?Send)]
impl ChannelStreamWriter for TelegramStreamWriter {
    async fn write(&mut self, event: ChannelStreamEvent) -> ChannelResult<()> {
        match event {
            ChannelStreamEvent::Snapshot(output) => {
                let card = crate::im_card::resolve_im_card(&output);
                let approval_markup = card
                    .as_ref()
                    .and_then(types::TelegramInlineKeyboardMarkup::from_im_card);
                let text = if let Some(card) = card.as_ref() {
                    build_im_card_message(card)
                } else {
                    render_telegram_response(&output, self.show_reasoning)
                };
                if text.trim().is_empty() {
                    return Ok(());
                }
                if self.last_rendered.as_deref() == Some(text.as_str()) {
                    return Ok(());
                }
                self.last_rendered = Some(text.clone());
                let should_flush = self.message_id.is_none()
                    || self
                        .last_update_at
                        .is_none_or(|instant| instant.elapsed() >= TELEGRAM_STREAM_UPDATE_INTERVAL)
                    || approval_markup.is_some();
                if !should_flush {
                    return Ok(());
                }
                match self.message_id {
                    Some(message_id) => {
                        let mut request =
                            EditMessageTextRequest::html(&self.chat_id, message_id, &text);
                        if let Some(markup) = approval_markup {
                            request = request.with_reply_markup(markup);
                        }
                        let _ = self.edit_message_with_retry(request).await?;
                    }
                    None => {
                        let mut request = SendMessageRequest::html(&self.chat_id, &text);
                        if let Some(markup) = approval_markup {
                            request = request.with_reply_markup(markup);
                        }
                        let message = self.send_message_with_retry(request).await?;
                        self.message_id = Some(message.message_id);
                    }
                }
                self.last_update_at = Some(Instant::now());
                Ok(())
            }
            ChannelStreamEvent::Clear => {
                self.last_rendered = None;
                if let Some(message_id) = self.message_id {
                    let _ = self
                        .client
                        .edit_message_text(EditMessageTextRequest::html(
                            &self.chat_id,
                            message_id,
                            "<i>Processing...</i>",
                        ))
                        .await;
                    self.last_update_at = Some(Instant::now());
                }
                Ok(())
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ManagedChannelDriver for TelegramChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Telegram
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
        TelegramChannel::run_until_shutdown(self, runtime, shutdown, reporter).await
    }
}

#[async_trait::async_trait(?Send)]
impl Channel for TelegramChannel {
    fn name(&self) -> &'static str {
        "telegram"
    }

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let config = ChannelInstanceConfig::Telegram(TelegramConfig {
            id: self.config.account_id.clone(),
            enabled: true,
            bot_token: self.config.bot_token.clone(),
            show_reasoning: self.config.show_reasoning,
            stream_output: self.config.stream_output,
            allowlist: self.config.allowlist.clone(),
            proxy: self.config.proxy.clone(),
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

#[cfg(test)]
mod tests {
    use super::render::{build_im_card_message, render_telegram_response, resolve_approval_card};
    use super::types::{
        TelegramAudio, TelegramCallbackInbound, TelegramCallbackQuery, TelegramChat,
        TelegramChatType, TelegramDocument, TelegramInlineKeyboardMarkup, TelegramMessage,
        TelegramPhotoSize, TelegramUser, extract_media_references, is_sender_allowed, message_text,
    };
    use crate::ChannelResponse;
    use serde_json::Value;
    use std::collections::BTreeMap;

    fn private_chat() -> TelegramChat {
        TelegramChat {
            id: 10,
            kind: TelegramChatType::Private,
        }
    }

    #[test]
    fn message_text_prefers_text_over_caption() {
        let message = TelegramMessage {
            message_id: 1,
            chat: private_chat(),
            from: Some(TelegramUser {
                id: 100,
                is_bot: false,
                username: None,
            }),
            text: Some("hello".to_string()),
            entities: Vec::new(),
            caption: Some("caption".to_string()),
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };
        assert_eq!(message_text(&message).as_deref(), Some("hello"));
    }

    #[test]
    fn message_text_falls_back_to_caption_and_attachment_summary() {
        let caption_message = TelegramMessage {
            message_id: 1,
            chat: private_chat(),
            from: None,
            text: None,
            entities: Vec::new(),
            caption: Some("photo caption".to_string()),
            caption_entities: Vec::new(),
            photo: vec![TelegramPhotoSize {
                file_id: "f1".to_string(),
                file_unique_id: "u1".to_string(),
                width: 10,
                height: 10,
            }],
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };
        assert_eq!(
            message_text(&caption_message).as_deref(),
            Some("photo caption")
        );

        let audio_only = TelegramMessage {
            message_id: 2,
            chat: private_chat(),
            from: None,
            text: None,
            entities: Vec::new(),
            caption: None,
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: Some(TelegramAudio {
                file_id: "a1".to_string(),
                file_unique_id: "u2".to_string(),
                file_name: Some("clip.mp3".to_string()),
                mime_type: Some("audio/mpeg".to_string()),
            }),
            voice: None,
            video: None,
            reply_to_message: None,
        };
        assert_eq!(
            message_text(&audio_only).as_deref(),
            Some("Received audio attachment.")
        );
    }

    #[test]
    fn extract_media_references_builds_photo_and_document() {
        let message = TelegramMessage {
            message_id: 22,
            chat: private_chat(),
            from: None,
            text: None,
            entities: Vec::new(),
            caption: None,
            caption_entities: Vec::new(),
            photo: vec![TelegramPhotoSize {
                file_id: "photo-1".to_string(),
                file_unique_id: "uniq-photo".to_string(),
                width: 100,
                height: 100,
            }],
            document: Some(TelegramDocument {
                file_id: "doc-1".to_string(),
                file_unique_id: "uniq-doc".to_string(),
                file_name: Some("report.pdf".to_string()),
                mime_type: Some("application/pdf".to_string()),
            }),
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        };

        let media = extract_media_references(&message);
        assert_eq!(media.len(), 2);
        assert_eq!(
            media[0]
                .metadata
                .get("telegram.file_id")
                .and_then(Value::as_str),
            Some("photo-1")
        );
        assert_eq!(
            media[1]
                .metadata
                .get("telegram.declared_file_extension")
                .and_then(Value::as_str),
            Some("pdf")
        );
    }

    #[test]
    fn callback_query_maps_to_command() {
        let inbound = TelegramCallbackInbound::from_callback(TelegramCallbackQuery {
            id: "cb-1".to_string(),
            from: TelegramUser {
                id: 1,
                is_bot: false,
                username: None,
            },
            data: Some("approve:approval-1".to_string()),
            message: Some(TelegramMessage {
                message_id: 1,
                chat: private_chat(),
                from: None,
                text: None,
                entities: Vec::new(),
                caption: None,
                caption_entities: Vec::new(),
                photo: Vec::new(),
                document: None,
                audio: None,
                voice: None,
                video: None,
                reply_to_message: None,
            }),
        })
        .expect("callback inbound");

        assert_eq!(inbound.chat_id, "10");
        assert_eq!(inbound.command, "/approve approval-1");
    }

    #[test]
    fn sender_allowlist_supports_wildcard() {
        assert!(is_sender_allowed(&[], "123"));
        assert!(is_sender_allowed(&["*".to_string()], "123"));
        assert!(is_sender_allowed(&["123".to_string()], "123"));
        assert!(!is_sender_allowed(&["456".to_string()], "123"));
    }

    #[test]
    fn telegram_render_uses_html_and_approval_markup() {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "approval.id".to_string(),
            Value::String("approval-1".to_string()),
        );
        metadata.insert(
            "approval.signal".to_string(),
            serde_json::json!({"command_preview": "python3 -c \"print(1)\""}),
        );
        let output = ChannelResponse {
            content: "**Title**\n\n```text\n/help\n```".to_string(),
            reasoning: Some("line 1".to_string()),
            metadata,
            attachments: Vec::new(),
        };

        let rendered = render_telegram_response(&output, true);
        assert!(rendered.contains("<b>Title</b>"));
        assert!(rendered.contains("<pre><code class=\"language-text\">/help</code></pre>"));

        let card = resolve_approval_card(&output).expect("approval card");
        let approval = build_im_card_message(&card);
        assert!(approval.contains("Approval Required"));

        let keyboard = TelegramInlineKeyboardMarkup::from_im_card(&card).expect("keyboard");
        assert_eq!(
            keyboard.inline_keyboard[0][0].callback_data,
            Some("approve:approval-1".to_string())
        );
    }
}
