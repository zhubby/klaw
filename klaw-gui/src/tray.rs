use crate::icon;
use anyhow::Context;
use std::sync::mpsc::{self, Receiver};
use tray_icon::{
    TrayIcon,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
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
    let icon = load_tray_icon()?;
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

fn load_tray_icon() -> anyhow::Result<tray_icon::Icon> {
    icon::tray_icon().context("failed to load embedded tray icon")
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
        MENU_OPEN_KLAW_ID, MENU_OPEN_SETTINGS_ID, MENU_QUIT_KLAW_ID, MENU_SHOW_ABOUT_ID,
        TrayCommand, tray_command_for_menu_id,
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
