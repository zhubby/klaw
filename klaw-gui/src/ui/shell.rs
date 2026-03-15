use crate::panels::PanelRegistry;
use crate::state::{UiAction, UiState};
use crate::ui::{sidebar, workbench};

#[derive(Default)]
pub struct ShellUi {
    panels: PanelRegistry,
}

impl ShellUi {
    pub fn render(&mut self, ctx: &egui::Context, state: &UiState) -> Vec<UiAction> {
        let mut actions = Vec::new();

        egui::SidePanel::left("klaw-sidebar")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                actions.extend(sidebar::show_sidebar(ui, state));
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            actions.extend(workbench::show_workbench(ui, state, &mut self.panels));
        });

        actions
    }
}
