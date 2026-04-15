use crate::state::UiState;

pub fn apply_theme(ctx: &egui::Context, state: &UiState) {
    klaw_ui_kit::apply_theme(ctx, state.theme_mode, state.light_theme, state.dark_theme);
}
