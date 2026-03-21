mod app;
mod domain;
mod notifications;
mod panels;
mod runtime_bridge;
mod settings;
mod state;
mod theme;
mod time_format;
mod tray;
mod ui;
pub mod widgets;

pub use domain::menu::WorkbenchMenu;
pub use panels::{PanelRenderer, RenderCtx};
pub use runtime_bridge::{
    clear_log_receiver, clear_runtime_command_sender, drain_log_chunks, install_log_receiver,
    install_runtime_command_sender, request_env_check, request_gateway_status,
    request_restart_gateway, request_run_cron_now, request_set_gateway_enabled,
    request_sync_channels, GatewayStatusSnapshot, RuntimeCommand,
};
pub use state::workbench::{TabId, WorkbenchState, WorkbenchTab};
pub use state::UiAction;

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
    if let Some(icon) = load_macos_app_icon() {
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
fn load_macos_app_icon() -> Option<egui::IconData> {
    use objc2::ClassType as _;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSString};

    let Some(mtm) = MainThreadMarker::new() else {
        return None;
    };

    let icon_path = format!("{}/assets/icons/logo.icns", env!("CARGO_MANIFEST_DIR"));
    let icon_path = NSString::from_str(&icon_path);
    let Some(icon) = (unsafe { NSImage::initWithContentsOfFile(NSImage::alloc(), &icon_path) })
    else {
        return None;
    };

    let app = NSApplication::sharedApplication(mtm);
    unsafe {
        app.setApplicationIconImage(Some(&icon));
    }

    let tiff = unsafe { icon.TIFFRepresentation() }?;
    macos_nsdata_to_vec(&tiff).and_then(|bytes| {
        let image = image::load_from_memory(&bytes).ok()?.into_rgba8();
        Some(egui::IconData {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        })
    })
}

#[cfg(target_os = "macos")]
fn macos_nsdata_to_vec(data: &objc2_foundation::NSData) -> Option<Vec<u8>> {
    use std::ptr::NonNull;

    let len = data.length();
    if len == 0 {
        return None;
    }

    let mut bytes = vec![0_u8; len];
    let ptr = NonNull::new(bytes.as_mut_ptr().cast())?;
    unsafe {
        data.getBytes_length(ptr, len);
    }
    Some(bytes)
}
