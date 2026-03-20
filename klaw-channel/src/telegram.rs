use crate::{
    manager::{ChannelKind, ManagedChannelDriver},
    media::{
        attach_declared_media_metadata, build_media_reference, ingest_media_reference_bytes,
        ArchiveMediaIngestContext, DEFAULT_INLINE_MEDIA_MAX_BYTES,
    },
    render::{render_agent_output, OutputRenderStyle},
    Channel, ChannelRequest, ChannelResult, ChannelRuntime,
};
use klaw_archive::open_default_archive_service;
use klaw_config::{TelegramConfig, TelegramProxyConfig};
use klaw_core::{MediaReference, MediaSourceKind};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;
use tokio::sync::{mpsc, watch};
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";
const TELEGRAM_LONG_POLL_TIMEOUT_SECS: u64 = 20;
const TELEGRAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const TELEGRAM_RECONNECT_DELAY: Duration = Duration::from_secs(3);
const UPDATE_DEDUP_TTL: Duration = Duration::from_secs(60 * 60);
const UPDATE_DEDUP_MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Clone)]
pub struct TelegramChannel {
    config: TelegramChannelConfig,
    client: TelegramApiClient,
    update_deduper: EventDeduper,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramChannelConfig {
    pub account_id: String,
    pub bot_token: String,
    pub show_reasoning: bool,
    pub allowlist: Vec<String>,
    pub proxy: TelegramProxyConfig,
}

impl Default for TelegramChannelConfig {
    fn default() -> Self {
        Self {
            account_id: "default".to_string(),
            bot_token: String::new(),
            show_reasoning: false,
            allowlist: Vec::new(),
            proxy: TelegramProxyConfig::default(),
        }
    }
}

impl TelegramChannel {
    pub fn from_app_config(config: TelegramConfig) -> ChannelResult<Self> {
        Self::new(TelegramChannelConfig {
            account_id: config.id,
            bot_token: config.bot_token,
            show_reasoning: config.show_reasoning,
            allowlist: config.allowlist,
            proxy: config.proxy,
        })
    }

    pub fn new(config: TelegramChannelConfig) -> ChannelResult<Self> {
        let client = TelegramApiClient::new(&config.bot_token, &config.proxy)?;
        Ok(Self {
            config,
            client,
            update_deduper: EventDeduper::new(UPDATE_DEDUP_TTL, UPDATE_DEDUP_MAX_ENTRIES),
        })
    }

    fn validate_config(&self) -> ChannelResult<()> {
        if self.config.bot_token.trim().is_empty() {
            return Err("telegram bot_token is required".into());
        }
        Ok(())
    }

    pub async fn run_until_shutdown(
        &mut self,
        runtime: &dyn ChannelRuntime,
        shutdown: &mut watch::Receiver<bool>,
    ) -> ChannelResult<()> {
        self.validate_config()?;
        info!(
            account_id = self.config.account_id.as_str(),
            "telegram channel started"
        );

        let (updates_tx, mut updates_rx) = mpsc::unbounded_channel::<TelegramPollResult>();
        let mut poll_shutdown = shutdown.clone();
        let client = self.client.clone();
        let account_id = self.config.account_id.clone();
        let poll_task = tokio::spawn(async move {
            let mut offset: Option<i64> = None;
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
                        if let Some(last_update_id) = updates.last().map(|update| update.update_id)
                        {
                            offset = Some(last_update_id + 1);
                        }
                        if updates_tx.send(Ok(updates)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
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
                        Err(err) => {
                            warn!(error = %err, "failed to fetch telegram updates");
                        }
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

        let Some(mut inbound) = TelegramInbound::from_message(update.update_id, message) else {
            return Ok(());
        };
        let session_key = format!("telegram:{}:{}", self.config.account_id, inbound.chat_id);
        self.materialize_media_references(&session_key, &mut inbound)
            .await;

        let maybe_output = runtime
            .submit(ChannelRequest {
                channel: self.name().to_string(),
                input: inbound.text.clone(),
                session_key,
                chat_id: inbound.chat_id.clone(),
                media_references: inbound.media_references.clone(),
                metadata: BTreeMap::new(),
            })
            .await;

        match maybe_output {
            Ok(Some(output)) => {
                let text = render_agent_output(
                    &output,
                    self.config.show_reasoning,
                    OutputRenderStyle::Markdown,
                );
                self.client.send_message(&inbound.chat_id, &text).await?;
            }
            Ok(None) => {}
            Err(err) => return Err(err),
        }

        Ok(())
    }

    async fn materialize_media_references(&self, session_key: &str, inbound: &mut TelegramInbound) {
        if inbound.media_references.is_empty() {
            return;
        }

        let archive_service = match open_default_archive_service().await {
            Ok(service) => service,
            Err(err) => {
                warn!(
                    update_id = inbound.update_id,
                    error = %err,
                    "failed to open archive service for telegram media ingestion"
                );
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

            let file_path = match file
                .file_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                Some(path) => path,
                None => {
                    warn!(
                        update_id = inbound.update_id,
                        file_id, "telegram file path missing"
                    );
                    continue;
                }
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

            match ingest_media_reference_bytes(
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
                Ok(record) => {
                    info!(
                        update_id = inbound.update_id,
                        archive_id = record.id.as_str(),
                        storage_rel_path = record.storage_rel_path.as_str(),
                        "telegram media archived"
                    );
                }
                Err(err) => {
                    warn!(update_id = inbound.update_id, file_id = file_id.as_str(), error = %err, "failed to ingest telegram media into archive");
                }
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
    ) -> ChannelResult<()> {
        TelegramChannel::run_until_shutdown(self, runtime, shutdown).await
    }
}

#[async_trait::async_trait(?Send)]
impl Channel for TelegramChannel {
    fn name(&self) -> &'static str {
        "telegram"
    }

    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()> {
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
        self.run_until_shutdown(runtime, &mut shutdown_rx).await
    }
}

#[derive(Debug, Clone)]
struct TelegramApiClient {
    http: reqwest::Client,
    api_base: String,
    file_base: String,
}

impl TelegramApiClient {
    fn new(bot_token: &str, proxy: &TelegramProxyConfig) -> ChannelResult<Self> {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .timeout(TELEGRAM_REQUEST_TIMEOUT);
        if proxy.enabled {
            let proxy_url = proxy.url.trim();
            if proxy_url.is_empty() {
                return Err("telegram proxy.url is required when proxy.enabled=true".into());
            }
            builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
        }
        let http = builder.build()?;
        let api_base = format!("{TELEGRAM_API_BASE}/bot{}", bot_token.trim());
        let file_base = format!("{TELEGRAM_API_BASE}/file/bot{}", bot_token.trim());
        Ok(Self {
            http,
            api_base,
            file_base,
        })
    }

    async fn get_updates(&self, offset: Option<i64>) -> ChannelResult<Vec<TelegramUpdate>> {
        let payload = GetUpdatesRequest {
            offset,
            timeout: TELEGRAM_LONG_POLL_TIMEOUT_SECS,
            allowed_updates: vec!["message".to_string()],
        };
        self.post("getUpdates", &payload).await
    }

    async fn get_file(&self, file_id: &str) -> ChannelResult<TelegramFile> {
        self.post("getFile", &GetFileRequest { file_id }).await
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> ChannelResult<()> {
        let _: TelegramMessage = self
            .post("sendMessage", &build_send_message_request(chat_id, text))
            .await?;
        Ok(())
    }

    async fn download_file(&self, file_path: &str) -> ChannelResult<Vec<u8>> {
        let url = format!("{}/{}", self.file_base, file_path.trim_start_matches('/'));
        let response = self.http.get(url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "telegram download file failed: HTTP {} body={}",
                status, body
            )
            .into());
        }
        Ok(response.bytes().await?.to_vec())
    }

    async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        body: &B,
    ) -> ChannelResult<T> {
        let url = format!("{}/{}", self.api_base, method);
        let response = self.http.post(url).json(body).send().await?;
        let status = response.status();
        let envelope: TelegramApiEnvelope<T> = response.json().await?;
        if !status.is_success() || !envelope.ok {
            return Err(format!(
                "telegram {} failed: HTTP {} description={}",
                method,
                status,
                envelope
                    .description
                    .as_deref()
                    .unwrap_or("unknown telegram api error")
            )
            .into());
        }
        envelope
            .result
            .ok_or_else(|| format!("telegram {} missing result", method).into())
    }
}

#[derive(Debug, Clone)]
struct TelegramInbound {
    update_id: i64,
    message_id: String,
    chat_id: String,
    text: String,
    media_references: Vec<MediaReference>,
}

type TelegramPollResult = Result<Vec<TelegramUpdate>, String>;

impl TelegramInbound {
    fn from_message(update_id: i64, message: TelegramMessage) -> Option<Self> {
        let text = message_text(&message)?;
        Some(Self {
            update_id,
            message_id: message.message_id.to_string(),
            chat_id: message.chat.id.to_string(),
            text,
            media_references: extract_media_references(&message),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct TelegramApiEnvelope<T> {
    ok: bool,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GetUpdatesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i64>,
    timeout: u64,
    allowed_updates: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GetFileRequest<'a> {
    file_id: &'a str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct SendMessageRequest {
    chat_id: String,
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    chat: TelegramChat,
    #[serde(default)]
    from: Option<TelegramUser>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    photo: Vec<TelegramPhotoSize>,
    #[serde(default)]
    document: Option<TelegramDocument>,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramUser {
    id: i64,
    #[serde(default)]
    is_bot: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramPhotoSize {
    file_id: String,
    #[serde(default)]
    file_unique_id: String,
    #[serde(default)]
    width: i64,
    #[serde(default)]
    height: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramDocument {
    file_id: String,
    #[serde(default)]
    file_unique_id: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TelegramFile {
    #[serde(default)]
    file_path: Option<String>,
}

fn build_send_message_request(chat_id: &str, text: &str) -> SendMessageRequest {
    SendMessageRequest {
        chat_id: chat_id.trim().to_string(),
        text: text.trim().to_string(),
    }
}

fn message_text(message: &TelegramMessage) -> Option<String> {
    if let Some(text) = message
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(text.to_string());
    }
    if let Some(caption) = message
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(caption.to_string());
    }
    if !message.photo.is_empty() {
        return Some("Received photo attachment.".to_string());
    }
    if message.document.is_some() {
        return Some("Received document attachment.".to_string());
    }
    None
}

fn extract_media_references(message: &TelegramMessage) -> Vec<MediaReference> {
    let mut out = Vec::new();

    if let Some(photo) = message
        .photo
        .iter()
        .max_by_key(|photo| (photo.width, photo.height))
    {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "telegram.file_id".to_string(),
            Value::String(photo.file_id.clone()),
        );
        if !photo.file_unique_id.trim().is_empty() {
            metadata.insert(
                "telegram.file_unique_id".to_string(),
                Value::String(photo.file_unique_id.clone()),
            );
        }
        attach_declared_media_metadata(
            &mut metadata,
            Some("image/jpeg"),
            Some("jpg"),
            "telegram.declared_mime_type",
            "telegram.declared_file_extension",
        );
        out.push(build_media_reference(
            MediaSourceKind::ChannelInbound,
            &message.message_id.to_string(),
            Some(format!("telegram-photo-{}.jpg", message.message_id)),
            Some("image/jpeg".to_string()),
            metadata,
        ));
    }

    if let Some(document) = message.document.as_ref() {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "telegram.file_id".to_string(),
            Value::String(document.file_id.clone()),
        );
        if !document.file_unique_id.trim().is_empty() {
            metadata.insert(
                "telegram.file_unique_id".to_string(),
                Value::String(document.file_unique_id.clone()),
            );
        }
        attach_declared_media_metadata(
            &mut metadata,
            document.mime_type.as_deref(),
            document
                .file_name
                .as_deref()
                .and_then(file_extension_from_name),
            "telegram.declared_mime_type",
            "telegram.declared_file_extension",
        );
        out.push(build_media_reference(
            MediaSourceKind::ChannelInbound,
            &message.message_id.to_string(),
            document.file_name.clone(),
            document.mime_type.clone(),
            metadata,
        ));
    }

    out
}

fn file_extension_from_name(name: &str) -> Option<&str> {
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.trim();
    if ext.is_empty() {
        None
    } else {
        Some(ext)
    }
}

fn is_sender_allowed(allowlist: &[String], sender_id: &str) -> bool {
    if allowlist.is_empty() {
        return true;
    }
    allowlist.iter().any(|allowed| {
        let allowed = allowed.trim();
        allowed == "*" || allowed == sender_id
    })
}

#[derive(Debug, Clone)]
struct EventDeduper {
    ttl: Duration,
    max_entries: usize,
    timestamps: HashMap<String, Instant>,
    order: VecDeque<String>,
}

impl EventDeduper {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries,
            timestamps: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert_if_new(&mut self, key: &str) -> bool {
        let now = Instant::now();
        self.evict_expired(now);
        if self.timestamps.contains_key(key) {
            return false;
        }
        let key = key.to_string();
        self.timestamps.insert(key.clone(), now);
        self.order.push_back(key);
        while self.order.len() > self.max_entries {
            if let Some(front) = self.order.pop_front() {
                self.timestamps.remove(&front);
            }
        }
        true
    }

    fn evict_expired(&mut self, now: Instant) {
        while let Some(front) = self.order.front().cloned() {
            let Some(timestamp) = self.timestamps.get(&front).copied() else {
                self.order.pop_front();
                continue;
            };
            if now.duration_since(timestamp) < self.ttl {
                break;
            }
            self.order.pop_front();
            self.timestamps.remove(&front);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_text_prefers_text_over_caption() {
        let message = TelegramMessage {
            message_id: 1,
            chat: TelegramChat { id: 10 },
            from: Some(TelegramUser {
                id: 100,
                is_bot: false,
            }),
            text: Some("hello".to_string()),
            caption: Some("caption".to_string()),
            photo: Vec::new(),
            document: None,
        };

        assert_eq!(message_text(&message).as_deref(), Some("hello"));
    }

    #[test]
    fn message_text_falls_back_to_caption_and_attachment_summary() {
        let caption_message = TelegramMessage {
            message_id: 1,
            chat: TelegramChat { id: 10 },
            from: None,
            text: None,
            caption: Some("photo caption".to_string()),
            photo: vec![TelegramPhotoSize {
                file_id: "f1".to_string(),
                file_unique_id: "u1".to_string(),
                width: 10,
                height: 10,
            }],
            document: None,
        };
        assert_eq!(
            message_text(&caption_message).as_deref(),
            Some("photo caption")
        );

        let photo_only_message = TelegramMessage {
            message_id: 2,
            chat: TelegramChat { id: 10 },
            from: None,
            text: None,
            caption: None,
            photo: vec![TelegramPhotoSize {
                file_id: "f2".to_string(),
                file_unique_id: "u2".to_string(),
                width: 10,
                height: 10,
            }],
            document: None,
        };
        assert_eq!(
            message_text(&photo_only_message).as_deref(),
            Some("Received photo attachment.")
        );
    }

    #[test]
    fn extract_media_references_builds_photo_and_document() {
        let message = TelegramMessage {
            message_id: 22,
            chat: TelegramChat { id: 10 },
            from: None,
            text: None,
            caption: None,
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
    fn telegram_inbound_uses_chat_scoped_session_fields() {
        let message = TelegramMessage {
            message_id: 33,
            chat: TelegramChat { id: -42 },
            from: Some(TelegramUser {
                id: 7,
                is_bot: false,
            }),
            text: Some("/help".to_string()),
            caption: None,
            photo: Vec::new(),
            document: None,
        };

        let inbound = TelegramInbound::from_message(9001, message).expect("inbound");
        assert_eq!(inbound.chat_id, "-42");
        assert_eq!(inbound.message_id, "33");
        assert_eq!(inbound.text, "/help");
    }

    #[test]
    fn sender_allowlist_supports_wildcard() {
        assert!(is_sender_allowed(&[], "123"));
        assert!(is_sender_allowed(&["*".to_string()], "123"));
        assert!(is_sender_allowed(&["123".to_string()], "123"));
        assert!(!is_sender_allowed(&["456".to_string()], "123"));
    }

    #[test]
    fn build_send_message_request_trims_fields() {
        let request = build_send_message_request(" 42 ", " hello ");
        assert_eq!(
            request,
            SendMessageRequest {
                chat_id: "42".to_string(),
                text: "hello".to_string(),
            }
        );
    }
}
