use crate::state::UiState;
use crate::theme;
use crate::ui::shell::ShellUi;

pub struct KlawGuiApp {
    state: UiState,
    shell: ShellUi,
}

impl KlawGuiApp {
    pub fn new(creation_ctx: &eframe::CreationContext<'_>) -> Self {
        theme::apply_theme(&creation_ctx.egui_ctx);
        Self {
            state: UiState::default(),
            shell: ShellUi::default(),
        }
    }
}

impl eframe::App for KlawGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let actions = self.shell.render(ctx, &self.state);
        for action in actions {
            self.state.apply(action);
        }
    }
}
