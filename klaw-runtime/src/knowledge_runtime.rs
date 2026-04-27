use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use klaw_config::AppConfig;
use klaw_knowledge::{
    CreateKnowledgeNoteInput, KnowledgeAutoIndexHandle, KnowledgeEntry, KnowledgeError,
    KnowledgeHit, KnowledgeProvider, KnowledgeRuntimeSnapshot, KnowledgeRuntimeState,
    KnowledgeSearchQuery, KnowledgeSourceInfo, KnowledgeStatus, KnowledgeSyncProgress,
    KnowledgeSyncResult, ObsidianKnowledgeProvider, open_configured_obsidian_provider,
};
use tokio::sync::{Mutex, Notify, mpsc};

#[async_trait]
pub trait KnowledgeRuntimeProvider: KnowledgeProvider {
    async fn status(&self, enabled: bool) -> Result<KnowledgeStatus, KnowledgeError>;

    async fn sync(
        &self,
        enabled: bool,
        progress: mpsc::UnboundedSender<KnowledgeSyncProgress>,
    ) -> Result<KnowledgeSyncResult, KnowledgeError>;

    async fn start_auto_index(
        &self,
    ) -> Result<Option<Box<dyn KnowledgeAutoIndexHandle>>, KnowledgeError> {
        Ok(None)
    }
}

#[async_trait]
pub trait KnowledgeProviderLoader: Send + Sync {
    async fn load(
        &self,
        config: AppConfig,
    ) -> Result<Arc<dyn KnowledgeRuntimeProvider>, KnowledgeError>;
}

#[derive(Debug, Default)]
pub struct ConfiguredKnowledgeProviderLoader;

#[async_trait]
impl KnowledgeProviderLoader for ConfiguredKnowledgeProviderLoader {
    async fn load(
        &self,
        config: AppConfig,
    ) -> Result<Arc<dyn KnowledgeRuntimeProvider>, KnowledgeError> {
        let provider = open_configured_obsidian_provider(&config).await?;
        Ok(Arc::new(provider))
    }
}

#[async_trait]
impl KnowledgeRuntimeProvider for ObsidianKnowledgeProvider {
    async fn status(&self, enabled: bool) -> Result<KnowledgeStatus, KnowledgeError> {
        ObsidianKnowledgeProvider::status(self, enabled).await
    }

    async fn sync(
        &self,
        enabled: bool,
        progress: mpsc::UnboundedSender<KnowledgeSyncProgress>,
    ) -> Result<KnowledgeSyncResult, KnowledgeError> {
        let indexed_notes = self
            .reindex_with_progress(|update| {
                let _ = progress.send(update);
            })
            .await?;
        let embedded_chunks = self
            .embed_missing_chunks_with_progress(|update| {
                let _ = progress.send(update);
            })
            .await?;
        let status = self.status(enabled).await?;
        Ok(KnowledgeSyncResult {
            indexed_notes,
            embedded_chunks,
            status,
        })
    }

    async fn start_auto_index(
        &self,
    ) -> Result<Option<Box<dyn KnowledgeAutoIndexHandle>>, KnowledgeError> {
        Ok(Some(Box::new(self.start_auto_index_watcher()?)))
    }
}

pub struct KnowledgeRuntimeService {
    config: Mutex<AppConfig>,
    loader: Arc<dyn KnowledgeProviderLoader>,
    provider: Mutex<Option<Arc<dyn KnowledgeRuntimeProvider>>>,
    auto_index: Mutex<Option<Box<dyn KnowledgeAutoIndexHandle>>>,
    snapshot: Mutex<KnowledgeRuntimeSnapshot>,
    notify: Notify,
    shutting_down: AtomicBool,
}

impl KnowledgeRuntimeService {
    pub fn start(config: AppConfig, loader: Arc<dyn KnowledgeProviderLoader>) -> Arc<Self> {
        let initial = initial_snapshot(&config);
        let service = Arc::new(Self {
            config: Mutex::new(config),
            loader,
            provider: Mutex::new(None),
            auto_index: Mutex::new(None),
            snapshot: Mutex::new(initial.clone()),
            notify: Notify::new(),
            shutting_down: AtomicBool::new(false),
        });
        if initial.state == KnowledgeRuntimeState::Loading {
            let service_ref = Arc::clone(&service);
            tokio::spawn(async move {
                service_ref.load_provider().await;
            });
        }
        service
    }

    pub fn start_configured(config: &AppConfig) -> Arc<Self> {
        Self::start(config.clone(), Arc::new(ConfiguredKnowledgeProviderLoader))
    }

    pub async fn snapshot(&self) -> KnowledgeRuntimeSnapshot {
        self.snapshot.lock().await.clone()
    }

    pub async fn reload(self: &Arc<Self>, config: AppConfig) {
        self.shutting_down.store(false, Ordering::Relaxed);
        self.stop_auto_index().await;
        let initial = initial_snapshot(&config);
        *self.config.lock().await = config;
        *self.provider.lock().await = None;
        self.set_snapshot(initial.clone()).await;
        if initial.state == KnowledgeRuntimeState::Loading {
            let service_ref = Arc::clone(self);
            tokio::spawn(async move {
                service_ref.load_provider().await;
            });
        }
    }

    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::Relaxed);
        self.stop_auto_index().await;
        *self.provider.lock().await = None;
        self.set_snapshot(KnowledgeRuntimeSnapshot {
            state: KnowledgeRuntimeState::Disabled,
            status: None,
            error: None,
        })
        .await;
    }

    pub async fn wait_until_ready(&self) -> Result<(), KnowledgeError> {
        loop {
            let snapshot = self.snapshot().await;
            match snapshot.state {
                KnowledgeRuntimeState::Ready => return Ok(()),
                KnowledgeRuntimeState::Disabled => {
                    return Err(KnowledgeError::SourceUnavailable(
                        "knowledge runtime is disabled".to_string(),
                    ));
                }
                KnowledgeRuntimeState::Unconfigured => {
                    return Err(KnowledgeError::InvalidConfig(
                        "knowledge.obsidian.vault_path must be configured".to_string(),
                    ));
                }
                KnowledgeRuntimeState::Error => {
                    return Err(KnowledgeError::SourceUnavailable(
                        snapshot
                            .error
                            .unwrap_or_else(|| "knowledge runtime failed".to_string()),
                    ));
                }
                KnowledgeRuntimeState::Loading | KnowledgeRuntimeState::Syncing => {
                    self.notify.notified().await;
                }
            }
        }
    }

    pub async fn status(&self) -> Result<KnowledgeRuntimeSnapshot, KnowledgeError> {
        let state = self.snapshot().await.state;
        if state != KnowledgeRuntimeState::Ready && state != KnowledgeRuntimeState::Syncing {
            return Ok(self.snapshot().await);
        }
        let provider = self.ready_provider().await?;
        let enabled = self.config.lock().await.knowledge.enabled;
        let status = provider.status(enabled).await?;
        self.set_snapshot(KnowledgeRuntimeSnapshot {
            state,
            status: Some(status),
            error: None,
        })
        .await;
        Ok(self.snapshot().await)
    }

    pub async fn sync(
        &self,
        progress: mpsc::UnboundedSender<KnowledgeSyncProgress>,
    ) -> Result<KnowledgeSyncResult, KnowledgeError> {
        let provider = self.ready_provider().await?;
        let enabled = self.config.lock().await.knowledge.enabled;
        let current = self.snapshot().await;
        self.set_snapshot(KnowledgeRuntimeSnapshot {
            state: KnowledgeRuntimeState::Syncing,
            status: current.status,
            error: None,
        })
        .await;
        let result = provider.sync(enabled, progress).await;
        match result {
            Ok(result) => {
                self.set_snapshot(KnowledgeRuntimeSnapshot {
                    state: KnowledgeRuntimeState::Ready,
                    status: Some(result.status.clone()),
                    error: None,
                })
                .await;
                Ok(result)
            }
            Err(err) => {
                self.set_snapshot(KnowledgeRuntimeSnapshot {
                    state: KnowledgeRuntimeState::Error,
                    status: None,
                    error: Some(err.to_string()),
                })
                .await;
                Err(err)
            }
        }
    }

    async fn load_provider(&self) {
        if self.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        let config = self.config.lock().await.clone();
        match self.loader.load(config.clone()).await {
            Ok(provider) => {
                if self.shutting_down.load(Ordering::Relaxed) {
                    return;
                }
                let status = provider.status(config.knowledge.enabled).await;
                match status {
                    Ok(status) => {
                        if self.shutting_down.load(Ordering::Relaxed) {
                            return;
                        }
                        let auto_index = if config.knowledge.obsidian.auto_index {
                            match provider.start_auto_index().await {
                                Ok(handle) => handle,
                                Err(err) => {
                                    self.set_snapshot(KnowledgeRuntimeSnapshot {
                                        state: KnowledgeRuntimeState::Error,
                                        status: None,
                                        error: Some(err.to_string()),
                                    })
                                    .await;
                                    return;
                                }
                            }
                        } else {
                            None
                        };
                        *self.provider.lock().await = Some(provider);
                        *self.auto_index.lock().await = auto_index;
                        self.set_snapshot(KnowledgeRuntimeSnapshot {
                            state: KnowledgeRuntimeState::Ready,
                            status: Some(status),
                            error: None,
                        })
                        .await;
                    }
                    Err(err) => {
                        self.set_snapshot(KnowledgeRuntimeSnapshot {
                            state: KnowledgeRuntimeState::Error,
                            status: None,
                            error: Some(err.to_string()),
                        })
                        .await;
                    }
                }
            }
            Err(err) => {
                self.set_snapshot(KnowledgeRuntimeSnapshot {
                    state: KnowledgeRuntimeState::Error,
                    status: None,
                    error: Some(err.to_string()),
                })
                .await;
            }
        }
    }

    async fn ready_provider(&self) -> Result<Arc<dyn KnowledgeRuntimeProvider>, KnowledgeError> {
        self.wait_until_ready().await?;
        self.provider.lock().await.clone().ok_or_else(|| {
            KnowledgeError::SourceUnavailable("knowledge runtime is not ready".to_string())
        })
    }

    async fn set_snapshot(&self, snapshot: KnowledgeRuntimeSnapshot) {
        *self.snapshot.lock().await = snapshot;
        self.notify.notify_waiters();
    }

    async fn stop_auto_index(&self) {
        if let Some(handle) = self.auto_index.lock().await.take() {
            handle.stop().await;
        }
    }
}

#[async_trait]
impl KnowledgeProvider for KnowledgeRuntimeService {
    fn provider_name(&self) -> &str {
        "runtime"
    }

    async fn search(
        &self,
        query: KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeHit>, KnowledgeError> {
        self.ready_provider().await?.search(query).await
    }

    async fn get(&self, id: &str) -> Result<Option<KnowledgeEntry>, KnowledgeError> {
        self.ready_provider().await?.get(id).await
    }

    async fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError> {
        self.ready_provider().await?.list_sources().await
    }

    async fn create_note(
        &self,
        input: CreateKnowledgeNoteInput,
    ) -> Result<KnowledgeEntry, KnowledgeError> {
        self.ready_provider().await?.create_note(input).await
    }
}

fn initial_snapshot(config: &AppConfig) -> KnowledgeRuntimeSnapshot {
    if !config.knowledge.enabled {
        return KnowledgeRuntimeSnapshot {
            state: KnowledgeRuntimeState::Disabled,
            status: None,
            error: None,
        };
    }
    let vault_path = config
        .knowledge
        .obsidian
        .vault_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    if vault_path.is_none() {
        return KnowledgeRuntimeSnapshot {
            state: KnowledgeRuntimeState::Unconfigured,
            status: None,
            error: Some("knowledge.obsidian.vault_path must be configured".to_string()),
        };
    }
    KnowledgeRuntimeSnapshot {
        state: KnowledgeRuntimeState::Loading,
        status: None,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use klaw_config::AppConfig;
    use klaw_knowledge::{
        CreateKnowledgeNoteInput, KnowledgeAutoIndexHandle, KnowledgeEntry, KnowledgeError,
        KnowledgeHit, KnowledgeProvider, KnowledgeRuntimeState, KnowledgeSearchQuery,
        KnowledgeSourceInfo, KnowledgeStatus, KnowledgeSyncProgress, KnowledgeSyncResult,
    };
    use tokio::sync::mpsc;

    use super::{KnowledgeProviderLoader, KnowledgeRuntimeProvider, KnowledgeRuntimeService};

    #[derive(Default)]
    struct CountingLoader {
        loads: AtomicUsize,
        auto_index_starts: Arc<AtomicUsize>,
        auto_index_stops: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl KnowledgeProviderLoader for CountingLoader {
        async fn load(
            &self,
            _config: AppConfig,
        ) -> Result<Arc<dyn KnowledgeRuntimeProvider>, KnowledgeError> {
            self.loads.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(FakeRuntimeProvider {
                auto_index_starts: Arc::clone(&self.auto_index_starts),
                auto_index_stops: Arc::clone(&self.auto_index_stops),
                ..Default::default()
            }))
        }
    }

    #[derive(Default)]
    struct FakeRuntimeProvider {
        searches: AtomicUsize,
        syncs: AtomicUsize,
        auto_index_starts: Arc<AtomicUsize>,
        auto_index_stops: Arc<AtomicUsize>,
    }

    struct FakeAutoIndexHandle {
        stops: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl KnowledgeAutoIndexHandle for FakeAutoIndexHandle {
        async fn stop(self: Box<Self>) {
            self.stops.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[async_trait]
    impl KnowledgeProvider for FakeRuntimeProvider {
        fn provider_name(&self) -> &str {
            "fake"
        }

        async fn search(
            &self,
            _query: KnowledgeSearchQuery,
        ) -> Result<Vec<KnowledgeHit>, KnowledgeError> {
            self.searches.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }

        async fn get(&self, _id: &str) -> Result<Option<KnowledgeEntry>, KnowledgeError> {
            Ok(None)
        }

        async fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError> {
            Ok(Vec::new())
        }

        async fn create_note(
            &self,
            input: CreateKnowledgeNoteInput,
        ) -> Result<KnowledgeEntry, KnowledgeError> {
            Ok(KnowledgeEntry {
                id: input.path.clone(),
                title: "Created".to_string(),
                content: input.content,
                tags: Vec::new(),
                uri: input.path,
                source: "fake".to_string(),
                metadata: serde_json::json!({}),
                created_at_ms: 1,
                updated_at_ms: 1,
            })
        }
    }

    #[async_trait]
    impl KnowledgeRuntimeProvider for FakeRuntimeProvider {
        async fn status(&self, enabled: bool) -> Result<KnowledgeStatus, KnowledgeError> {
            Ok(KnowledgeStatus {
                enabled,
                provider: "fake".to_string(),
                source_name: "Fake".to_string(),
                vault_path: Some("/tmp/fake".to_string()),
                entry_count: 1,
                chunk_count: 1,
                embedded_chunk_count: 1,
                missing_embedding_count: 0,
            })
        }

        async fn sync(
            &self,
            enabled: bool,
            progress: mpsc::UnboundedSender<KnowledgeSyncProgress>,
        ) -> Result<KnowledgeSyncResult, KnowledgeError> {
            self.syncs.fetch_add(1, Ordering::Relaxed);
            drop(progress);
            Ok(KnowledgeSyncResult {
                indexed_notes: 0,
                embedded_chunks: 0,
                status: self.status(enabled).await?,
            })
        }

        async fn start_auto_index(
            &self,
        ) -> Result<Option<Box<dyn KnowledgeAutoIndexHandle>>, KnowledgeError> {
            self.auto_index_starts.fetch_add(1, Ordering::Relaxed);
            Ok(Some(Box::new(FakeAutoIndexHandle {
                stops: Arc::clone(&self.auto_index_stops),
            })))
        }
    }

    #[tokio::test]
    async fn service_loads_provider_once_for_status_search_and_sync() {
        let loader = Arc::new(CountingLoader::default());
        let mut config = AppConfig::default();
        config.knowledge.enabled = true;
        config.knowledge.obsidian.vault_path = Some("/tmp/fake".to_string());
        let service = KnowledgeRuntimeService::start(config, loader.clone());

        service
            .wait_until_ready()
            .await
            .expect("service should load");
        assert_eq!(service.snapshot().await.state, KnowledgeRuntimeState::Ready);

        service.status().await.expect("status should work");
        service
            .search(KnowledgeSearchQuery {
                text: "auth".to_string(),
                limit: 5,
                ..Default::default()
            })
            .await
            .expect("search should work");
        let (_progress_tx, progress_rx) = mpsc::unbounded_channel();
        drop(progress_rx);
        service.sync(_progress_tx).await.expect("sync should work");

        assert_eq!(loader.loads.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn reload_from_disabled_starts_loading_configured_provider() {
        let loader = Arc::new(CountingLoader::default());
        let disabled = AppConfig::default();
        let service = KnowledgeRuntimeService::start(disabled, loader.clone());

        assert_eq!(
            service.snapshot().await.state,
            KnowledgeRuntimeState::Disabled
        );

        let mut enabled = AppConfig::default();
        enabled.knowledge.enabled = true;
        enabled.knowledge.obsidian.vault_path = Some("/tmp/fake".to_string());
        service.reload(enabled).await;
        service
            .wait_until_ready()
            .await
            .expect("service should load");

        assert_eq!(service.snapshot().await.state, KnowledgeRuntimeState::Ready);
        assert_eq!(loader.loads.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn shutdown_releases_ready_provider() {
        let loader = Arc::new(CountingLoader::default());
        let mut config = AppConfig::default();
        config.knowledge.enabled = true;
        config.knowledge.obsidian.vault_path = Some("/tmp/fake".to_string());
        let service = KnowledgeRuntimeService::start(config, loader.clone());

        service
            .wait_until_ready()
            .await
            .expect("service should load");
        service.shutdown().await;

        assert_eq!(
            service.snapshot().await.state,
            KnowledgeRuntimeState::Disabled
        );
        assert!(service.provider.lock().await.is_none());
    }

    #[tokio::test]
    async fn auto_index_disabled_does_not_start_watcher() {
        let loader = Arc::new(CountingLoader::default());
        let mut config = AppConfig::default();
        config.knowledge.enabled = true;
        config.knowledge.obsidian.vault_path = Some("/tmp/fake".to_string());
        config.knowledge.obsidian.auto_index = false;
        let service = KnowledgeRuntimeService::start(config, loader.clone());

        service
            .wait_until_ready()
            .await
            .expect("service should load");

        assert_eq!(loader.auto_index_starts.load(Ordering::Relaxed), 0);
        assert!(service.auto_index.lock().await.is_none());
    }

    #[tokio::test]
    async fn auto_index_reload_stops_previous_watcher_and_starts_new_one() {
        let loader = Arc::new(CountingLoader::default());
        let mut config = AppConfig::default();
        config.knowledge.enabled = true;
        config.knowledge.obsidian.vault_path = Some("/tmp/fake".to_string());
        config.knowledge.obsidian.auto_index = true;
        let service = KnowledgeRuntimeService::start(config.clone(), loader.clone());

        service
            .wait_until_ready()
            .await
            .expect("service should load");
        assert_eq!(loader.auto_index_starts.load(Ordering::Relaxed), 1);
        assert_eq!(loader.auto_index_stops.load(Ordering::Relaxed), 0);

        service.reload(config).await;
        service
            .wait_until_ready()
            .await
            .expect("service should reload");

        assert_eq!(loader.auto_index_starts.load(Ordering::Relaxed), 2);
        assert_eq!(loader.auto_index_stops.load(Ordering::Relaxed), 1);
        assert!(service.auto_index.lock().await.is_some());
    }
}
