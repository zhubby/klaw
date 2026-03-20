use crate::notifications::NotificationCenter;
use crate::panels::{PanelRegistry, RenderCtx};
use crate::state::{UiAction, UiState};
use egui::scroll_area::ScrollBarVisibility;

pub fn show_workbench(
    ui: &mut egui::Ui,
    state: &UiState,
    panels: &mut PanelRegistry,
    notifications: &mut NotificationCenter,
) -> Vec<UiAction> {
    let mut actions = Vec::new();

    egui::ScrollArea::horizontal()
        .id_salt("workbench-tab-strip")
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for tab in &state.workbench.tabs {
                    let is_active = state.workbench.active_tab == Some(tab.id);
                    let tab_button = ui.selectable_label(is_active, tab.title.as_str());
                    if tab_button.clicked() {
                        actions.push(UiAction::ActivateTab(tab.id));
                    }
                    if tab.closable && ui.small_button("x").clicked() {
                        actions.push(UiAction::CloseTab(tab.id));
                    }
                    ui.separator();
                }
            });
        });

    ui.separator();

    if let Some(active) = state.workbench.active_tab() {
        let ctx = RenderCtx {
            menu: active.menu,
            tab_title: active.title.as_str(),
        };
        panels.render_for(ui, &ctx, notifications);
    } else {
        ui.heading("No open tabs");
        ui.label("Use the sidebar to open a module.");
    }

    actions
}
