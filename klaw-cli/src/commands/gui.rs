use clap::Args;
use klaw_acp::AcpConfigSnapshot;
use klaw_channel::{ChannelConfigSnapshot, ChannelManager};
use klaw_config::AppConfig;
use klaw_llm::ToolDefinition;
use klaw_mcp::McpConfigSnapshot;
use klaw_runtime::gateway_manager::GatewayManager;
use klaw_runtime::{
    RuntimeBundle, build_channel_driver_factory, build_hosted_runtime,
    reload_runtime_skills_prompt, set_runtime_provider_override, shutdown_runtime_bundle,
    sync_runtime_providers, sync_runtime_tools,
};
use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, oneshot, watch};

use super::startup_display::print_startup_banner;
use crate::commands::signal::shutdown_signal;
use klaw_config::ConfigStore;
use tracing::{info, warn};

async fn cancel_pending_acp_permissions(
    pending: &AsyncMutex<BTreeMap<u64, oneshot::Sender<klaw_acp::AcpPermissionDecision>>>,
) {
    let waiters = {
        let mut guard = pending.lock().await;
        std::mem::take(&mut *guard)
    };
    for (_, waiter) in waiters {
        let _ = waiter.send(klaw_acp::AcpPermissionDecision::Cancelled);
    }
}

fn wait_for_worker_shutdown<T>(
    worker: std::thread::JoinHandle<T>,
    timeout: Duration,
) -> io::Result<T>
where
    T: Send + 'static,
{
    let (join_tx, join_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = join_tx.send(worker.join());
    });

    match join_rx.recv_timeout(timeout) {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(io::Error::other(format!(
            "gui runtime worker panicked: {err:?}"
        ))),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(io::Error::other(
            "timed out waiting for gui runtime worker shutdown",
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::other(
            "gui runtime worker join channel closed unexpectedly",
        )),
    }
}

fn provider_runtime_snapshot(runtime: &RuntimeBundle) -> klaw_gui::ProviderRuntimeSnapshot {
    let provider_runtime = runtime.runtime.provider_runtime_snapshot();
    let runtime_provider_override = runtime
        .runtime_provider_override
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .clone();
    let active_provider_id = runtime_provider_override
        .clone()
        .unwrap_or_else(|| provider_runtime.default_provider_id.clone());
    let active_model = provider_runtime
        .provider_default_models
        .get(&active_provider_id)
        .cloned()
        .unwrap_or_else(|| provider_runtime.default_model.clone());
    klaw_gui::ProviderRuntimeSnapshot {
        default_provider_id: provider_runtime.default_provider_id,
        provider_default_models: provider_runtime.provider_default_models,
        runtime_provider_override,
        active_provider_id,
        active_model,
    }
}

fn tool_definitions(runtime: &RuntimeBundle) -> Vec<ToolDefinition> {
    let mut definitions = runtime
        .runtime
        .tools
        .list()
        .into_iter()
        .filter_map(|name| runtime.runtime.tools.get(&name))
        .map(|tool| ToolDefinition {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters: tool.parameters(),
        })
        .collect::<Vec<_>>();
    definitions.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    definitions
}

async fn run_execute_acp_prompt_stream_command(
    agent_id: String,
    prompt: String,
    working_directory: Option<String>,
    timeout_seconds: Option<u64>,
    events: std::sync::mpsc::Sender<klaw_gui::AcpPromptEvent>,
    acp_manager: Arc<AsyncMutex<klaw_acp::AcpManager>>,
    pending_permissions: Arc<
        AsyncMutex<BTreeMap<u64, oneshot::Sender<klaw_acp::AcpPermissionDecision>>>,
    >,
    permission_counter: Arc<AtomicU64>,
    cancel_rx: watch::Receiver<bool>,
) {
    tracing::debug!(
        agent = %agent_id,
        working_directory = ?working_directory,
        timeout_seconds,
        prompt_len = prompt.len(),
        "gui requested acp test prompt"
    );
    let timeout = timeout_seconds.map(Duration::from_secs);
    let execution_config = {
        let guard = acp_manager.lock().await;
        guard.agent_execution_config(&agent_id)
    };
    let result = match execution_config {
        Ok((config, startup_timeout)) => {
            let chunk_events = events.clone();
            let session_permission_counter = Arc::clone(&permission_counter);
            let sink = Arc::new(move |update: klaw_acp::AcpPromptUpdate| match update {
                klaw_acp::AcpPromptUpdate::SessionEvent(event) => {
                    let _ = chunk_events.send(klaw_gui::AcpPromptEvent::SessionEvent(event));
                }
                klaw_acp::AcpPromptUpdate::PermissionRequest(request) => {
                    let request_id = session_permission_counter.fetch_add(1, Ordering::Relaxed);
                    let _ = chunk_events.send(klaw_gui::AcpPromptEvent::PermissionRequested {
                        request_id,
                        request,
                    });
                }
            });
            let permission_events = events.clone();
            let interactive_permission_counter = Arc::clone(&permission_counter);
            let permission_waiters_for_handler = Arc::clone(&pending_permissions);
            let permission_handler: klaw_acp::AcpPermissionRequestHandler = Arc::new(
                move |request: klaw_acp::AcpPermissionRequest| -> klaw_acp::AcpPermissionRequestFuture {
                    let pending_permissions = Arc::clone(&permission_waiters_for_handler);
                    let permission_events = permission_events.clone();
                    let request_id = interactive_permission_counter.fetch_add(1, Ordering::Relaxed);
                    Box::pin(async move {
                        let (decision_tx, decision_rx) = oneshot::channel();
                        pending_permissions
                            .lock()
                            .await
                            .insert(request_id, decision_tx);
                        let _ = permission_events.send(klaw_gui::AcpPromptEvent::PermissionRequested {
                            request_id,
                            request,
                        });
                        match decision_rx.await {
                            Ok(decision) => decision,
                            Err(_) => klaw_acp::AcpPermissionDecision::Cancelled,
                        }
                    })
                },
            );
            klaw_acp::AcpManager::execute_prompt_with_config_stream(
                config,
                startup_timeout,
                &prompt,
                working_directory.as_deref(),
                timeout,
                Some(sink),
                Some(permission_handler),
                Some(cancel_rx),
            )
            .await
        }
        Err(err) => Err(err),
    };
    cancel_pending_acp_permissions(pending_permissions.as_ref()).await;
    match result {
        Ok(final_output) => {
            let _ = events.send(klaw_gui::AcpPromptEvent::Completed { final_output });
        }
        Err(klaw_acp::AcpExecutionError::Cancelled { .. }) => {
            let _ = events.send(klaw_gui::AcpPromptEvent::Stopped);
        }
        Err(err) => {
            let _ = events.send(klaw_gui::AcpPromptEvent::Failed(err.to_string()));
        }
    }
}

#[derive(Debug, Args)]
pub struct GuiCommand {}

impl GuiCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let (startup_tx, startup_rx) = std::sync::mpsc::channel::<Result<_, String>>();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (runtime_cmd_tx, runtime_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let config_for_thread = config.as_ref().clone();
        let channel_snapshot = ChannelConfigSnapshot::from_channels_config(&config.channels)
            .map_err(io::Error::other)?;

        let worker = std::thread::Builder::new()
            .name("klaw-gui-runtime".to_string())
            .spawn(move || -> Result<(), String> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;

                runtime.block_on(async move {
                    let hosted = match build_hosted_runtime(&config_for_thread).await {
                        Ok(hosted) => hosted,
                        Err(err) => {
                            let err = err.to_string();
                            let _ = startup_tx.send(Err(err.clone()));
                            return Err(err);
                        }
                    };
                    let _ = startup_tx.send(Ok(hosted.startup_report.clone()));

                    let runtime = Arc::clone(&hosted.runtime);
                    let background = Arc::clone(&hosted.background);
                    let gateway_manager = Arc::new(AsyncMutex::new(GatewayManager::new(
                        &config_for_thread,
                        Arc::clone(&runtime),
                    )));
                    let gateway_status_cache = Arc::new(StdMutex::new({
                        let manager = gateway_manager.lock().await;
                        manager.snapshot()
                    }));
                    if let Err(err) = gateway_manager
                        .lock()
                        .await
                        .start_if_enabled(&config_for_thread)
                        .await
                    {
                        warn!(error = %err, "failed to start gateway for gui runtime");
                    }
                    {
                        let snapshot = gateway_manager.lock().await.snapshot();
                        let mut cache = gateway_status_cache
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        *cache = snapshot;
                    }
                    let adapter = Arc::clone(&hosted.adapter);
                    let local = tokio::task::LocalSet::new();
                    local
                        .run_until(async move {
                            let mut shutdown_rx = shutdown_rx;
                            let mut runtime_cmd_rx = runtime_cmd_rx;
                            let mut runtime_cmd_open = true;
                            let mut active_acp_prompt_cancel: Option<watch::Sender<bool>> = None;
                            let active_acp_permission_waiters = Arc::new(AsyncMutex::new(
                                BTreeMap::<
                                    u64,
                                    oneshot::Sender<klaw_acp::AcpPermissionDecision>,
                                >::new(),
                            ));
                            let next_acp_permission_id = Arc::new(AtomicU64::new(1));
                            let channel_factory = build_channel_driver_factory(&config_for_thread)
                                .map_err(|err| err.to_string())?;
                            let channel_manager = Arc::new(AsyncMutex::new(
                                ChannelManager::with_factory(
                                    Arc::clone(&adapter),
                                    channel_factory,
                                ),
                            ));
                            channel_manager.lock().await.sync(channel_snapshot).await;

                            let mcp_manager = {
                                let guard = runtime.mcp_init.lock().await;
                                guard.manager()
                            };
                            let acp_manager = {
                                let guard = runtime.acp_init.lock().await;
                                guard.manager()
                            };

                            let shutdown_by_signal = loop {
                                tokio::select! {
                                    changed = shutdown_rx.changed() => {
                                        match changed {
                                            Ok(()) => break !*shutdown_rx.borrow(),
                                            Err(_) => break false,
                                        }
                                    }
                                    _ = shutdown_signal() => {
                                        break true
                                    }
                                    command = runtime_cmd_rx.recv(), if runtime_cmd_open => {
                                        match command {
                                            Some(klaw_gui::RuntimeCommand::ReloadSkillsPrompt) => {
                                                if let Err(err) = reload_runtime_skills_prompt(runtime.as_ref()).await {
                                                    warn!(error = %err, "failed to reload runtime skills prompt");
                                                }
                                            }
                                            Some(klaw_gui::RuntimeCommand::SetProviderOverride { provider_id, response }) => {
                                                let result = set_runtime_provider_override(
                                                    runtime.as_ref(),
                                                    provider_id.as_deref(),
                                                )
                                                .map_err(|err| err.to_string());
                                                let _ = response.send(result);
                                            }
                                            Some(klaw_gui::RuntimeCommand::SyncProviders { response }) => {
                                                let runtime = Arc::clone(&runtime);
                                                tokio::task::spawn_local(async move {
                                                    let result = match ConfigStore::open(None) {
                                                        Ok(store) => {
                                                            let snapshot = store.snapshot();
                                                            sync_runtime_providers(runtime.as_ref(), &snapshot.config)
                                                                .map(|_| provider_runtime_snapshot(runtime.as_ref()))
                                                                .map_err(|err| err.to_string())
                                                        }
                                                        Err(err) => Err(err.to_string()),
                                                    };
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetProviderStatus { response }) => {
                                                let _ = response.send(Ok(provider_runtime_snapshot(runtime.as_ref())));
                                            }
                                            Some(klaw_gui::RuntimeCommand::SyncChannels { response }) => {
                                                let channel_manager = Arc::clone(&channel_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = match ConfigStore::open(None) {
                                                        Ok(store) => {
                                                            let snapshot = store.snapshot();
                                                            match ChannelConfigSnapshot::from_channels_config(&snapshot.config.channels) {
                                                                Ok(channel_snapshot) => Ok(channel_manager.lock().await.sync(channel_snapshot).await),
                                                                Err(err) => Err(err),
                                                            }
                                                        }
                                                        Err(err) => Err(err.to_string()),
                                                    };
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetChannelStatus { response }) => {
                                                let channel_manager = Arc::clone(&channel_manager);
                                                tokio::task::spawn_local(async move {
                                                    let statuses = channel_manager.lock().await.snapshot();
                                                    let _ = response.send(Ok(statuses));
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::RestartChannel { instance_key, response }) => {
                                                let channel_manager = Arc::clone(&channel_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = match ConfigStore::open(None) {
                                                        Ok(store) => {
                                                            let snapshot = store.snapshot();
                                                            match ChannelConfigSnapshot::from_channels_config(&snapshot.config.channels) {
                                                                Ok(channel_snapshot) => {
                                                                    match klaw_channel::ChannelInstanceKey::parse(&instance_key) {
                                                                        Ok(key) => {
                                                                            channel_manager
                                                                                .lock()
                                                                                .await
                                                                                .restart_channel(&key, &channel_snapshot)
                                                                                .await
                                                                        }
                                                                        Err(err) => Err(err),
                                                                    }
                                                                }
                                                                Err(err) => Err(err),
                                                            }
                                                        }
                                                        Err(err) => Err(err.to_string()),
                                                    };
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::RestartMcpServer { server_id, response }) => {
                                                let manager = Arc::clone(&mcp_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = match ConfigStore::open(None) {
                                                        Ok(store) => {
                                                            let snapshot = store.snapshot();
                                                            let mcp_snapshot = McpConfigSnapshot::from_mcp_config(&snapshot.config.mcp);
                                                            let mut guard = manager.lock().await;
                                                            guard.restart_server(&klaw_mcp::McpServerKey::new(&server_id), &mcp_snapshot).await
                                                        }
                                                        Err(err) => Err(err.to_string()),
                                                    };
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::SyncMcp { response }) => {
                                                let manager = Arc::clone(&mcp_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = match ConfigStore::open(None) {
                                                        Ok(store) => {
                                                            let snapshot = store.snapshot();
                                                            let mcp_snapshot = McpConfigSnapshot::from_mcp_config(&snapshot.config.mcp);
                                                            let mut guard = manager.lock().await;
                                                            Ok(guard.sync(mcp_snapshot).await)
                                                        }
                                                        Err(err) => Err(err.to_string()),
                                                    };
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::SyncAcp { response }) => {
                                                let manager = Arc::clone(&acp_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = match ConfigStore::open(None) {
                                                        Ok(store) => {
                                                            let snapshot = store.snapshot();
                                                            let acp_snapshot = AcpConfigSnapshot::from_config(&snapshot.config.acp);
                                                            let mut guard = manager.lock().await;
                                                            Ok(guard.sync(Arc::clone(&manager), acp_snapshot).await)
                                                        }
                                                        Err(err) => Err(err.to_string()),
                                                    };
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::SyncTools { response }) => {
                                                let runtime = Arc::clone(&runtime);
                                                tokio::task::spawn_local(async move {
                                                    let result = match ConfigStore::open(None) {
                                                        Ok(store) => {
                                                            let snapshot = store.snapshot();
                                                            sync_runtime_tools(runtime.as_ref(), &snapshot.config)
                                                                .await
                                                                .map_err(|err| err.to_string())
                                                        }
                                                        Err(err) => Err(err.to_string()),
                                                    };
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetToolDefinitions { response }) => {
                                                let _ = response.send(Ok(tool_definitions(runtime.as_ref())));
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetMcpStatus { response }) => {
                                                let result = match ConfigStore::open(None) {
                                                    Ok(store) => {
                                                        let snapshot = store.snapshot();
                                                        let mcp_snapshot = McpConfigSnapshot::from_mcp_config(&snapshot.config.mcp);
                                                        match mcp_manager.try_lock() {
                                                            Ok(guard) => Ok(guard.runtime_snapshot(&mcp_snapshot)),
                                                            Err(_) => Err("mcp manager is busy".to_string()),
                                                        }
                                                    }
                                                    Err(err) => Err(err.to_string()),
                                                };
                                                let _ = response.send(result);
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetAcpStatus { response }) => {
                                                let result = match ConfigStore::open(None) {
                                                    Ok(store) => {
                                                        let snapshot = store.snapshot();
                                                        let acp_snapshot = AcpConfigSnapshot::from_config(&snapshot.config.acp);
                                                        match acp_manager.try_lock() {
                                                            Ok(guard) => Ok(guard.runtime_snapshot(&acp_snapshot)),
                                                            Err(_) => Err("acp manager is busy".to_string()),
                                                        }
                                                    }
                                                    Err(err) => Err(err.to_string()),
                                                };
                                                let _ = response.send(result);
                                            }
                                            Some(klaw_gui::RuntimeCommand::ExecuteAcpPromptStream {
                                                agent_id,
                                                prompt,
                                                working_directory,
                                                timeout_seconds,
                                                events,
                                            }) => {
                                                let (cancel_tx, cancel_rx) = watch::channel(false);
                                                active_acp_prompt_cancel = Some(cancel_tx);
                                                cancel_pending_acp_permissions(
                                                    active_acp_permission_waiters.as_ref(),
                                                )
                                                .await;
                                                let manager = Arc::clone(&acp_manager);
                                                let pending_permissions = Arc::clone(
                                                    &active_acp_permission_waiters,
                                                );
                                                let permission_counter =
                                                    Arc::clone(&next_acp_permission_id);
                                                tokio::task::spawn_local(run_execute_acp_prompt_stream_command(
                                                    agent_id,
                                                    prompt,
                                                    working_directory,
                                                    timeout_seconds,
                                                    events,
                                                    manager,
                                                    pending_permissions,
                                                    permission_counter,
                                                    cancel_rx,
                                                ));
                                            }
                                            Some(klaw_gui::RuntimeCommand::StopAcpPrompt { response }) => {
                                                cancel_pending_acp_permissions(
                                                    active_acp_permission_waiters.as_ref(),
                                                )
                                                .await;
                                                let result = match active_acp_prompt_cancel.take() {
                                                    Some(cancel) => cancel
                                                        .send(true)
                                                        .map_err(|_| "acp prompt is no longer running".to_string()),
                                                    None => Err("no ACP test prompt is currently running".to_string()),
                                                };
                                                let _ = response.send(result.map(|_| ()));
                                            }
                                            Some(klaw_gui::RuntimeCommand::ResolveAcpPermission {
                                                request_id,
                                                decision,
                                                response,
                                            }) => {
                                                let waiter = active_acp_permission_waiters
                                                    .lock()
                                                    .await
                                                    .remove(&request_id);
                                                let result = match waiter {
                                                    Some(waiter) => waiter
                                                        .send(decision)
                                                        .map_err(|_| {
                                                            "acp permission request is no longer waiting"
                                                                .to_string()
                                                        }),
                                                    None => Err(format!(
                                                        "unknown acp permission request `{request_id}`"
                                                    )),
                                                };
                                                let _ = response.send(result.map(|_| ()));
                                            }
                                            Some(klaw_gui::RuntimeCommand::RunCronNow { cron_id, response }) => {
                                                let runtime = Arc::clone(&runtime);
                                                let background = Arc::clone(&background);
                                                tokio::task::spawn_local(async move {
                                                    let result = background.run_cron_now(&cron_id).await;
                                                    if result.is_ok() {
                                                        background.on_runtime_tick(runtime.as_ref()).await;
                                                    }
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::RunHeartbeatNow { heartbeat_id, response }) => {
                                                let runtime = Arc::clone(&runtime);
                                                let background = Arc::clone(&background);
                                                tokio::task::spawn_local(async move {
                                                    let result = background.run_heartbeat_now(&heartbeat_id).await;
                                                    if result.is_ok() {
                                                        background.on_runtime_tick(runtime.as_ref()).await;
                                                    }
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::RunMemoryArchiveNow { response }) => {
                                                let runtime = Arc::clone(&runtime);
                                                let background = Arc::clone(&background);
                                                tokio::task::spawn_local(async move {
                                                    let result = background.run_memory_archive_now().await;
                                                    if result.is_ok() {
                                                        background.on_runtime_tick(runtime.as_ref()).await;
                                                    }
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetEnvCheck { response }) => {
                                                let env_check = runtime.env_check.clone();
                                                let _ = response.send(env_check);
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetGatewayStatus { response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                let gateway_status_cache = Arc::clone(&gateway_status_cache);
                                                tokio::task::spawn_local(async move {
                                                    if let Ok(mut gateway_manager) = gateway_manager.try_lock() {
                                                        if let Err(err) = gateway_manager.refresh_from_store() {
                                                            warn!(error = %err, "failed to refresh gateway config metadata");
                                                        }
                                                        let snapshot = gateway_manager.snapshot();
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        *cache = snapshot.clone();
                                                        let _ = response.send(snapshot);
                                                        return;
                                                    }
                                                    let snapshot = gateway_status_cache
                                                        .lock()
                                                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                                                        .clone();
                                                    let _ = response.send(snapshot);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetTailscaleHostStatus { response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                let gateway_status_cache = Arc::clone(&gateway_status_cache);
                                                tokio::task::spawn_local(async move {
                                                    let host = tokio::task::spawn_blocking(
                                                        klaw_gateway::TailscaleManager::inspect_host,
                                                    )
                                                    .await
                                                    .unwrap_or_else(|err| klaw_gateway::TailscaleHostInfo {
                                                        status: klaw_gateway::TailscaleStatus::Error(
                                                            format!("failed to join tailscale host probe: {err}"),
                                                        ),
                                                        message: Some(
                                                            "Tailscale host probe worker failed unexpectedly."
                                                                .to_string(),
                                                        ),
                                                        ..Default::default()
                                                    });
                                                    let mut gateway_manager = gateway_manager.lock().await;
                                                    gateway_manager.set_tailscale_host(host.clone());
                                                    let snapshot = gateway_manager.snapshot();
                                                    let mut cache = gateway_status_cache
                                                        .lock()
                                                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                    *cache = snapshot;
                                                    let _ = response.send(Ok(host));
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::StartGateway { response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                let gateway_status_cache = Arc::clone(&gateway_status_cache);
                                                tokio::task::spawn_local(async move {
                                                    {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        cache.transitioning = true;
                                                    }
                                                    let result = gateway_manager.lock().await.start_from_store().await;
                                                    if let Ok(snapshot) = &result {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        *cache = snapshot.clone();
                                                    }
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::SetGatewayEnabled { enabled, response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                let gateway_status_cache = Arc::clone(&gateway_status_cache);
                                                tokio::task::spawn_local(async move {
                                                    {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        cache.transitioning = true;
                                                    }
                                                    let result = gateway_manager.lock().await.set_enabled(enabled).await;
                                                    if let Ok(snapshot) = &result {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        *cache = snapshot.clone();
                                                    }
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::RestartGateway { response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                let gateway_status_cache = Arc::clone(&gateway_status_cache);
                                                tokio::task::spawn_local(async move {
                                                    {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        cache.transitioning = true;
                                                    }
                                                    let result = gateway_manager.lock().await.restart_from_store().await;
                                                    if let Ok(snapshot) = &result {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        *cache = snapshot.clone();
                                                    }
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::SetTailscaleMode { mode, response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                let gateway_status_cache = Arc::clone(&gateway_status_cache);
                                                tokio::task::spawn_local(async move {
                                                    {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        cache.transitioning = true;
                                                    }
                                                    let result = gateway_manager.lock().await.set_tailscale_mode(mode).await;
                                                    if let Ok(snapshot) = &result {
                                                        let mut cache = gateway_status_cache
                                                            .lock()
                                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                                        *cache = snapshot.clone();
                                                    }
                                                    let _ = response.send(result);
                                                });
                                            }
                                            None => {
                                                runtime_cmd_open = false;
                                            }
                                        }
                                    }
                                }
                            };
                            if shutdown_by_signal {
                                info!("shutdown signal received, stopping gui runtime");
                            }

                            if let Err(err) = gateway_manager.lock().await.stop().await {
                                warn!(error = %err, "failed to stop gateway during gui shutdown");
                            }
                            channel_manager.lock().await.shutdown_all().await;
                            if let Err(err) = shutdown_runtime_bundle(runtime.as_ref()).await {
                                warn!(error = %err, "runtime shutdown failed");
                            }
                            if shutdown_by_signal {
                                std::process::exit(130);
                            }
                            Ok::<(), String>(())
                        })
                        .await
                })
            })
            .map_err(|err| {
                io::Error::other(format!("failed to spawn gui runtime worker: {err}"))
            })?;

        let startup_result = tokio::task::spawn_blocking(move || startup_rx.recv())
            .await
            .map_err(|err| io::Error::other(format!("startup wait task failed: {err}")))?
            .map_err(|err| io::Error::other(format!("startup channel closed: {err}")))?;
        let startup_report =
            startup_result.map_err(|err| io::Error::other(format!("gui startup failed: {err}")))?;
        print_startup_banner(config.as_ref(), &startup_report);

        klaw_gui::install_runtime_command_sender(runtime_cmd_tx);
        let gui_result = klaw_gui::run();
        klaw_gui::clear_runtime_command_sender();
        klaw_gui::clear_log_receiver();
        let _ = shutdown_tx.send(true);
        let worker_result = tokio::task::spawn_blocking(move || {
            wait_for_worker_shutdown(worker, Duration::from_secs(5))
        })
        .await
        .map_err(|err| io::Error::other(format!("gui worker join wait failed: {err}")))?
        .map_err(|err| {
            warn!(error = %err, "gui runtime worker did not shut down cleanly");
            err
        })?;
        worker_result
            .map_err(|err| io::Error::other(format!("gui runtime worker failed: {err}")))?;
        if let Err(err) = gui_result {
            return Err(Box::new(io::Error::other(err.to_string())));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::wait_for_worker_shutdown;
    use std::time::Duration;

    #[test]
    fn wait_for_worker_shutdown_returns_result_before_timeout() {
        let worker = std::thread::spawn(|| Ok::<_, String>(()));

        let result =
            wait_for_worker_shutdown(worker, Duration::from_millis(100)).expect("join succeeds");

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn wait_for_worker_shutdown_times_out_for_stuck_worker() {
        let worker = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            Ok::<_, String>(())
        });

        let err = wait_for_worker_shutdown(worker, Duration::from_millis(5))
            .expect_err("join should time out");

        assert!(err.to_string().contains("timed out"));
    }
}
