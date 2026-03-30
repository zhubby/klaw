use crate::runtime_bridge::{RuntimeRequestHandle, begin_set_provider_override_request};
use crate::state::persistence;
use crate::state::{UiAction, UiState, WindowSize};
use crate::theme;
use crate::tray::{self, TrayCommand, TrayIntegration};
use crate::ui::shell::ShellUi;
use crate::{hide_macos_app, show_macos_app};
use std::time::{Duration, Instant};

const UI_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

fn tray_command_ui_action(command: TrayCommand) -> Option<UiAction> {
    match command {
        TrayCommand::OpenKlaw => None,
        TrayCommand::ShowAbout => Some(UiAction::ShowAbout),
        TrayCommand::QuitKlaw => Some(UiAction::QuitApp),
    }
}

pub struct KlawGuiApp {
    state: UiState,
    shell: ShellUi,
    tray: Option<TrayIntegration>,
    state_dirty: bool,
    last_state_save_at: Instant,
    should_quit: bool,
    pending_provider_override: Option<PendingProviderOverride>,
}

struct PendingProviderOverride {
    requested_provider_id: Option<String>,
    request: RuntimeRequestHandle<(String, String)>,
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
            should_quit: false,
            pending_provider_override: None,
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
            UiAction::HideWindow => {
                self.hide_window(ctx);
            }
            UiAction::QuitApp => {
                self.save_state_now();
                self.should_quit = true;
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
                self.sync_persisted_ui_state_from_disk();
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
                if self.pending_provider_override.is_some() {
                    self.shell
                        .show_info("Runtime provider update is already in progress");
                    return;
                }
                let requested_provider_id = provider_id.clone();
                self.shell.set_pending_provider_override(provider_id.clone());
                self.pending_provider_override = Some(PendingProviderOverride {
                    requested_provider_id,
                    request: begin_set_provider_override_request(provider_id),
                });
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

    fn show_window(&self, ctx: &egui::Context) {
        show_macos_app();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        self.save_state_now();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        hide_macos_app();
    }

    fn mark_state_dirty(&mut self) {
        self.state_dirty = true;
    }

    fn save_state_now(&mut self) {
        self.sync_persisted_ui_state_from_disk();
        if persistence::save_ui_state(&self.state).is_ok() {
            self.state_dirty = false;
            self.last_state_save_at = Instant::now();
        }
    }

    fn sync_persisted_ui_state_from_disk(&mut self) {
        let disk_state = persistence::load_ui_state();
        self.state.light_theme = disk_state.light_theme;
        self.state.dark_theme = disk_state.dark_theme;
        self.state.logs_panel = disk_state.logs_panel;
    }

    fn handle_tray_command(&mut self, ctx: &egui::Context, command: TrayCommand) {
        match command {
            TrayCommand::OpenKlaw => {
                self.show_window(ctx);
            }
            TrayCommand::ShowAbout => {
                self.show_window(ctx);
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

    fn poll_pending_actions(&mut self) {
        let Some(pending) = self.pending_provider_override.as_mut() else {
            return;
        };
        let Some(result) = pending.request.try_take_result() else {
            return;
        };

        let requested_provider_id = pending.requested_provider_id.clone();
        self.pending_provider_override = None;
        self.shell.clear_pending_provider_override();

        match result {
            Ok((active_provider, active_model)) => {
                self.shell
                    .set_runtime_provider_override(requested_provider_id.clone());
                self.state
                    .apply(UiAction::SetRuntimeProviderOverride(requested_provider_id));
                self.shell.show_info(format!(
                    "Runtime provider set to '{active_provider}' ({active_model})"
                ));
                self.mark_state_dirty();
            }
            Err(err) => {
                self.shell
                    .set_runtime_provider_override(self.state.runtime_provider_override.clone());
                self.shell
                    .show_error(format!("Failed to update runtime provider: {err}"));
            }
        }
    }
}

impl eframe::App for KlawGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_tray_commands(ctx);
        self.poll_pending_actions();
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
            if self.should_quit {
                self.save_state_now();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_window(ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tray_command_ui_action;
    use crate::state::UiAction;
    use crate::tray::TrayCommand;

    #[test]
    fn tray_about_and_quit_commands_keep_expected_actions() {
        assert_eq!(
            tray_command_ui_action(TrayCommand::ShowAbout),
            Some(UiAction::ShowAbout)
        );
        assert_eq!(
            tray_command_ui_action(TrayCommand::QuitKlaw),
            Some(UiAction::QuitApp)
        );
        assert_eq!(tray_command_ui_action(TrayCommand::OpenKlaw), None);
    }
}
