use crate::state::{UiAction, UiState};
use crate::theme;
use crate::ui::shell::ShellUi;

pub struct KlawGuiApp {
    state: UiState,
    shell: ShellUi,
}

impl KlawGuiApp {
    pub fn new(creation_ctx: &eframe::CreationContext<'_>) -> Self {
        let app = Self {
            state: UiState::default(),
            shell: ShellUi::default(),
        };
        theme::install_fonts(&creation_ctx.egui_ctx);
        theme::apply_theme(&creation_ctx.egui_ctx, app.state.theme_mode);
        app
    }

    fn handle_action(&mut self, ctx: &egui::Context, action: UiAction) {
        match action {
            UiAction::CloseWindow => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            UiAction::ToggleFullscreen => {
                self.state.apply(action);
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.state.fullscreen));
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
            UiAction::CycleTheme => {
                self.state.apply(action);
                theme::apply_theme(ctx, self.state.theme_mode);
            }
            UiAction::ShowAbout
            | UiAction::HideAbout
            | UiAction::OpenMenu(_)
            | UiAction::ActivateTab(_)
            | UiAction::CloseTab(_) => {
                self.state.apply(action);
            }
        }
    }
}

impl eframe::App for KlawGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::apply_theme(ctx, self.state.theme_mode);
        let actions = self.shell.render(ctx, &self.state);
        for action in actions {
            self.handle_action(ctx, action);
        }
    }
}
