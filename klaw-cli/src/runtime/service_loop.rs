use super::{drain_runtime_queue, RuntimeBundle};
use klaw_channel::dingtalk::{send_session_webhook_markdown_via_proxy, DingtalkProxyConfig};
use klaw_config::AppConfig;
use klaw_core::{InboundMessage, OutboundMessage};
use klaw_cron::{CronWorker, CronWorkerConfig};
use klaw_storage::DefaultSessionStore;
use std::{collections::BTreeMap, sync::Mutex, time::Duration};
use tracing::warn;

pub type StdioCronWorker =
    CronWorker<DefaultSessionStore, klaw_core::InMemoryTransport<InboundMessage>>;

#[derive(Debug, Clone)]
pub struct BackgroundServiceConfig {
    pub cron_tick_interval: Duration,
    pub runtime_tick_interval: Duration,
    pub runtime_drain_batch: usize,
    pub cron_batch_limit: i64,
    pub dingtalk_titles: BTreeMap<String, String>,
    pub dingtalk_proxies: BTreeMap<String, DingtalkProxyConfig>,
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
        }
    }
}

pub struct BackgroundServices {
    cron_worker: StdioCronWorker,
    config: BackgroundServiceConfig,
    runtime_drain_error: Mutex<Option<String>>,
    dispatched_outbound_count: Mutex<usize>,
}

impl BackgroundServices {
    pub fn new(runtime: &RuntimeBundle, config: BackgroundServiceConfig) -> Self {
        let cron_worker = CronWorker::new(
            std::sync::Arc::new(runtime.session_store.clone()),
            std::sync::Arc::new(runtime.inbound_transport.clone()),
            CronWorkerConfig {
                poll_interval: Duration::from_secs(1),
                batch_limit: config.cron_batch_limit,
            },
        );
        Self {
            cron_worker,
            config,
            runtime_drain_error: Mutex::new(None),
            dispatched_outbound_count: Mutex::new(0),
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
    }

    pub async fn run_cron_now(&self, cron_id: &str) -> Result<String, String> {
        self.cron_worker
            .run_job_now(cron_id)
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
            dispatch_outbound_message(msg, &self.config).await?;
        }

        let mut guard = self
            .dispatched_outbound_count
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = messages.len();
        Ok(())
    }
}

async fn dispatch_outbound_message(
    msg: &klaw_core::Envelope<OutboundMessage>,
    config: &BackgroundServiceConfig,
) -> Result<(), String> {
    if msg.payload.channel != "dingtalk" {
        return Ok(());
    }
    if msg
        .payload
        .metadata
        .get("channel.delivery_mode")
        .and_then(|value| value.as_str())
        == Some("direct_reply")
    {
        return Ok(());
    }

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
            infer_account_id(&msg.header.session_key)
                .and_then(|account_id| config.dingtalk_titles.get(account_id).cloned())
        })
        .unwrap_or_else(|| "Klaw".to_string());

    let proxy = infer_account_id(&msg.header.session_key)
        .and_then(|account_id| config.dingtalk_proxies.get(account_id))
        .cloned()
        .unwrap_or_default();

    let body = render_outbound_markdown(&msg.payload);
    send_session_webhook_markdown_via_proxy(&proxy, session_webhook, &bot_title, &body)
        .await
        .map_err(|err| err.to_string())
}

fn infer_account_id(session_key: &str) -> Option<&str> {
    let mut parts = session_key.split(':');
    let channel = parts.next()?;
    if channel != "dingtalk" {
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
