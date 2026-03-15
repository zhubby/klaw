use crate::domain::menu::WorkbenchMenu;
use crate::state::{UiAction, UiState};

pub fn show_sidebar(ui: &mut egui::Ui, state: &UiState) -> Vec<UiAction> {
    let mut actions = Vec::new();

    ui.heading("Klaw Workbench");
    ui.label("Modules");
    ui.separator();

    for menu in WorkbenchMenu::ALL {
        let is_active = state.workbench.active_tab.is_some_and(|id| id.menu == menu);
        let label = format!("{} {}", menu.icon(), menu.title());
        if ui.selectable_label(is_active, label).clicked() {
            actions.push(UiAction::OpenMenu(menu));
        }
    }

    actions
}
