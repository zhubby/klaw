mod app;
mod autostart;
mod domain;
mod icon;
mod notifications;
mod panels;
mod runtime_bridge;
mod settings;
mod state;
mod sync_runtime;
mod theme;
mod time_format;
mod tray;
mod ui;
mod voice_test;
pub mod widgets;

pub use domain::menu::WorkbenchMenu;
pub use panels::{PanelRenderer, RenderCtx};
pub use runtime_bridge::{
    AcpPromptEvent, GatewayStatusSnapshot, ProviderRuntimeSnapshot, RuntimeCommand,
    clear_log_receiver, clear_runtime_command_sender, drain_log_chunks, install_log_receiver,
    install_runtime_command_sender, request_acp_status, request_env_check,
    request_execute_acp_prompt_stream, request_gateway_status, request_mcp_status,
    request_provider_status, request_restart_gateway, request_run_cron_now,
    request_run_heartbeat_now, request_set_gateway_enabled, request_set_tailscale_mode,
    request_start_gateway, request_stop_acp_prompt, request_sync_acp, request_sync_channels,
    request_sync_mcp, request_sync_providers, request_sync_tools, request_tool_definitions,
};
pub use state::UiAction;
pub use state::workbench::{TabId, WorkbenchState, WorkbenchTab};

pub fn run() -> anyhow::Result<()> {
    let viewport = configure_platform_viewport(
        egui::ViewportBuilder::default()
            .with_title("Klaw")
            .with_decorations(false)
            .with_titlebar_shown(false)
            .with_titlebar_buttons_shown(false)
            .with_fullsize_content_view(true),
    );
    let native_options = eframe::NativeOptions {
        viewport,
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Klaw",
        native_options,
        Box::new(|creation_ctx| Ok(Box::new(app::KlawGuiApp::new(creation_ctx)))),
    )
    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_platform_viewport(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    install_macos_app_icon();
    if let Some(icon) = icon::viewport_icon() {
        viewport.with_icon(icon)
    } else {
        viewport
    }
}

#[cfg(not(target_os = "macos"))]
fn configure_platform_viewport(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    viewport
}

#[cfg(target_os = "macos")]
fn install_macos_app_icon() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(icon) = icon::application_icon_image() else {
        return;
    };

    let app = NSApplication::sharedApplication(mtm);
    unsafe {
        app.setApplicationIconImage(Some(&icon));
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn show_macos_app() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let app = NSApplication::sharedApplication(mtm);
    unsafe {
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.activate();
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_macos_app() {}

#[cfg(target_os = "macos")]
pub(crate) fn hide_macos_app() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_macos_app() {}
