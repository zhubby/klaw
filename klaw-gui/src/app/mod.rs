use crate::state::persistence;
use crate::state::{UiAction, UiState, WindowSize};
use crate::theme;
use crate::ui::shell::ShellUi;
use std::time::{Duration, Instant};

const UI_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

pub struct KlawGuiApp {
    state: UiState,
    shell: ShellUi,
    state_dirty: bool,
    last_state_save_at: Instant,
}

impl KlawGuiApp {
    pub fn new(creation_ctx: &eframe::CreationContext<'_>) -> Self {
        let state = persistence::load_ui_state();
        let app = Self {
            state,
            shell: ShellUi::default(),
            state_dirty: false,
            last_state_save_at: Instant::now(),
        };
        theme::install_fonts(&creation_ctx.egui_ctx);
        theme::apply_theme(&creation_ctx.egui_ctx, app.state.theme_mode);
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
            UiAction::ToggleFullscreen => {
                self.state.apply(action);
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.state.fullscreen));
                self.mark_state_dirty();
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
                self.mark_state_dirty();
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
        if persistence::save_ui_state(&self.state).is_ok() {
            self.state_dirty = false;
            self.last_state_save_at = Instant::now();
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
        self.sync_window_size_from_viewport(ctx);
        theme::apply_theme(ctx, self.state.theme_mode);
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
