use clap::Args;
use klaw_config::AppConfig;
use std::{io, sync::Arc};
use tokio::sync::watch;

use super::dingtalk_runtime::{spawn_enabled_channels, wait_for_channels_shutdown};
use super::startup_display::print_startup_banner;
use crate::commands::signal::shutdown_signal;
use crate::runtime::service_loop::{BackgroundServiceConfig, BackgroundServices};
use crate::runtime::{
    build_runtime_bundle, finalize_startup_report, reload_runtime_skills_prompt,
    shutdown_runtime_bundle, SharedChannelRuntime,
};
use tracing::{info, warn};

#[derive(Debug, Args)]
pub struct GuiCommand {}

impl GuiCommand {
    pub async fn run(self, config: Arc<AppConfig>) -> Result<(), Box<dyn std::error::Error>> {
        let (startup_tx, startup_rx) = std::sync::mpsc::channel::<Result<_, String>>();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (runtime_cmd_tx, runtime_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let config_for_thread = config.as_ref().clone();
        let dingtalk_configs = config.channels.dingtalk.clone();

        let worker = std::thread::Builder::new()
            .name("klaw-gui-runtime".to_string())
            .spawn(move || -> Result<(), String> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;

                runtime.block_on(async move {
                    let mut runtime = build_runtime_bundle(&config_for_thread)
                        .await
                        .map_err(|err| err.to_string())?;
                    let startup_report = finalize_startup_report(&mut runtime)
                        .await
                        .map_err(|err| err.to_string())?;
                    let _ = startup_tx.send(Ok(startup_report));

                    let runtime = Arc::new(runtime);
                    let background = Arc::new(BackgroundServices::new(
                        runtime.as_ref(),
                        BackgroundServiceConfig::from_app_config(&config_for_thread),
                    ));
                    let adapter = Arc::new(SharedChannelRuntime::new(runtime.clone(), background));

                    let local = tokio::task::LocalSet::new();
                    local
                        .run_until(async move {
                            let mut shutdown_rx = shutdown_rx;
                            let mut runtime_cmd_rx = runtime_cmd_rx;
                            let mut runtime_cmd_open = true;
                            let mut dingtalk_handles = spawn_enabled_channels(
                                dingtalk_configs,
                                Arc::clone(&adapter),
                                shutdown_rx.clone(),
                            );

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

                            wait_for_channels_shutdown(&mut dingtalk_handles).await;
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
        let _ = shutdown_tx.send(true);
        let worker_result = worker
            .join()
            .map_err(|err| io::Error::other(format!("gui runtime worker panicked: {err:?}")))?;
        worker_result
            .map_err(|err| io::Error::other(format!("gui runtime worker failed: {err}")))?;
        if let Err(err) = gui_result {
            return Err(Box::new(io::Error::other(err.to_string())));
        }
        Ok(())
    }
}
