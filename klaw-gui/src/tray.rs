use anyhow::Context;
use std::sync::mpsc::{self, Receiver};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIcon,
};

const MENU_OPEN_KLAW_ID: &str = "tray.open_klaw";
const MENU_OPEN_SETTINGS_ID: &str = "tray.open_settings";
const MENU_SHOW_ABOUT_ID: &str = "tray.show_about";
const MENU_QUIT_KLAW_ID: &str = "tray.quit_klaw";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenKlaw,
    OpenSettings,
    ShowAbout,
    QuitKlaw,
}

pub struct TrayIntegration {
    #[allow(dead_code)]
    pub tray_icon: TrayIcon,
    pub command_rx: Receiver<TrayCommand>,
}

pub fn install(egui_ctx: &egui::Context) -> anyhow::Result<Option<TrayIntegration>> {
    let Some(icon) = load_tray_icon()? else {
        return Ok(None);
    };
    let menu = build_tray_menu()?;
    let (command_tx, command_rx) = mpsc::channel();
    let repaint_ctx = egui_ctx.clone();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(command) = tray_command_for_menu_id(event.id().0.as_str()) {
            let _ = command_tx.send(command);
            repaint_ctx.request_repaint();
        }
    }));

    let tray_icon = tray_icon::TrayIconBuilder::new()
        .with_tooltip("Klaw")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_menu_on_left_click(true)
        .build()
        .context("failed to create tray icon")?;

    Ok(Some(TrayIntegration {
        tray_icon,
        command_rx,
    }))
}

fn load_tray_icon() -> anyhow::Result<Option<tray_icon::Icon>> {
    let icon_path = tray_icon_path();
    if !icon_path.exists() {
        return Ok(None);
    }

    let image = image::open(&icon_path)
        .with_context(|| format!("failed to load tray icon from {}", icon_path.display()))?
        .into_rgba8();
    let width = image.width();
    let height = image.height();

    tray_icon::Icon::from_rgba(image.into_raw(), width, height)
        .map(Some)
        .map_err(|err| anyhow::anyhow!("failed to convert tray icon image: {err}"))
}

#[cfg(target_os = "macos")]
fn tray_icon_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/icons/logo.iconset/icon_16x16@2x.png")
}

#[cfg(not(target_os = "macos"))]
fn tray_icon_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/icons/logo.iconset/icon_32x32.png")
}

fn build_tray_menu() -> anyhow::Result<Menu> {
    let menu = Menu::new();
    let open_item = MenuItem::with_id(MENU_OPEN_KLAW_ID, "Open Klaw", true, None);
    let settings_item = MenuItem::with_id(MENU_OPEN_SETTINGS_ID, "Setting", true, None);
    let about_item = MenuItem::with_id(MENU_SHOW_ABOUT_ID, "About", true, None);
    let quit_item = MenuItem::with_id(MENU_QUIT_KLAW_ID, "Quit Klaw", true, None);
    let separator = PredefinedMenuItem::separator();

    menu.append_items(&[
        &open_item,
        &settings_item,
        &about_item,
        &separator,
        &quit_item,
    ])
    .context("failed to build tray menu")?;

    Ok(menu)
}

fn tray_command_for_menu_id(menu_id: &str) -> Option<TrayCommand> {
    match menu_id {
        MENU_OPEN_KLAW_ID => Some(TrayCommand::OpenKlaw),
        MENU_OPEN_SETTINGS_ID => Some(TrayCommand::OpenSettings),
        MENU_SHOW_ABOUT_ID => Some(TrayCommand::ShowAbout),
        MENU_QUIT_KLAW_ID => Some(TrayCommand::QuitKlaw),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        tray_command_for_menu_id, TrayCommand, MENU_OPEN_KLAW_ID, MENU_OPEN_SETTINGS_ID,
        MENU_QUIT_KLAW_ID, MENU_SHOW_ABOUT_ID,
    };

    #[test]
    fn tray_menu_ids_map_to_expected_commands() {
        assert_eq!(
            tray_command_for_menu_id(MENU_OPEN_KLAW_ID),
            Some(TrayCommand::OpenKlaw)
        );
        assert_eq!(
            tray_command_for_menu_id(MENU_OPEN_SETTINGS_ID),
            Some(TrayCommand::OpenSettings)
        );
        assert_eq!(
            tray_command_for_menu_id(MENU_SHOW_ABOUT_ID),
            Some(TrayCommand::ShowAbout)
        );
        assert_eq!(
            tray_command_for_menu_id(MENU_QUIT_KLAW_ID),
            Some(TrayCommand::QuitKlaw)
        );
        assert_eq!(tray_command_for_menu_id("tray.unknown"), None);
    }
}
