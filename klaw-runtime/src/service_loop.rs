use super::{RuntimeBundle, drain_runtime_queue};
use klaw_channel::ChannelResult;
use klaw_channel::dingtalk::{
    DingtalkProxyConfig, is_session_webhook_session_not_found_error,
    send_proactive_markdown_via_proxy, send_session_webhook_markdown_via_proxy,
};
use klaw_channel::telegram::dispatch_background_outbound as dispatch_telegram_background_outbound;
use klaw_config::{AppConfig, CronMissedRunPolicy};
use klaw_core::{
    DeliveryMode, Envelope, InMemoryTransport, InboundMessage, MessageTransport, OutboundMessage,
    Subscription, TransportAckHandle, TransportError,
};
use klaw_cron::{CronWorker, CronWorkerConfig, MissedRunPolicy};
use klaw_gateway::{GatewayWebsocketServerFrame, OutboundEvent};
use klaw_heartbeat::{HeartbeatWorker, HeartbeatWorkerConfig, should_suppress_output};
use klaw_storage::{ChatRecord, DefaultSessionStore, SessionStorage};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io,
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
    time::SystemTime,
};
use tokio::time::timeout;
use tracing::{debug, warn};

type StdioCronWorker = CronWorker<DefaultSessionStore, FilteringInboundTransport>;
type StdioHeartbeatWorker = HeartbeatWorker<DefaultSessionStore, FilteringInboundTransport>;
const OUTBOUND_DISPATCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelAvailability {
    dingtalk_enabled: std::collections::BTreeSet<String>,
    telegram_enabled: std::collections::BTreeSet<String>,
    websocket_enabled: bool,
}

impl ChannelAvailability {
    pub(crate) fn from_app_config(config: &AppConfig) -> Self {
        Self {
            dingtalk_enabled: config
                .channels
                .dingtalk
                .iter()
                .filter(|channel| channel.enabled)
                .map(|channel| channel.id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect(),
            telegram_enabled: config
                .channels
                .telegram
                .iter()
                .filter(|channel| channel.enabled)
                .map(|channel| channel.id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect(),
            websocket_enabled: config
                .channels
                .websocket
                .iter()
                .any(|channel| channel.enabled),
        }
    }

    pub(crate) fn disabled_reason(&self, channel: &str, session_key: &str) -> Option<String> {
        match channel {
            "dingtalk" => {
                let account_id = infer_account_id(session_key, "dingtalk")?;
                (!self.dingtalk_enabled.contains(account_id))
                    .then(|| format!("target dingtalk channel '{account_id}' is disabled"))
            }
            "telegram" => {
                let account_id = infer_account_id(session_key, "telegram")?;
                (!self.telegram_enabled.contains(account_id))
                    .then(|| format!("target telegram channel '{account_id}' is disabled"))
            }
            "websocket" => (!self.websocket_enabled)
                .then(|| "target websocket channel is disabled".to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundDingtalkAccountConfig {
    pub client_id: String,
    pub client_secret: String,
    pub bot_title: String,
    pub proxy: DingtalkProxyConfig,
}

#[derive(Debug, Clone)]
pub struct BackgroundServiceConfig {
    pub cron_tick_interval: Duration,
    pub runtime_tick_interval: Duration,
    pub runtime_drain_batch: usize,
    pub cron_batch_limit: i64,
    pub cron_missed_run_policy: MissedRunPolicy,
    channel_availability: ChannelAvailability,
    pub dingtalk_accounts: BTreeMap<String, BackgroundDingtalkAccountConfig>,
    pub telegram_configs: BTreeMap<String, klaw_config::TelegramConfig>,
}

impl BackgroundServiceConfig {
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            cron_tick_interval: Duration::from_millis(config.cron.tick_ms),
            runtime_tick_interval: Duration::from_millis(config.cron.runtime_tick_ms),
            runtime_drain_batch: config.cron.runtime_drain_batch,
            cron_batch_limit: config.cron.batch_limit,
            cron_missed_run_policy: match config.cron.missed_run_policy {
                CronMissedRunPolicy::Skip => MissedRunPolicy::Skip,
                CronMissedRunPolicy::CatchUp => MissedRunPolicy::CatchUp,
            },
            channel_availability: ChannelAvailability::from_app_config(config),
            dingtalk_accounts: config
                .channels
                .dingtalk
                .iter()
                .map(|cfg| {
                    (
                        cfg.id.clone(),
                        BackgroundDingtalkAccountConfig {
                            client_id: cfg.client_id.clone(),
                            client_secret: cfg.client_secret.clone(),
                            bot_title: cfg.bot_title.clone(),
                            proxy: DingtalkProxyConfig {
                                enabled: cfg.proxy.enabled,
                                url: cfg.proxy.url.clone(),
                            },
                        },
                    )
                })
                .collect(),
            telegram_configs: config
                .channels
                .telegram
                .iter()
                .map(|cfg| (cfg.id.clone(), cfg.clone()))
                .collect(),
        }
    }
}

impl Default for BackgroundServiceConfig {
    fn default() -> Self {
        Self {
            cron_tick_interval: Duration::from_secs(1),
            runtime_tick_interval: Duration::from_millis(200),
            runtime_drain_batch: 8,
            cron_batch_limit: 64,
            cron_missed_run_policy: MissedRunPolicy::Skip,
            channel_availability: ChannelAvailability::default(),
            dingtalk_accounts: BTreeMap::new(),
            telegram_configs: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
struct FilteringInboundTransport {
    inner: InMemoryTransport<InboundMessage>,
    availability: ChannelAvailability,
}

impl FilteringInboundTransport {
    fn new(inner: InMemoryTransport<InboundMessage>, availability: ChannelAvailability) -> Self {
        Self {
            inner,
            availability,
        }
    }
}

#[async_trait::async_trait]
impl MessageTransport<InboundMessage> for FilteringInboundTransport {
    fn mode(&self) -> DeliveryMode {
        self.inner.mode()
    }

    async fn publish(
        &self,
        topic: &'static str,
        msg: Envelope<InboundMessage>,
    ) -> Result<(), TransportError> {
        if topic == klaw_core::MessageTopic::Inbound.as_str() {
            if let Some(reason) = self
                .availability
                .disabled_reason(&msg.payload.channel, &msg.payload.session_key)
            {
                debug!(
                    source = inbound_source(&msg.payload),
                    channel = msg.payload.channel.as_str(),
                    target_session_key = msg.payload.session_key.as_str(),
                    reason = %reason,
                    "skipping inbound publish because target channel is disabled"
                );
                return Ok(());
            }
        }
        self.inner.publish(topic, msg).await
    }

    async fn consume(
        &self,
        subscription: &Subscription,
    ) -> Result<klaw_core::TransportMessage<InboundMessage>, TransportError> {
        self.inner.consume(subscription).await
    }

    async fn ack(&self, handle: &TransportAckHandle) -> Result<(), TransportError> {
        self.inner.ack(handle).await
    }

    async fn nack(
        &self,
        handle: &TransportAckHandle,
        requeue_after: Option<Duration>,
    ) -> Result<(), TransportError> {
        self.inner.nack(handle, requeue_after).await
    }
}

pub struct BackgroundServices {
    cron_worker: StdioCronWorker,
    heartbeat_worker: StdioHeartbeatWorker,
    config: BackgroundServiceConfig,
    runtime_drain_error: Mutex<Option<String>>,
    dispatched_outbound_count: Mutex<usize>,
    outbound_dispatch_tx: mpsc::Sender<Envelope<OutboundMessage>>,
}

impl BackgroundServices {
    pub fn new(runtime: &RuntimeBundle, config: BackgroundServiceConfig) -> Self {
        let outbound_dispatch_tx = spawn_outbound_dispatcher(
            config.clone(),
            runtime.session_store.clone(),
            runtime.websocket_broadcaster.clone(),
        );
        let inbound_transport = FilteringInboundTransport::new(
            runtime.inbound_transport.clone(),
            config.channel_availability.clone(),
        );
        let cron_worker = CronWorker::new(
            std::sync::Arc::new(runtime.session_store.clone()),
            std::sync::Arc::new(inbound_transport.clone()),
            CronWorkerConfig {
                poll_interval: Duration::from_secs(1),
                batch_limit: config.cron_batch_limit,
                missed_run_policy: config.cron_missed_run_policy,
            },
        );
        let heartbeat_worker = HeartbeatWorker::new(
            std::sync::Arc::new(runtime.session_store.clone()),
            std::sync::Arc::new(inbound_transport),
            HeartbeatWorkerConfig {
                poll_interval: Duration::from_secs(1),
                batch_limit: config.cron_batch_limit,
            },
        );
        Self {
            cron_worker,
            heartbeat_worker,
            config,
            runtime_drain_error: Mutex::new(None),
            dispatched_outbound_count: Mutex::new(0),
            outbound_dispatch_tx,
        }
    }

    pub fn cron_tick_interval(&self) -> Duration {
        self.config.cron_tick_interval
    }

    pub fn runtime_tick_interval(&self) -> Duration {
        self.config.runtime_tick_interval
    }

    pub async fn on_cron_tick(&self) {
        if let Err(err) = self.cron_worker.run_tick().await {
            warn!(error = %err, "cron tick failed");
        }
        if let Err(err) = self.heartbeat_worker.run_tick().await {
            warn!(error = %err, "heartbeat tick failed");
        }
    }

    pub async fn run_cron_now(&self, cron_id: &str) -> Result<String, String> {
        self.cron_worker
            .run_job_now(cron_id)
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn run_heartbeat_now(&self, heartbeat_id: &str) -> Result<String, String> {
        self.heartbeat_worker
            .run_job_now(heartbeat_id)
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn on_runtime_tick(&self, runtime: &RuntimeBundle) {
        if let Err(err) = drain_runtime_queue(runtime, self.config.runtime_drain_batch).await {
            let message = err.to_string();
            let mut last_error = self
                .runtime_drain_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if last_error.as_deref() != Some(message.as_str()) {
                warn!(error = %message, "background runtime drain failed");
                *last_error = Some(message);
            }
        } else {
            let mut last_error = self
                .runtime_drain_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *last_error = None;
            if let Err(err) = self.dispatch_outbound_messages(runtime).await {
                warn!(error = %err, "background outbound dispatch failed");
            }
        }
    }

    async fn dispatch_outbound_messages(&self, runtime: &RuntimeBundle) -> Result<(), String> {
        let messages = runtime.outbound_transport.published_messages().await;
        let start = {
            let guard = self
                .dispatched_outbound_count
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard
        };

        for msg in messages.iter().skip(start) {
            if let Err(err) = self.outbound_dispatch_tx.send(msg.clone()) {
                warn!(
                    session_key = msg.header.session_key.as_str(),
                    channel = msg.payload.channel.as_str(),
                    error = %err,
                    "outbound message enqueue failed; continuing"
                );
            }
        }

        let mut guard = self
            .dispatched_outbound_count
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = messages.len();
        Ok(())
    }
}

fn spawn_outbound_dispatcher(
    config: BackgroundServiceConfig,
    session_store: DefaultSessionStore,
    websocket_broadcaster: std::sync::Arc<klaw_gateway::GatewayWebsocketBroadcaster>,
) -> mpsc::Sender<Envelope<OutboundMessage>> {
    let (tx, rx) = mpsc::channel::<Envelope<OutboundMessage>>();
    thread::Builder::new()
        .name("klaw-outbound-dispatch".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    warn!(error = %err, "failed to start outbound dispatch runtime");
                    return;
                }
            };

            for msg in rx {
                if let Err(err) = runtime.block_on(dispatch_outbound_message(
                    &msg,
                    &config,
                    &session_store,
                    websocket_broadcaster.as_ref(),
                )) {
                    warn!(
                        session_key = msg.header.session_key.as_str(),
                        channel = msg.payload.channel.as_str(),
                        error = %err,
                        "outbound message dispatch failed"
                    );
                }
            }
        })
        .expect("outbound dispatch worker should start");
    tx
}

async fn dispatch_outbound_message(
    msg: &Envelope<OutboundMessage>,
    config: &BackgroundServiceConfig,
    session_store: &DefaultSessionStore,
    websocket_broadcaster: &klaw_gateway::GatewayWebsocketBroadcaster,
) -> Result<(), String> {
    if msg
        .payload
        .metadata
        .get("channel.delivery_mode")
        .and_then(|value| value.as_str())
        == Some("direct_reply")
    {
        return Ok(());
    }
    if should_suppress_output(&msg.payload.content, &msg.payload.metadata) {
        return Ok(());
    }

    let delivery_target = resolve_outbound_delivery_target(msg, session_store).await;
    mirror_outbound_to_delivery_session(msg, delivery_target.as_ref(), session_store).await?;

    match delivery_target
        .as_ref()
        .map(|target| target.channel.as_str())
        .unwrap_or(msg.payload.channel.as_str())
    {
        "dingtalk" => dispatch_dingtalk_outbound_message(msg, config, session_store).await,
        "telegram" => dispatch_telegram_outbound_message(msg, config).await,
        "websocket" => {
            dispatch_websocket_outbound_message(
                msg,
                delivery_target.as_ref(),
                websocket_broadcaster,
            )
            .await
        }
        _ => Ok(()),
    }
}

async fn dispatch_dingtalk_outbound_message(
    msg: &Envelope<OutboundMessage>,
    config: &BackgroundServiceConfig,
    _session_store: &DefaultSessionStore,
) -> Result<(), String> {
    let account_id = resolve_outbound_account_id(msg, "dingtalk").map(str::to_string);
    let account = account_id
        .as_deref()
        .and_then(|account_id| config.dingtalk_accounts.get(account_id));
    let bot_title = msg
        .payload
        .metadata
        .get("channel.dingtalk.bot_title")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| account.map(|account| account.bot_title.clone()))
        .unwrap_or_else(|| "Klaw".to_string());
    let body = render_outbound_markdown(&msg.payload);

    let Some(session_webhook) = msg
        .payload
        .metadata
        .get("channel.dingtalk.session_webhook")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        warn!(
            account_id = account_id.as_deref().unwrap_or("unknown"),
            chat_id = msg.payload.chat_id.as_str(),
            "dingtalk outbound missing session webhook, falling back to proactive send"
        );
        return send_dingtalk_proactive_fallback(
            msg,
            account_id.as_deref(),
            account,
            &bot_title,
            &body,
        )
        .await;
    };

    match send_dingtalk_session_markdown(
        account.map(|account| &account.proxy),
        session_webhook,
        &bot_title,
        &body,
    )
    .await
    {
        Ok(()) => return Ok(()),
        Err(err) if !is_session_webhook_session_not_found_error(err.as_ref()) => {
            return Err(err.to_string());
        }
        Err(err) => {
            warn!(
                account_id = account_id.as_deref().unwrap_or("unknown"),
                chat_id = msg.payload.chat_id.as_str(),
                error = %err,
                "dingtalk outbound stale session webhook detected, falling back to proactive send"
            );
            send_dingtalk_proactive_fallback(msg, account_id.as_deref(), account, &bot_title, &body)
                .await
                .map_err(|fallback_err| format!("{err}; proactive_fallback={fallback_err}"))
        }
    }
}

async fn send_dingtalk_session_markdown(
    proxy: Option<&DingtalkProxyConfig>,
    session_webhook: &str,
    bot_title: &str,
    body: &str,
) -> ChannelResult<()> {
    let default_proxy = DingtalkProxyConfig::default();
    let proxy = proxy.unwrap_or(&default_proxy);
    timeout(
        OUTBOUND_DISPATCH_TIMEOUT,
        send_session_webhook_markdown_via_proxy(proxy, session_webhook, bot_title, body),
    )
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "dingtalk outbound delivery timed out after {}s",
                OUTBOUND_DISPATCH_TIMEOUT.as_secs()
            ),
        )
    })?
}

async fn send_dingtalk_proactive_fallback(
    msg: &Envelope<OutboundMessage>,
    account_id: Option<&str>,
    account: Option<&BackgroundDingtalkAccountConfig>,
    bot_title: &str,
    body: &str,
) -> Result<(), String> {
    let Some(account) = account else {
        let reason = match account_id {
            Some(account_id) => format!("missing dingtalk account config for '{account_id}'"),
            None => "missing dingtalk account config for proactive fallback".to_string(),
        };
        return Err(reason);
    };
    warn!(
        account_id = account_id.unwrap_or("unknown"),
        chat_id = msg.payload.chat_id.as_str(),
        "dingtalk outbound using proactive fallback"
    );
    timeout(
        OUTBOUND_DISPATCH_TIMEOUT,
        send_proactive_markdown_via_proxy(
            &account.proxy,
            &account.client_id,
            &account.client_secret,
            &msg.payload.chat_id,
            bot_title,
            body,
        ),
    )
    .await
    .map_err(|_| {
        format!(
            "dingtalk proactive fallback timed out after {}s",
            OUTBOUND_DISPATCH_TIMEOUT.as_secs()
        )
    })?
    .map_err(|err| err.to_string())
}

struct OutboundDeliveryTarget {
    channel: String,
    session_key: Option<String>,
}

async fn mirror_outbound_to_delivery_session(
    msg: &Envelope<OutboundMessage>,
    delivery_target: Option<&OutboundDeliveryTarget>,
    session_store: &DefaultSessionStore,
) -> Result<(), String> {
    let target_channel = delivery_target
        .map(|target| target.channel.as_str())
        .unwrap_or(msg.payload.channel.as_str());
    let Some(target_session_key) = delivery_target
        .and_then(|target| target.session_key.as_deref())
        .map(ToString::to_string)
    else {
        return Ok(());
    };
    if target_session_key == msg.header.session_key {
        return Ok(());
    }

    session_store
        .touch_session(&target_session_key, &msg.payload.chat_id, target_channel)
        .await
        .map_err(|err| err.to_string())?;
    session_store
        .append_chat_record(
            &target_session_key,
            &ChatRecord::new(
                "assistant",
                msg.payload.content.clone(),
                Some(msg.header.message_id.to_string()),
            )
            .with_metadata_json(
                (!msg.payload.metadata.is_empty())
                    .then(|| serde_json::to_string(&msg.payload.metadata).ok())
                    .flatten(),
            ),
        )
        .await
        .map_err(|err| err.to_string())
}

fn resolve_delivery_session_key(metadata: &BTreeMap<String, Value>) -> Option<String> {
    ["channel.delivery_session_key", "channel.base_session_key"]
        .into_iter()
        .find_map(|key| {
            metadata
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
}

async fn resolve_outbound_delivery_target(
    msg: &Envelope<OutboundMessage>,
    session_store: &DefaultSessionStore,
) -> Option<OutboundDeliveryTarget> {
    if let Some(target_session_key) = resolve_delivery_session_key(&msg.payload.metadata) {
        let channel = session_store
            .get_session(&target_session_key)
            .await
            .ok()
            .map(|session| session.channel)
            .unwrap_or_else(|| msg.payload.channel.clone());
        return Some(OutboundDeliveryTarget {
            channel,
            session_key: Some(target_session_key),
        });
    }

    match msg.payload.channel.as_str() {
        "websocket" | "terminal" => Some(OutboundDeliveryTarget {
            channel: msg.payload.channel.clone(),
            session_key: Some(msg.header.session_key.clone()),
        }),
        _ => None,
    }
}

async fn dispatch_websocket_outbound_message(
    msg: &Envelope<OutboundMessage>,
    delivery_target: Option<&OutboundDeliveryTarget>,
    websocket_broadcaster: &klaw_gateway::GatewayWebsocketBroadcaster,
) -> Result<(), String> {
    let Some(target_session_key) = delivery_target
        .and_then(|target| target.session_key.as_deref())
        .map(ToString::to_string)
    else {
        return Ok(());
    };

    let delivered = websocket_broadcaster
        .broadcast_to_session(
            &target_session_key,
            GatewayWebsocketServerFrame::Event {
                event: OutboundEvent::SessionMessage,
                payload: serde_json::json!({
                    "message_id": msg.header.message_id.to_string(),
                    "session_key": target_session_key,
                    "response": {
                        "content": msg.payload.content,
                        "metadata": msg.payload.metadata,
                    },
                    "role": "assistant",
                    "timestamp_ms": SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64,
                }),
            },
        )
        .await;
    if delivered == 0 {
        return Ok(());
    }
    Ok(())
}

async fn dispatch_telegram_outbound_message(
    msg: &Envelope<OutboundMessage>,
    config: &BackgroundServiceConfig,
) -> Result<(), String> {
    let Some(account_id) = resolve_outbound_account_id(msg, "telegram") else {
        return Ok(());
    };
    let Some(telegram_config) = config.telegram_configs.get(account_id) else {
        return Ok(());
    };
    timeout(
        OUTBOUND_DISPATCH_TIMEOUT,
        dispatch_telegram_background_outbound(telegram_config, &msg.payload),
    )
    .await
    .map_err(|_| {
        format!(
            "telegram outbound delivery timed out after {}s",
            OUTBOUND_DISPATCH_TIMEOUT.as_secs()
        )
    })?
    .map_err(|err| err.to_string())
}

fn infer_account_id<'a>(session_key: &'a str, expected_channel: &str) -> Option<&'a str> {
    let mut parts = session_key.split(':');
    let channel = parts.next()?;
    if channel != expected_channel {
        return None;
    }
    parts.next()
}

fn inbound_source(payload: &InboundMessage) -> &'static str {
    if payload.metadata.contains_key("cron_id") {
        "cron"
    } else if payload.metadata.get("trigger.kind").and_then(Value::as_str) == Some("heartbeat") {
        "heartbeat"
    } else {
        "background"
    }
}

fn resolve_outbound_account_id<'a>(
    msg: &'a Envelope<OutboundMessage>,
    expected_channel: &str,
) -> Option<&'a str> {
    resolve_outbound_channel_session_key(msg, expected_channel)
        .and_then(|session_key| infer_account_id(session_key, expected_channel))
}

fn resolve_outbound_channel_session_key<'a>(
    msg: &'a Envelope<OutboundMessage>,
    expected_channel: &str,
) -> Option<&'a str> {
    let header_session_key = msg.header.session_key.as_str();
    if infer_account_id(header_session_key, expected_channel).is_some() {
        return Some(header_session_key);
    }

    ["channel.delivery_session_key", "channel.base_session_key"]
        .into_iter()
        .find_map(|key| {
            msg.payload
                .metadata
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .filter(|value| infer_account_id(value, expected_channel).is_some())
        })
}

fn render_outbound_markdown(output: &OutboundMessage) -> String {
    match output
        .metadata
        .get("reasoning")
        .and_then(|value| value.as_str())
    {
        Some(reasoning) if !reasoning.trim().is_empty() => {
            format!("{}\n\n---\n\n> reasoning\n{}\n", output.content, reasoning)
        }
        _ => output.content.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BackgroundDingtalkAccountConfig, BackgroundServiceConfig, ChannelAvailability,
        FilteringInboundTransport, dispatch_outbound_message, resolve_outbound_account_id,
    };
    use klaw_channel::dingtalk::is_session_webhook_session_not_found_error;
    use klaw_config::{AppConfig, DingtalkConfig};
    use klaw_core::{
        Envelope, EnvelopeHeader, InMemoryTransport, InboundMessage, MessageTopic,
        MessageTransport, OutboundMessage,
    };
    use klaw_gateway::GatewayWebsocketBroadcaster;
    use klaw_session::{SessionManager, SqliteSessionManager};
    use klaw_storage::{DefaultSessionStore, StoragePaths};
    use serde_json::{Value, json};
    use std::{collections::BTreeMap, path::PathBuf};
    use uuid::Uuid;

    async fn create_store() -> DefaultSessionStore {
        let root = PathBuf::from(std::env::temp_dir())
            .join(format!("klaw-service-loop-{}", Uuid::new_v4()));
        DefaultSessionStore::open(StoragePaths::from_root(root))
            .await
            .expect("store should open")
    }

    fn outbound_message(
        header_session_key: &str,
        channel: &str,
        metadata: BTreeMap<String, Value>,
    ) -> Envelope<OutboundMessage> {
        Envelope {
            header: EnvelopeHeader::new(header_session_key),
            metadata: BTreeMap::new(),
            payload: OutboundMessage {
                channel: channel.to_string(),
                chat_id: "chat-1".to_string(),
                content: "hello".to_string(),
                reply_to: None,
                metadata,
            },
        }
    }

    #[test]
    fn resolve_outbound_account_id_prefers_header_session_key() {
        let msg = outbound_message(
            "telegram:acc-header:chat-1",
            "telegram",
            BTreeMap::from([(
                "channel.delivery_session_key".to_string(),
                json!("telegram:acc-meta:chat-1"),
            )]),
        );

        assert_eq!(
            resolve_outbound_account_id(&msg, "telegram"),
            Some("acc-header")
        );
    }

    #[test]
    fn resolve_outbound_account_id_falls_back_to_delivery_session_key() {
        let msg = outbound_message(
            "cron:job-1:run-1",
            "telegram",
            BTreeMap::from([(
                "channel.delivery_session_key".to_string(),
                json!("telegram:acc-delivery:chat-1:child"),
            )]),
        );

        assert_eq!(
            resolve_outbound_account_id(&msg, "telegram"),
            Some("acc-delivery")
        );
    }

    #[test]
    fn resolve_outbound_account_id_falls_back_to_base_session_key() {
        let msg = outbound_message(
            "cron:job-1:run-1",
            "telegram",
            BTreeMap::from([(
                "channel.base_session_key".to_string(),
                json!("telegram:acc-base:chat-1"),
            )]),
        );

        assert_eq!(
            resolve_outbound_account_id(&msg, "telegram"),
            Some("acc-base")
        );
    }

    #[test]
    fn resolve_outbound_account_id_rejects_wrong_channel_metadata() {
        let msg = outbound_message(
            "cron:job-1:run-1",
            "telegram",
            BTreeMap::from([(
                "channel.delivery_session_key".to_string(),
                json!("dingtalk:acc-delivery:chat-1"),
            )]),
        );

        assert_eq!(resolve_outbound_account_id(&msg, "telegram"), None);
    }

    #[test]
    fn background_service_config_collects_dingtalk_accounts() {
        let config = BackgroundServiceConfig::from_app_config(&AppConfig {
            channels: klaw_config::ChannelsConfig {
                dingtalk: vec![DingtalkConfig {
                    id: "acc-1".to_string(),
                    client_id: "client-1".to_string(),
                    client_secret: "secret-1".to_string(),
                    bot_title: "Ops Bot".to_string(),
                    proxy: klaw_config::DingtalkProxyConfig {
                        enabled: true,
                        url: "http://127.0.0.1:8080".to_string(),
                    },
                    ..DingtalkConfig::default()
                }],
                ..klaw_config::ChannelsConfig::default()
            },
            ..AppConfig::default()
        });

        assert_eq!(
            config.dingtalk_accounts.get("acc-1"),
            Some(&BackgroundDingtalkAccountConfig {
                client_id: "client-1".to_string(),
                client_secret: "secret-1".to_string(),
                bot_title: "Ops Bot".to_string(),
                proxy: klaw_channel::dingtalk::DingtalkProxyConfig {
                    enabled: true,
                    url: "http://127.0.0.1:8080".to_string(),
                },
            })
        );
    }

    #[test]
    fn detects_dingtalk_session_not_found_error() {
        let not_found_err = klaw_channel::dingtalk::DingtalkApiError::SessionWebhookBusiness {
            context: "markdown send".to_string(),
            errcode: 300001,
            errmsg: "session 不存在".to_string(),
            body: json!({"errcode": 300001, "errmsg": "session 不存在"}),
        };
        let other_err = klaw_channel::dingtalk::DingtalkApiError::SessionWebhookBusiness {
            context: "markdown send".to_string(),
            errcode: 40035,
            errmsg: "invalid".to_string(),
            body: json!({"errcode": 40035, "errmsg": "invalid"}),
        };

        assert!(is_session_webhook_session_not_found_error(&not_found_err));
        assert!(!is_session_webhook_session_not_found_error(&other_err));
    }

    #[test]
    fn channel_availability_detects_disabled_dingtalk_target() {
        let availability = ChannelAvailability::from_app_config(&AppConfig {
            channels: klaw_config::ChannelsConfig {
                dingtalk: vec![DingtalkConfig {
                    id: "acc-enabled".to_string(),
                    enabled: true,
                    ..DingtalkConfig::default()
                }],
                ..klaw_config::ChannelsConfig::default()
            },
            ..AppConfig::default()
        });

        assert_eq!(
            availability.disabled_reason("dingtalk", "dingtalk:acc-disabled:chat-1"),
            Some("target dingtalk channel 'acc-disabled' is disabled".to_string())
        );
        assert_eq!(
            availability.disabled_reason("dingtalk", "dingtalk:acc-enabled:chat-1"),
            None
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn filtering_inbound_transport_skips_disabled_target_channel() {
        let availability = ChannelAvailability::from_app_config(&AppConfig {
            channels: klaw_config::ChannelsConfig {
                telegram: vec![klaw_config::TelegramConfig {
                    id: "bot-enabled".to_string(),
                    enabled: true,
                    ..klaw_config::TelegramConfig::default()
                }],
                ..klaw_config::ChannelsConfig::default()
            },
            ..AppConfig::default()
        });
        let inner = InMemoryTransport::new();
        let transport = FilteringInboundTransport::new(inner.clone(), availability);
        let envelope = Envelope {
            header: EnvelopeHeader::new("cron:job-1:run-1"),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "telegram".to_string(),
                sender_id: "system-cron".to_string(),
                chat_id: "chat-1".to_string(),
                session_key: "telegram:bot-disabled:chat-1".to_string(),
                content: "ping".to_string(),
                metadata: BTreeMap::from([("cron_id".to_string(), json!("job-1"))]),
                media_references: Vec::new(),
            },
        };

        transport
            .publish(MessageTopic::Inbound.as_str(), envelope)
            .await
            .expect("publish should succeed");

        assert!(inner.published_messages().await.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dispatch_outbound_message_skips_heartbeat_silent_ack_for_websocket() {
        let store = create_store().await;
        let manager = SqliteSessionManager::from_store(store.clone());
        manager
            .get_or_create_session_state(
                "websocket:test",
                "chat-1",
                "websocket",
                "provider",
                "model",
            )
            .await
            .expect("session should exist");

        let broadcaster = GatewayWebsocketBroadcaster::new();
        let (frame_tx, mut frame_rx) = tokio::sync::mpsc::unbounded_channel();
        broadcaster
            .register(
                "conn-1".to_string(),
                Some("websocket:test".to_string()),
                frame_tx,
            )
            .await;

        let msg = Envelope {
            header: EnvelopeHeader::new("websocket:test"),
            metadata: BTreeMap::new(),
            payload: OutboundMessage {
                channel: "websocket".to_string(),
                chat_id: "chat-1".to_string(),
                content: " HEARTBEAT_OK ".to_string(),
                reply_to: None,
                metadata: BTreeMap::from([
                    ("trigger.kind".to_string(), json!("heartbeat")),
                    (
                        "heartbeat.silent_ack_token".to_string(),
                        json!("HEARTBEAT_OK"),
                    ),
                ]),
            },
        };

        dispatch_outbound_message(
            &msg,
            &BackgroundServiceConfig::default(),
            &store,
            &broadcaster,
        )
        .await
        .expect("dispatch should succeed");

        assert!(frame_rx.try_recv().is_err());
        assert!(
            manager
                .read_chat_records("websocket:test")
                .await
                .expect("history should load")
                .is_empty()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dispatch_outbound_message_websocket_frame_includes_metadata_and_message_id() {
        let store = create_store().await;
        let manager = SqliteSessionManager::from_store(store.clone());
        manager
            .get_or_create_session_state(
                "websocket:test",
                "chat-1",
                "websocket",
                "provider",
                "model",
            )
            .await
            .expect("session should exist");

        let broadcaster = GatewayWebsocketBroadcaster::new();
        let (frame_tx, mut frame_rx) = tokio::sync::mpsc::unbounded_channel();
        broadcaster
            .register(
                "conn-1".to_string(),
                Some("websocket:test".to_string()),
                frame_tx,
            )
            .await;

        let msg = Envelope {
            header: EnvelopeHeader::new("websocket:test"),
            metadata: BTreeMap::new(),
            payload: OutboundMessage {
                channel: "websocket".to_string(),
                chat_id: "chat-1".to_string(),
                content: "Need action".to_string(),
                reply_to: None,
                metadata: BTreeMap::from([
                    ("trigger.kind".to_string(), json!("heartbeat")),
                    ("custom.flag".to_string(), json!(true)),
                ]),
            },
        };
        let expected_message_id = msg.header.message_id.to_string();

        dispatch_outbound_message(
            &msg,
            &BackgroundServiceConfig::default(),
            &store,
            &broadcaster,
        )
        .await
        .expect("dispatch should succeed");

        let frame = frame_rx.try_recv().expect("frame should be broadcast");
        let payload = match frame {
            klaw_gateway::GatewayWebsocketServerFrame::Event { event, payload } => {
                assert_eq!(event, klaw_gateway::OutboundEvent::SessionMessage);
                payload
            }
            other => panic!("unexpected frame: {other:?}"),
        };

        assert_eq!(
            payload.get("message_id").and_then(Value::as_str),
            Some(expected_message_id.as_str())
        );
        assert_eq!(
            payload
                .get("response")
                .and_then(|response| response.get("content"))
                .and_then(Value::as_str),
            Some("Need action")
        );
        assert_eq!(
            payload
                .get("response")
                .and_then(|response| response.get("metadata"))
                .and_then(Value::as_object)
                .and_then(|metadata| metadata.get("custom.flag"))
                .and_then(Value::as_bool),
            Some(true)
        );
    }
}
