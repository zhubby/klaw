use crate::{ChannelResult, ChannelRuntime, dingtalk::DingtalkChannel, telegram::TelegramChannel};
use klaw_config::{ChannelsConfig, DingtalkConfig, LocalAttachmentConfig, TelegramConfig};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex},
};
use tokio::{sync::watch, task::JoinHandle, time};
use tracing::{info, warn};

const CHANNEL_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChannelKind {
    Dingtalk,
    Telegram,
    Feishu,
}

impl ChannelKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dingtalk => "dingtalk",
            Self::Telegram => "telegram",
            Self::Feishu => "feishu",
        }
    }
}

impl fmt::Display for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChannelInstanceKey(String);

impl ChannelInstanceKey {
    #[must_use]
    pub fn new(kind: ChannelKind, id: impl AsRef<str>) -> Self {
        Self(format!("{}:{}", kind.as_str(), id.as_ref().trim()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChannelInstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelInstanceConfig {
    Dingtalk(DingtalkConfig),
    Telegram(TelegramConfig),
}

impl ChannelInstanceConfig {
    #[must_use]
    pub fn kind(&self) -> ChannelKind {
        match self {
            Self::Dingtalk(_) => ChannelKind::Dingtalk,
            Self::Telegram(_) => ChannelKind::Telegram,
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Dingtalk(config) => &config.id,
            Self::Telegram(config) => &config.id,
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        match self {
            Self::Dingtalk(config) => config.enabled,
            Self::Telegram(config) => config.enabled,
        }
    }

    #[must_use]
    pub fn key(&self) -> ChannelInstanceKey {
        ChannelInstanceKey::new(self.kind(), self.id())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelConfigSnapshot {
    instances: Vec<ChannelInstanceConfig>,
}

impl ChannelConfigSnapshot {
    pub fn from_channels_config(channels: &ChannelsConfig) -> Result<Self, String> {
        let mut keys = BTreeSet::new();
        let mut instances = Vec::new();

        for config in &channels.dingtalk {
            let instance = ChannelInstanceConfig::Dingtalk(config.clone());
            let key = instance.key();
            if !keys.insert(key.clone()) {
                return Err(format!("duplicated channel instance '{}'", key.as_str()));
            }
            instances.push(instance);
        }
        for config in &channels.telegram {
            let instance = ChannelInstanceConfig::Telegram(config.clone());
            let key = instance.key();
            if !keys.insert(key.clone()) {
                return Err(format!("duplicated channel instance '{}'", key.as_str()));
            }
            instances.push(instance);
        }

        Ok(Self { instances })
    }

    #[must_use]
    pub fn instances(&self) -> &[ChannelInstanceConfig] {
        &self.instances
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelLifecycleState {
    Starting,
    Running,
    Stopped,
    Failed,
}

impl ChannelLifecycleState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInstanceStatus {
    pub key: ChannelInstanceKey,
    pub kind: ChannelKind,
    pub id: String,
    pub enabled: bool,
    pub state: ChannelLifecycleState,
    pub last_error: Option<String>,
}

impl ChannelInstanceStatus {
    #[must_use]
    pub fn from_config(
        config: &ChannelInstanceConfig,
        state: ChannelLifecycleState,
        last_error: Option<String>,
    ) -> Self {
        Self {
            key: config.key(),
            kind: config.kind(),
            id: config.id().to_string(),
            enabled: config.enabled(),
            state,
            last_error,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelSyncResult {
    pub keep: Vec<ChannelInstanceKey>,
    pub start: Vec<ChannelInstanceKey>,
    pub restart: Vec<ChannelInstanceKey>,
    pub stop: Vec<ChannelInstanceKey>,
    pub statuses: Vec<ChannelInstanceStatus>,
}

#[async_trait::async_trait(?Send)]
pub trait ManagedChannelDriver {
    fn kind(&self) -> ChannelKind;

    fn instance_id(&self) -> &str;

    async fn run_until_shutdown(
        &mut self,
        runtime: &dyn ChannelRuntime,
        shutdown: &mut watch::Receiver<bool>,
    ) -> ChannelResult<()>;
}

pub trait ChannelDriverFactory {
    fn build(&self, config: &ChannelInstanceConfig)
    -> ChannelResult<Box<dyn ManagedChannelDriver>>;
}

#[derive(Debug, Default, Clone)]
pub struct DefaultChannelDriverFactory {
    local_attachments: LocalAttachmentConfig,
}

impl DefaultChannelDriverFactory {
    #[must_use]
    pub fn new(local_attachments: LocalAttachmentConfig) -> Self {
        Self { local_attachments }
    }
}

impl ChannelDriverFactory for DefaultChannelDriverFactory {
    fn build(
        &self,
        config: &ChannelInstanceConfig,
    ) -> ChannelResult<Box<dyn ManagedChannelDriver>> {
        match config {
            ChannelInstanceConfig::Dingtalk(config) => {
                Ok(Box::new(DingtalkChannel::from_app_config(
                    config.clone(),
                    self.local_attachments.clone(),
                )?))
            }
            ChannelInstanceConfig::Telegram(config) => {
                Ok(Box::new(TelegramChannel::from_app_config(
                    config.clone(),
                    self.local_attachments.clone(),
                )?))
            }
        }
    }
}

struct ManagedChannelHandle {
    config: ChannelInstanceConfig,
    shutdown_tx: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

pub struct ChannelManager<R, F = DefaultChannelDriverFactory>
where
    R: ChannelRuntime + 'static,
    F: ChannelDriverFactory + 'static,
{
    runtime: Arc<R>,
    factory: F,
    channels: BTreeMap<ChannelInstanceKey, ManagedChannelHandle>,
    statuses: Arc<Mutex<BTreeMap<ChannelInstanceKey, ChannelInstanceStatus>>>,
}

impl<R> ChannelManager<R, DefaultChannelDriverFactory>
where
    R: ChannelRuntime + 'static,
{
    #[must_use]
    pub fn new(runtime: Arc<R>) -> Self {
        Self::with_factory(runtime, DefaultChannelDriverFactory::default())
    }
}

impl<R, F> ChannelManager<R, F>
where
    R: ChannelRuntime + 'static,
    F: ChannelDriverFactory + 'static,
{
    #[must_use]
    pub fn with_factory(runtime: Arc<R>, factory: F) -> Self {
        Self {
            runtime,
            factory,
            channels: BTreeMap::new(),
            statuses: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub async fn sync(&mut self, snapshot: ChannelConfigSnapshot) -> ChannelSyncResult {
        let current = self
            .channels
            .iter()
            .map(|(key, channel)| (key.clone(), channel.config.clone()))
            .collect::<BTreeMap<_, _>>();
        let plan = plan_channel_updates(&current, snapshot.instances());

        for key in &plan.stop {
            self.stop_channel(key, false).await;
        }

        for config in &plan.restart {
            self.stop_channel(&config.key(), false).await;
        }

        for config in plan.start.iter().chain(plan.restart.iter()) {
            self.start_channel(config.clone());
        }

        self.reconcile_statuses(&snapshot);

        ChannelSyncResult {
            keep: plan.keep,
            start: plan.start.into_iter().map(|config| config.key()).collect(),
            restart: plan
                .restart
                .into_iter()
                .map(|config| config.key())
                .collect(),
            stop: plan.stop,
            statuses: self.snapshot_statuses(snapshot.instances()),
        }
    }

    pub async fn shutdown_all(&mut self) {
        let keys = self.channels.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            self.stop_channel(&key, true).await;
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<ChannelInstanceStatus> {
        self.statuses
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .values()
            .cloned()
            .collect()
    }

    fn start_channel(&mut self, config: ChannelInstanceConfig) {
        let key = config.key();
        let key_for_task = key.clone();
        let runtime = Arc::clone(&self.runtime);
        let statuses = Arc::clone(&self.statuses);
        let config_for_task = config.clone();
        let (shutdown_tx, mut channel_shutdown) = watch::channel(false);

        {
            let mut guard = statuses.lock().unwrap_or_else(|err| err.into_inner());
            guard.insert(
                key.clone(),
                ChannelInstanceStatus::from_config(&config, ChannelLifecycleState::Starting, None),
            );
        }

        let mut driver = match self.factory.build(&config) {
            Ok(driver) => driver,
            Err(err) => {
                let message = err.to_string();
                let mut guard = statuses
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.insert(
                    key,
                    ChannelInstanceStatus::from_config(
                        &config,
                        ChannelLifecycleState::Failed,
                        Some(message),
                    ),
                );
                return;
            }
        };

        {
            let mut guard = statuses.lock().unwrap_or_else(|err| err.into_inner());
            guard.insert(
                key.clone(),
                ChannelInstanceStatus::from_config(&config, ChannelLifecycleState::Running, None),
            );
        }

        let handle = tokio::task::spawn_local(async move {
            info!(
                channel_kind = driver.kind().as_str(),
                channel_id = driver.instance_id(),
                instance_key = key_for_task.as_str(),
                "starting managed channel"
            );

            let result = driver
                .run_until_shutdown(runtime.as_ref(), &mut channel_shutdown)
                .await;
            let stopping = *channel_shutdown.borrow();

            let next_status = match result {
                Ok(()) => ChannelInstanceStatus::from_config(
                    &config_for_task,
                    ChannelLifecycleState::Stopped,
                    None,
                ),
                Err(err) => {
                    warn!(
                        instance_key = key_for_task.as_str(),
                        error = %err,
                        "managed channel stopped with error"
                    );
                    ChannelInstanceStatus::from_config(
                        &config_for_task,
                        ChannelLifecycleState::Failed,
                        Some(err.to_string()),
                    )
                }
            };

            let mut guard = statuses.lock().unwrap_or_else(|err| err.into_inner());
            if stopping || matches!(next_status.state, ChannelLifecycleState::Failed) {
                guard.insert(key_for_task, next_status);
            }
        });

        self.channels.insert(
            key,
            ManagedChannelHandle {
                config,
                shutdown_tx,
                handle,
            },
        );
    }

    async fn stop_channel(&mut self, key: &ChannelInstanceKey, shutdown_all: bool) {
        let Some(managed) = self.channels.remove(key) else {
            return;
        };
        let _ = managed.shutdown_tx.send(true);
        if let Err(err) = time::timeout(CHANNEL_SHUTDOWN_TIMEOUT, managed.handle).await {
            let mut guard = self
                .statuses
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.insert(
                key.clone(),
                ChannelInstanceStatus::from_config(
                    &managed.config,
                    ChannelLifecycleState::Failed,
                    Some(format!("timed out waiting channel shutdown: {err}")),
                ),
            );
            return;
        }

        let mut guard = self
            .statuses
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(
            key.clone(),
            ChannelInstanceStatus::from_config(
                &managed.config,
                ChannelLifecycleState::Stopped,
                None,
            ),
        );
        if shutdown_all {
            return;
        }
    }

    fn reconcile_statuses(&mut self, snapshot: &ChannelConfigSnapshot) {
        let desired_keys = snapshot
            .instances()
            .iter()
            .map(ChannelInstanceConfig::key)
            .collect::<BTreeSet<_>>();
        let mut guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        guard.retain(|key, _| desired_keys.contains(key));

        for config in snapshot.instances() {
            let key = config.key();
            match guard.get_mut(&key) {
                Some(status) => {
                    status.enabled = config.enabled();
                    status.kind = config.kind();
                    status.id = config.id().to_string();
                    if !config.enabled() {
                        status.state = ChannelLifecycleState::Stopped;
                        status.last_error = None;
                    }
                }
                None => {
                    let state = if config.enabled() {
                        ChannelLifecycleState::Stopped
                    } else {
                        ChannelLifecycleState::Stopped
                    };
                    guard.insert(key, ChannelInstanceStatus::from_config(config, state, None));
                }
            }
        }
    }

    fn snapshot_statuses(&self, instances: &[ChannelInstanceConfig]) -> Vec<ChannelInstanceStatus> {
        let guard = self.statuses.lock().unwrap_or_else(|err| err.into_inner());
        instances
            .iter()
            .map(|config| {
                guard.get(&config.key()).cloned().unwrap_or_else(|| {
                    ChannelInstanceStatus::from_config(config, ChannelLifecycleState::Stopped, None)
                })
            })
            .collect()
    }
}

#[derive(Debug, Default)]
struct ChannelSyncPlan {
    keep: Vec<ChannelInstanceKey>,
    start: Vec<ChannelInstanceConfig>,
    restart: Vec<ChannelInstanceConfig>,
    stop: Vec<ChannelInstanceKey>,
}

fn plan_channel_updates(
    current: &BTreeMap<ChannelInstanceKey, ChannelInstanceConfig>,
    desired: &[ChannelInstanceConfig],
) -> ChannelSyncPlan {
    let desired_enabled = desired
        .iter()
        .filter(|config| config.enabled())
        .cloned()
        .map(|config| (config.key(), config))
        .collect::<BTreeMap<_, _>>();

    let mut plan = ChannelSyncPlan::default();

    for key in current.keys() {
        if !desired_enabled.contains_key(key) {
            plan.stop.push(key.clone());
        }
    }

    for (key, desired_config) in desired_enabled {
        match current.get(&key) {
            Some(current_config) if current_config == &desired_config => {
                plan.keep.push(key);
            }
            Some(_) => {
                plan.restart.push(desired_config);
            }
            None => {
                plan.start.push(desired_config);
            }
        }
    }

    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        sync::atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct DummyRuntime;

    #[async_trait::async_trait(?Send)]
    impl ChannelRuntime for DummyRuntime {
        async fn submit(
            &self,
            _request: crate::ChannelRequest,
        ) -> ChannelResult<Option<crate::ChannelResponse>> {
            Ok(None)
        }

        fn cron_tick_interval(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1)
        }

        fn runtime_tick_interval(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1)
        }

        async fn on_cron_tick(&self) {}

        async fn on_runtime_tick(&self) {}
    }

    struct RecordingDriver {
        kind: ChannelKind,
        id: String,
        shutdowns: Arc<AtomicUsize>,
        fail_on_run: bool,
    }

    #[async_trait::async_trait(?Send)]
    impl ManagedChannelDriver for RecordingDriver {
        fn kind(&self) -> ChannelKind {
            self.kind
        }

        fn instance_id(&self) -> &str {
            &self.id
        }

        async fn run_until_shutdown(
            &mut self,
            _runtime: &dyn ChannelRuntime,
            shutdown: &mut watch::Receiver<bool>,
        ) -> ChannelResult<()> {
            shutdown
                .changed()
                .await
                .map_err(|err| io::Error::other(err.to_string()))?;
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
            if self.fail_on_run {
                Err(io::Error::other("driver run failure").into())
            } else {
                Ok(())
            }
        }
    }

    #[derive(Clone)]
    struct TestFactory {
        shutdowns: Arc<AtomicUsize>,
        build_failures: Arc<Mutex<BTreeSet<String>>>,
        run_failures: Arc<Mutex<BTreeSet<String>>>,
    }

    impl TestFactory {
        fn new() -> Self {
            Self {
                shutdowns: Arc::new(AtomicUsize::new(0)),
                build_failures: Arc::new(Mutex::new(BTreeSet::new())),
                run_failures: Arc::new(Mutex::new(BTreeSet::new())),
            }
        }
    }

    impl ChannelDriverFactory for TestFactory {
        fn build(
            &self,
            config: &ChannelInstanceConfig,
        ) -> ChannelResult<Box<dyn ManagedChannelDriver>> {
            let id = config.id().to_string();
            if self
                .build_failures
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .contains(&id)
            {
                return Err(io::Error::other(format!("build failure for {id}")).into());
            }

            let fail_on_run = self
                .run_failures
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .contains(&id);

            Ok(Box::new(RecordingDriver {
                kind: config.kind(),
                id,
                shutdowns: Arc::clone(&self.shutdowns),
                fail_on_run,
            }))
        }
    }

    fn dingtalk(id: &str, enabled: bool) -> ChannelInstanceConfig {
        ChannelInstanceConfig::Dingtalk(DingtalkConfig {
            id: id.to_string(),
            enabled,
            client_id: format!("{id}-client"),
            client_secret: format!("{id}-secret"),
            bot_title: format!("{id}-bot"),
            show_reasoning: false,
            stream_output: false,
            allowlist: Vec::new(),
            proxy: Default::default(),
        })
    }

    fn telegram(id: &str, enabled: bool) -> ChannelInstanceConfig {
        ChannelInstanceConfig::Telegram(TelegramConfig {
            id: id.to_string(),
            enabled,
            bot_token: format!("{id}-token"),
            show_reasoning: false,
            stream_output: false,
            allowlist: Vec::new(),
            proxy: Default::default(),
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sync_plans_keep_start_stop_and_restart() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let factory = TestFactory::new();
                let runtime = Arc::new(DummyRuntime);
                let mut manager = ChannelManager::with_factory(runtime, factory.clone());

                let first = ChannelConfigSnapshot {
                    instances: vec![dingtalk("alpha", true), dingtalk("beta", true)],
                };
                let result = manager.sync(first).await;
                assert_eq!(result.start.len(), 2);

                let second = ChannelConfigSnapshot {
                    instances: vec![
                        dingtalk("alpha", true),
                        dingtalk("beta", false),
                        dingtalk("gamma", true),
                    ],
                };
                let result = manager.sync(second).await;

                assert_eq!(
                    result.keep,
                    vec![ChannelInstanceKey::new(ChannelKind::Dingtalk, "alpha")]
                );
                assert_eq!(
                    result.start,
                    vec![ChannelInstanceKey::new(ChannelKind::Dingtalk, "gamma")]
                );
                assert!(result.restart.is_empty());
                assert_eq!(
                    result.stop,
                    vec![ChannelInstanceKey::new(ChannelKind::Dingtalk, "beta")]
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sync_restart_when_config_changes() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let factory = TestFactory::new();
                let runtime = Arc::new(DummyRuntime);
                let mut manager = ChannelManager::with_factory(runtime, factory);

                manager
                    .sync(ChannelConfigSnapshot {
                        instances: vec![dingtalk("alpha", true)],
                    })
                    .await;

                let mut changed = match dingtalk("alpha", true) {
                    ChannelInstanceConfig::Dingtalk(config) => config,
                    ChannelInstanceConfig::Telegram(_) => {
                        unreachable!("test helper returned telegram")
                    }
                };
                changed.show_reasoning = true;

                let result = manager
                    .sync(ChannelConfigSnapshot {
                        instances: vec![ChannelInstanceConfig::Dingtalk(changed)],
                    })
                    .await;

                assert_eq!(
                    result.restart,
                    vec![ChannelInstanceKey::new(ChannelKind::Dingtalk, "alpha")]
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_all_stops_running_channels() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let factory = TestFactory::new();
                let shutdowns = Arc::clone(&factory.shutdowns);
                let runtime = Arc::new(DummyRuntime);
                let mut manager = ChannelManager::with_factory(runtime, factory);

                manager
                    .sync(ChannelConfigSnapshot {
                        instances: vec![dingtalk("alpha", true), dingtalk("beta", true)],
                    })
                    .await;
                manager.shutdown_all().await;

                assert_eq!(shutdowns.load(Ordering::SeqCst), 2);
                assert!(
                    manager
                        .snapshot()
                        .iter()
                        .all(|status| status.state == ChannelLifecycleState::Stopped)
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_driver_build_does_not_block_other_instances() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let factory = TestFactory::new();
                factory
                    .build_failures
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .insert("broken".to_string());
                let runtime = Arc::new(DummyRuntime);
                let mut manager = ChannelManager::with_factory(runtime, factory);

                let result = manager
                    .sync(ChannelConfigSnapshot {
                        instances: vec![dingtalk("healthy", true), dingtalk("broken", true)],
                    })
                    .await;

                let broken = result
                    .statuses
                    .iter()
                    .find(|status| status.id == "broken")
                    .expect("broken status should exist");
                let healthy = result
                    .statuses
                    .iter()
                    .find(|status| status.id == "healthy")
                    .expect("healthy status should exist");

                assert_eq!(broken.state, ChannelLifecycleState::Failed);
                assert_eq!(healthy.state, ChannelLifecycleState::Running);
            })
            .await;
    }

    #[test]
    fn snapshot_from_channels_config_converts_dingtalk_instances() {
        let snapshot = ChannelConfigSnapshot::from_channels_config(&ChannelsConfig {
            dingtalk: vec![DingtalkConfig {
                id: "ops".to_string(),
                ..DingtalkConfig::default()
            }],
            telegram: vec![TelegramConfig {
                id: "bot".to_string(),
                ..TelegramConfig::default()
            }],
            disable_session_commands_for: Vec::new(),
        })
        .expect("snapshot should build");

        assert_eq!(snapshot.instances().len(), 2);
        assert_eq!(snapshot.instances()[0].key().as_str(), "dingtalk:ops");
        assert_eq!(snapshot.instances()[1].key().as_str(), "telegram:bot");
    }

    #[test]
    fn snapshot_from_channels_config_rejects_duplicate_same_type_ids() {
        let err = ChannelConfigSnapshot::from_channels_config(&ChannelsConfig {
            dingtalk: vec![
                DingtalkConfig {
                    id: "ops".to_string(),
                    ..DingtalkConfig::default()
                },
                DingtalkConfig {
                    id: "ops".to_string(),
                    ..DingtalkConfig::default()
                },
            ],
            telegram: Vec::new(),
            disable_session_commands_for: Vec::new(),
        })
        .expect_err("duplicate ids should fail");

        assert!(err.contains("duplicated channel instance"));
    }

    #[test]
    fn snapshot_from_channels_config_rejects_duplicate_telegram_ids() {
        let err = ChannelConfigSnapshot::from_channels_config(&ChannelsConfig {
            dingtalk: Vec::new(),
            telegram: vec![
                TelegramConfig {
                    id: "ops".to_string(),
                    ..TelegramConfig::default()
                },
                TelegramConfig {
                    id: "ops".to_string(),
                    ..TelegramConfig::default()
                },
            ],
            disable_session_commands_for: Vec::new(),
        })
        .expect_err("duplicate ids should fail");

        assert!(err.contains("duplicated channel instance"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sync_supports_telegram_instances() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let factory = TestFactory::new();
                let runtime = Arc::new(DummyRuntime);
                let mut manager = ChannelManager::with_factory(runtime, factory);

                let result = manager
                    .sync(ChannelConfigSnapshot {
                        instances: vec![telegram("ops-bot", true)],
                    })
                    .await;

                assert_eq!(
                    result.start,
                    vec![ChannelInstanceKey::new(ChannelKind::Telegram, "ops-bot")]
                );
                assert!(result.keep.is_empty());
                assert!(result.restart.is_empty());
                assert!(result.stop.is_empty());
            })
            .await;
    }
}
