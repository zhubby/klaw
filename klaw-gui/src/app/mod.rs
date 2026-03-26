use crate::domain::menu::WorkbenchMenu;
use crate::runtime_bridge::request_set_provider_override;
use crate::state::persistence;
use crate::state::{UiAction, UiState, WindowSize};
use crate::theme;
use crate::tray::{self, TrayCommand, TrayIntegration};
use crate::ui::shell::ShellUi;
use std::time::{Duration, Instant};

const UI_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

fn tray_command_ui_action(command: TrayCommand) -> Option<UiAction> {
    match command {
        TrayCommand::OpenKlaw => None,
        TrayCommand::OpenSettings => Some(UiAction::OpenMenu(WorkbenchMenu::Setting)),
        TrayCommand::ShowAbout => Some(UiAction::ShowAbout),
        TrayCommand::QuitKlaw => Some(UiAction::CloseWindow),
    }
}

pub struct KlawGuiApp {
    state: UiState,
    shell: ShellUi,
    tray: Option<TrayIntegration>,
    state_dirty: bool,
    last_state_save_at: Instant,
}

impl KlawGuiApp {
    pub fn new(creation_ctx: &eframe::CreationContext<'_>) -> Self {
        let state = persistence::load_ui_state();
        let app = Self {
            state,
            shell: ShellUi::default(),
            tray: tray::install(&creation_ctx.egui_ctx).ok().flatten(),
            state_dirty: false,
            last_state_save_at: Instant::now(),
        };
        theme::install_fonts(&creation_ctx.egui_ctx);
        theme::apply_theme(&creation_ctx.egui_ctx, &app.state);
        creation_ctx
            .egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(app.state.fullscreen));
        if let Some(size) = app.state.window_size {
            creation_ctx
                .egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    size.width as f32,
                    size.height as f32,
                )));
        }
        app
    }

    fn handle_action(&mut self, ctx: &egui::Context, action: UiAction) {
        match action {
            UiAction::CloseWindow => {
                self.save_state_now();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            UiAction::ForcePersistLayout => {
                self.save_state_now();
            }
            UiAction::ToggleFullscreen => {
                self.state.apply(action);
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.state.fullscreen));
                self.mark_state_dirty();
            }
            UiAction::SetThemeMode(theme_mode) => {
                self.sync_theme_presets_from_disk();
                self.state.apply(UiAction::SetThemeMode(theme_mode));
                theme::apply_theme(ctx, &self.state);
                self.save_state_now();
            }
            UiAction::MinimizeWindow => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            UiAction::ZoomWindow => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            }
            UiAction::StartWindowDrag => {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            UiAction::SetRuntimeProviderOverride(provider_id) => {
                match request_set_provider_override(provider_id.clone()) {
                    Ok((active_provider, active_model)) => {
                        self.state
                            .apply(UiAction::SetRuntimeProviderOverride(provider_id));
                        self.shell.show_info(format!(
                            "Runtime provider set to '{active_provider}' ({active_model})"
                        ));
                        self.mark_state_dirty();
                    }
                    Err(err) => {
                        self.shell
                            .show_error(format!("Failed to update runtime provider: {err}"));
                    }
                }
            }
            UiAction::ShowAbout
            | UiAction::HideAbout
            | UiAction::OpenMenu(_)
            | UiAction::ActivateTab(_)
            | UiAction::CloseTab(_) => {
                self.state.apply(action);
                self.mark_state_dirty();
            }
        }
    }

    fn mark_state_dirty(&mut self) {
        self.state_dirty = true;
    }

    fn save_state_now(&mut self) {
        self.sync_theme_presets_from_disk();
        if persistence::save_ui_state(&self.state).is_ok() {
            self.state_dirty = false;
            self.last_state_save_at = Instant::now();
        }
    }

    fn sync_theme_presets_from_disk(&mut self) {
        let disk_state = persistence::load_ui_state();
        self.state.light_theme = disk_state.light_theme;
        self.state.dark_theme = disk_state.dark_theme;
    }

    fn handle_tray_command(&mut self, ctx: &egui::Context, command: TrayCommand) {
        match command {
            TrayCommand::OpenKlaw => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            TrayCommand::OpenSettings => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            TrayCommand::ShowAbout => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            TrayCommand::QuitKlaw => {}
        }

        if let Some(action) = tray_command_ui_action(command) {
            self.handle_action(ctx, action);
        }
    }

    fn drain_tray_commands(&mut self, ctx: &egui::Context) {
        let mut pending = Vec::new();
        if let Some(tray) = &self.tray {
            while let Ok(command) = tray.command_rx.try_recv() {
                pending.push(command);
            }
        }

        for command in pending {
            self.handle_tray_command(ctx, command);
        }
    }

    fn sync_fullscreen_from_viewport(&mut self, ctx: &egui::Context) {
        let fullscreen = ctx.input(|input| input.viewport().fullscreen);
        let Some(fullscreen) = fullscreen else {
            return;
        };
        if self.state.fullscreen != fullscreen {
            self.state.fullscreen = fullscreen;
            self.mark_state_dirty();
        }
    }

    fn sync_window_size_from_viewport(&mut self, ctx: &egui::Context) {
        if self.state.fullscreen {
            return;
        }
        let size = ctx.input(|input| input.viewport().inner_rect.map(|rect| rect.size()));
        let Some(size) = size else {
            return;
        };
        let width = size.x.max(1.0).round() as u32;
        let height = size.y.max(1.0).round() as u32;
        let next = Some(WindowSize { width, height });
        if self.state.window_size != next {
            self.state.window_size = next;
            self.mark_state_dirty();
        }
    }
}

impl eframe::App for KlawGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_tray_commands(ctx);
        self.sync_fullscreen_from_viewport(ctx);
        self.sync_window_size_from_viewport(ctx);
        let actions = self.shell.render(ctx, &self.state);
        for action in actions {
            self.handle_action(ctx, action);
        }

        let should_flush =
            self.state_dirty && self.last_state_save_at.elapsed() >= UI_STATE_SAVE_DEBOUNCE;
        if should_flush {
            self.save_state_now();
        }

        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested {
            self.save_state_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tray_command_ui_action;
    use crate::domain::menu::WorkbenchMenu;
    use crate::state::UiAction;
    use crate::tray::TrayCommand;

    #[test]
    fn tray_settings_command_opens_setting_menu() {
        assert_eq!(
            tray_command_ui_action(TrayCommand::OpenSettings),
            Some(UiAction::OpenMenu(WorkbenchMenu::Setting))
        );
    }

    #[test]
    fn tray_about_and_quit_commands_keep_expected_actions() {
        assert_eq!(
            tray_command_ui_action(TrayCommand::ShowAbout),
            Some(UiAction::ShowAbout)
        );
        assert_eq!(
            tray_command_ui_action(TrayCommand::QuitKlaw),
            Some(UiAction::CloseWindow)
        );
        assert_eq!(tray_command_ui_action(TrayCommand::OpenKlaw), None);
    }
}
