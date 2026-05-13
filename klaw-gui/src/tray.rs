use crate::icon;
use anyhow::Context;
use std::sync::mpsc::{self, Receiver};
#[cfg(target_os = "linux")]
use std::thread;
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

const MENU_SHOW_ABOUT_ID: &str = "tray.show_about";
const MENU_SHOW_KLAW_ID: &str = "tray.show_klaw";
const MENU_QUIT_KLAW_ID: &str = "tray.quit_klaw";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    OpenKlaw,
    ShowAbout,
    QuitKlaw,
}

pub struct TrayIntegration {
    #[cfg(not(target_os = "linux"))]
    #[allow(dead_code)]
    pub tray_icon: TrayIcon,
    pub command_rx: Receiver<TrayCommand>,
}

#[cfg(target_os = "linux")]
pub fn install(egui_ctx: &egui::Context) -> anyhow::Result<Option<TrayIntegration>> {
    let icon = load_tray_icon()?;
    let (command_tx, command_rx) = mpsc::channel();
    let (init_tx, init_rx) = mpsc::channel();
    let repaint_ctx = egui_ctx.clone();

    thread::Builder::new()
        .name("klaw-linux-tray".to_string())
        .spawn(move || {
            if let Err(err) = gtk::init() {
                let _ = init_tx.send(Err(anyhow::anyhow!("failed to initialize GTK: {err}")));
                return;
            }

            install_event_handlers(command_tx, repaint_ctx);

            let tray_result = build_tray_menu()
                .and_then(|menu| build_tray_icon(icon, menu))
                .map(|_tray_icon| {
                    let _ = init_tx.send(Ok(()));
                    gtk::main();
                });

            if let Err(err) = tray_result {
                let _ = init_tx.send(Err(err));
            }
        })
        .context("failed to spawn Linux tray thread")?;

    init_rx
        .recv()
        .context("Linux tray thread exited before initialization")??;

    Ok(Some(TrayIntegration { command_rx }))
}

#[cfg(not(target_os = "linux"))]
pub fn install(egui_ctx: &egui::Context) -> anyhow::Result<Option<TrayIntegration>> {
    let icon = load_tray_icon()?;
    let menu = build_tray_menu()?;
    let (command_tx, command_rx) = mpsc::channel();
    install_event_handlers(command_tx, egui_ctx.clone());
    let tray_icon = build_tray_icon(icon, menu)?;

    Ok(Some(TrayIntegration {
        tray_icon,
        command_rx,
    }))
}

fn install_event_handlers(command_tx: mpsc::Sender<TrayCommand>, repaint_ctx: egui::Context) {
    let tray_click_tx = command_tx.clone();
    let tray_click_repaint_ctx = repaint_ctx.clone();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(command) = tray_command_for_menu_id(event.id().0.as_str()) {
            let _ = command_tx.send(command);
            repaint_ctx.request_repaint();
        }
    }));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let Some(command) = tray_command_for_icon_event(event) {
            let _ = tray_click_tx.send(command);
            tray_click_repaint_ctx.request_repaint();
        }
    }));
}

fn build_tray_icon(icon: tray_icon::Icon, menu: Menu) -> anyhow::Result<TrayIcon> {
    tray_icon::TrayIconBuilder::new()
        .with_tooltip("Klaw")
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_icon_as_template(true)
        .with_menu_on_left_click(false)
        .build()
        .context("failed to create tray icon")
}

fn load_tray_icon() -> anyhow::Result<tray_icon::Icon> {
    icon::tray_icon().context("failed to load embedded tray icon")
}

fn build_tray_menu() -> anyhow::Result<Menu> {
    let menu = Menu::new();
    let show_item = MenuItem::with_id(MENU_SHOW_KLAW_ID, "Show Klaw", true, None);
    let about_item = MenuItem::with_id(MENU_SHOW_ABOUT_ID, "About", true, None);
    let quit_item = MenuItem::with_id(MENU_QUIT_KLAW_ID, "Quit Klaw", true, None);
    let separator = PredefinedMenuItem::separator();

    menu.append_items(&[&show_item, &about_item, &separator, &quit_item])
        .context("failed to build tray menu")?;

    Ok(menu)
}

fn tray_command_for_menu_id(menu_id: &str) -> Option<TrayCommand> {
    match menu_id {
        MENU_SHOW_KLAW_ID => Some(TrayCommand::OpenKlaw),
        MENU_SHOW_ABOUT_ID => Some(TrayCommand::ShowAbout),
        MENU_QUIT_KLAW_ID => Some(TrayCommand::QuitKlaw),
        _ => None,
    }
}

fn tray_command_for_icon_event(event: TrayIconEvent) -> Option<TrayCommand> {
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => Some(TrayCommand::OpenKlaw),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MENU_QUIT_KLAW_ID, MENU_SHOW_ABOUT_ID, MENU_SHOW_KLAW_ID, TrayCommand,
        tray_command_for_icon_event, tray_command_for_menu_id,
    };
    use tray_icon::dpi::{PhysicalPosition, PhysicalSize};
    use tray_icon::{MouseButton, MouseButtonState, Rect, TrayIconEvent, TrayIconId};

    #[test]
    fn tray_menu_ids_map_to_expected_commands() {
        assert_eq!(
            tray_command_for_menu_id(MENU_SHOW_KLAW_ID),
            Some(TrayCommand::OpenKlaw)
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

    #[test]
    fn left_click_release_opens_klaw() {
        let event = TrayIconEvent::Click {
            id: TrayIconId::new("klaw"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: Rect {
                position: PhysicalPosition::new(0.0, 0.0),
                size: PhysicalSize::new(16_u32, 16_u32),
            },
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
        };

        assert_eq!(
            tray_command_for_icon_event(event),
            Some(TrayCommand::OpenKlaw)
        );
    }

    #[test]
    fn non_left_click_release_does_not_open_klaw() {
        let event = TrayIconEvent::Click {
            id: TrayIconId::new("klaw"),
            position: PhysicalPosition::new(0.0, 0.0),
            rect: Rect {
                position: PhysicalPosition::new(0.0, 0.0),
                size: PhysicalSize::new(16_u32, 16_u32),
            },
            button: MouseButton::Right,
            button_state: MouseButtonState::Up,
        };

        assert_eq!(tray_command_for_icon_event(event), None);
    }
}
