use super::{drain_runtime_queue, RuntimeBundle};
use klaw_channel::dingtalk::{send_session_webhook_markdown_via_proxy, DingtalkProxyConfig};
use klaw_channel::telegram::dispatch_background_outbound as dispatch_telegram_background_outbound;
use klaw_config::AppConfig;
use klaw_core::{Envelope, InboundMessage, OutboundMessage};
use klaw_cron::{CronWorker, CronWorkerConfig};
use klaw_heartbeat::{HeartbeatWorker, HeartbeatWorkerConfig};
use klaw_storage::DefaultSessionStore;
use std::{
    collections::BTreeMap,
    sync::{mpsc, Mutex},
    thread,
    time::Duration,
};
use tokio::time::timeout;
use tracing::warn;

pub type StdioCronWorker =
    CronWorker<DefaultSessionStore, klaw_core::InMemoryTransport<InboundMessage>>;
pub type StdioHeartbeatWorker =
    HeartbeatWorker<DefaultSessionStore, klaw_core::InMemoryTransport<InboundMessage>>;
const OUTBOUND_DISPATCH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct BackgroundServiceConfig {
    pub cron_tick_interval: Duration,
    pub runtime_tick_interval: Duration,
    pub runtime_drain_batch: usize,
    pub cron_batch_limit: i64,
    pub dingtalk_titles: BTreeMap<String, String>,
    pub dingtalk_proxies: BTreeMap<String, DingtalkProxyConfig>,
    pub telegram_configs: BTreeMap<String, klaw_config::TelegramConfig>,
}

impl BackgroundServiceConfig {
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            cron_tick_interval: Duration::from_millis(config.cron.tick_ms),
            runtime_tick_interval: Duration::from_millis(config.cron.runtime_tick_ms),
            runtime_drain_batch: config.cron.runtime_drain_batch,
            cron_batch_limit: config.cron.batch_limit,
            dingtalk_titles: config
                .channels
                .dingtalk
                .iter()
                .map(|cfg| (cfg.id.clone(), cfg.bot_title.clone()))
                .collect(),
            dingtalk_proxies: config
                .channels
                .dingtalk
                .iter()
                .map(|cfg| {
                    (
                        cfg.id.clone(),
                        DingtalkProxyConfig {
                            enabled: cfg.proxy.enabled,
                            url: cfg.proxy.url.clone(),
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
            dingtalk_titles: BTreeMap::new(),
            dingtalk_proxies: BTreeMap::new(),
            telegram_configs: BTreeMap::new(),
        }
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
        let outbound_dispatch_tx = spawn_outbound_dispatcher(config.clone());
        let cron_worker = CronWorker::new(
            std::sync::Arc::new(runtime.session_store.clone()),
            std::sync::Arc::new(runtime.inbound_transport.clone()),
            CronWorkerConfig {
                poll_interval: Duration::from_secs(1),
                batch_limit: config.cron_batch_limit,
            },
        );
        let heartbeat_worker = HeartbeatWorker::new(
            std::sync::Arc::new(runtime.session_store.clone()),
            std::sync::Arc::new(runtime.inbound_transport.clone()),
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
                if let Err(err) = runtime.block_on(dispatch_outbound_message(&msg, &config)) {
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

    match msg.payload.channel.as_str() {
        "dingtalk" => dispatch_dingtalk_outbound_message(msg, config).await,
        "telegram" => dispatch_telegram_outbound_message(msg, config).await,
        _ => Ok(()),
    }
}

async fn dispatch_dingtalk_outbound_message(
    msg: &Envelope<OutboundMessage>,
    config: &BackgroundServiceConfig,
) -> Result<(), String> {
    let Some(session_webhook) = msg
        .payload
        .metadata
        .get("channel.dingtalk.session_webhook")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };

    let bot_title = msg
        .payload
        .metadata
        .get("channel.dingtalk.bot_title")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            infer_account_id(&msg.header.session_key, "dingtalk")
                .and_then(|account_id| config.dingtalk_titles.get(account_id).cloned())
        })
        .unwrap_or_else(|| "Klaw".to_string());

    let proxy = infer_account_id(&msg.header.session_key, "dingtalk")
        .and_then(|account_id| config.dingtalk_proxies.get(account_id))
        .cloned()
        .unwrap_or_default();

    let body = render_outbound_markdown(&msg.payload);
    timeout(
        OUTBOUND_DISPATCH_TIMEOUT,
        send_session_webhook_markdown_via_proxy(&proxy, session_webhook, &bot_title, &body),
    )
    .await
    .map_err(|_| {
        format!(
            "dingtalk outbound delivery timed out after {}s",
            OUTBOUND_DISPATCH_TIMEOUT.as_secs()
        )
    })?
    .map_err(|err| err.to_string())
}

async fn dispatch_telegram_outbound_message(
    msg: &Envelope<OutboundMessage>,
    config: &BackgroundServiceConfig,
) -> Result<(), String> {
    let Some(account_id) = infer_account_id(&msg.header.session_key, "telegram") else {
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
