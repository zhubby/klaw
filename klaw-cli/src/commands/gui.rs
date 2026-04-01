use clap::Args;
use klaw_acp::AcpConfigSnapshot;
use klaw_channel::{ChannelConfigSnapshot, ChannelManager};
use klaw_config::AppConfig;
use klaw_llm::ToolDefinition;
use klaw_mcp::McpConfigSnapshot;
use std::{io, sync::Arc, time::Duration};
use tokio::sync::{Mutex as AsyncMutex, watch};

use super::startup_display::print_startup_banner;
use crate::commands::signal::shutdown_signal;
use crate::runtime::gateway_manager::GatewayManager;
use crate::runtime::service_loop::{BackgroundServiceConfig, BackgroundServices};
use crate::runtime::{
    SharedChannelRuntime, build_channel_driver_factory, build_runtime_bundle,
    finalize_startup_report, reload_runtime_skills_prompt, set_runtime_provider_override,
    shutdown_runtime_bundle, sync_runtime_providers, sync_runtime_tools,
};
use klaw_config::ConfigStore;
use tracing::{info, warn};

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

fn provider_runtime_snapshot(
    runtime: &crate::runtime::RuntimeBundle,
) -> klaw_gui::ProviderRuntimeSnapshot {
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

fn tool_definitions(runtime: &crate::runtime::RuntimeBundle) -> Vec<ToolDefinition> {
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
                    let mut runtime = match build_runtime_bundle(&config_for_thread).await {
                        Ok(runtime) => runtime,
                        Err(err) => {
                            let err = err.to_string();
                            let _ = startup_tx.send(Err(err.clone()));
                            return Err(err);
                        }
                    };
                    let startup_report = match finalize_startup_report(&mut runtime).await {
                        Ok(report) => report,
                        Err(err) => {
                            let err = err.to_string();
                            let _ = startup_tx.send(Err(err.clone()));
                            return Err(err);
                        }
                    };
                    let _ = startup_tx.send(Ok(startup_report));

                    let runtime = Arc::new(runtime);
                    let background = Arc::new(BackgroundServices::new(
                        runtime.as_ref(),
                        BackgroundServiceConfig::from_app_config(&config_for_thread),
                    ));
                    let gateway_manager = Arc::new(AsyncMutex::new(GatewayManager::new(
                        &config_for_thread,
                        runtime.clone(),
                    )));
                    if let Err(err) = gateway_manager
                        .lock()
                        .await
                        .start_if_enabled(&config_for_thread)
                        .await
                    {
                        warn!(error = %err, "failed to start gateway for gui runtime");
                    }
                    let adapter = Arc::new(SharedChannelRuntime::new(
                        runtime.clone(),
                        Arc::clone(&background),
                    ));

                    let local = tokio::task::LocalSet::new();
                    local
                        .run_until(async move {
                            let mut shutdown_rx = shutdown_rx;
                            let mut runtime_cmd_rx = runtime_cmd_rx;
                            let mut runtime_cmd_open = true;
                            let mut active_acp_prompt_cancel: Option<watch::Sender<bool>> = None;
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
                                                                    let Some((kind_raw, id)) = instance_key.split_once(':') else {
                                                                        let _ = response.send(Err(format!("invalid channel instance key '{}'", instance_key)));
                                                                        return;
                                                                    };
                                                                    let kind = match kind_raw {
                                                                        "dingtalk" => klaw_channel::ChannelKind::Dingtalk,
                                                                        "telegram" => klaw_channel::ChannelKind::Telegram,
                                                                        "feishu" => klaw_channel::ChannelKind::Feishu,
                                                                        _ => {
                                                                            let _ = response.send(Err(format!("invalid channel kind '{}'", kind_raw)));
                                                                            return;
                                                                        }
                                                                    };
                                                                    let key = klaw_channel::ChannelInstanceKey::new(kind, id);
                                                                    channel_manager
                                                                        .lock()
                                                                        .await
                                                                        .restart_channel(&key, &channel_snapshot)
                                                                        .await
                                                                }
                                                                Err(err) => Err(err),
                                                            }
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
                                                let manager = Arc::clone(&acp_manager);
                                                tokio::task::spawn_local(async move {
                                                    tracing::debug!(
                                                        agent = %agent_id,
                                                        working_directory = ?working_directory,
                                                        timeout_seconds,
                                                        prompt_len = prompt.len(),
                                                        "gui requested acp test prompt"
                                                    );
                                                    let timeout = timeout_seconds.map(std::time::Duration::from_secs);
                                                    let execution_config = {
                                                        let guard = manager.lock().await;
                                                        guard.agent_execution_config(&agent_id)
                                                    };
                                                    let result = match execution_config {
                                                        Ok((config, startup_timeout)) => {
                                                            let chunk_events = events.clone();
                                                            let sink = Arc::new(move |update: klaw_acp::AcpPromptUpdate| {
                                                                let chunk = match update {
                                                                    klaw_acp::AcpPromptUpdate::AnswerChunk(text) => text,
                                                                    klaw_acp::AcpPromptUpdate::ThoughtChunk(text) => {
                                                                        format!("\n[thought] {text}\n")
                                                                    }
                                                                    klaw_acp::AcpPromptUpdate::ToolUpdate(text) => {
                                                                        format!("\n[{text}]\n")
                                                                    }
                                                                };
                                                                let _ = chunk_events.send(klaw_gui::AcpPromptEvent::Chunk(chunk));
                                                            });
                                                            klaw_acp::AcpManager::execute_prompt_with_config_stream(
                                                                config,
                                                                startup_timeout,
                                                                &prompt,
                                                                working_directory.as_deref(),
                                                                timeout,
                                                                Some(sink),
                                                                Some(cancel_rx),
                                                            )
                                                            .await
                                                        }
                                                        Err(err) => Err(err),
                                                    };
                                                    match result {
                                                        Ok(final_output) => {
                                                            let _ = events.send(klaw_gui::AcpPromptEvent::Completed {
                                                                final_output,
                                                            });
                                                        }
                                                        Err(klaw_acp::AcpExecutionError::Cancelled { .. }) => {
                                                            let _ = events.send(klaw_gui::AcpPromptEvent::Stopped);
                                                        }
                                                        Err(err) => {
                                                            let _ = events.send(klaw_gui::AcpPromptEvent::Failed(err.to_string()));
                                                        }
                                                    }
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::StopAcpPrompt { response }) => {
                                                let result = match active_acp_prompt_cancel.take() {
                                                    Some(cancel) => cancel
                                                        .send(true)
                                                        .map_err(|_| "acp prompt is no longer running".to_string()),
                                                    None => Err("no ACP test prompt is currently running".to_string()),
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
                                            Some(klaw_gui::RuntimeCommand::GetEnvCheck { response }) => {
                                                let env_check = runtime.env_check.clone();
                                                let _ = response.send(env_check);
                                            }
                                            Some(klaw_gui::RuntimeCommand::GetGatewayStatus { response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                tokio::task::spawn_local(async move {
                                                    let mut gateway_manager = gateway_manager.lock().await;
                                                    if let Err(err) = gateway_manager.refresh_from_store() {
                                                        warn!(error = %err, "failed to refresh gateway config metadata");
                                                    }
                                                    let _ = response.send(gateway_manager.snapshot());
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::StartGateway { response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = gateway_manager.lock().await.start_from_store().await;
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::SetGatewayEnabled { enabled, response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = gateway_manager.lock().await.set_enabled(enabled).await;
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::RestartGateway { response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = gateway_manager.lock().await.restart_from_store().await;
                                                    let _ = response.send(result);
                                                });
                                            }
                                            Some(klaw_gui::RuntimeCommand::SetTailscaleMode { mode, response }) => {
                                                let gateway_manager = Arc::clone(&gateway_manager);
                                                tokio::task::spawn_local(async move {
                                                    let result = gateway_manager.lock().await.set_tailscale_mode(mode).await;
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
