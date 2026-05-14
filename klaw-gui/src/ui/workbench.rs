use crate::notifications::NotificationCenter;
use crate::panels::{PanelRegistry, RenderCtx};
use crate::settings::current_ui_language;
use crate::state::workbench::{TabId, WorkbenchTab};
use crate::state::{UiAction, UiState};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, NodeIndex, Style, SurfaceIndex};
use klaw_ui_kit::{LocaleDomain, Translator};

pub fn show_workbench(
    ui: &mut egui::Ui,
    state: &mut UiState,
    panels: &mut PanelRegistry,
    notifications: &mut NotificationCenter,
) -> Vec<UiAction> {
    puffin::profile_function!();
    let mut actions = Vec::new();

    if state.workbench.tab_count() == 0 {
        ui.heading("No open tabs");
        ui.label("Use the sidebar to open a module.");
        return actions;
    }

    let is_dark_mode = ui.visuals().dark_mode;
    let translator = Translator::new(LocaleDomain::Gui, current_ui_language());
    let mut style = Style::from_egui(ui.style().as_ref());
    style.tab_bar.show_scroll_bar_on_overflow = false;

    DockArea::new(&mut state.workbench.dock_state)
        .id(egui::Id::new("workbench-dock"))
        .style(style)
        .show_add_buttons(false)
        .show_close_buttons(true)
        .show_leaf_close_all_buttons(false)
        .show_leaf_collapse_buttons(false)
        .show_inside(
            ui,
            &mut WorkbenchTabViewer {
                active_tab: state.workbench.active_tab,
                is_dark_mode,
                light_theme: state.light_theme,
                dark_theme: state.dark_theme,
                panels,
                notifications,
                actions: &mut actions,
                translator,
            },
        );

    actions
}

struct WorkbenchTabViewer<'a> {
    active_tab: Option<TabId>,
    is_dark_mode: bool,
    light_theme: crate::state::LightThemePreset,
    dark_theme: crate::state::DarkThemePreset,
    panels: &'a mut PanelRegistry,
    notifications: &'a mut NotificationCenter,
    actions: &'a mut Vec<UiAction>,
    translator: Translator,
}

impl egui_dock::TabViewer for WorkbenchTabViewer<'_> {
    type Tab = WorkbenchTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        self.translator.text(tab.menu.i18n_key()).into()
    }

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("workbench-tab", tab.menu.id_key()))
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        let title = self.translator.text(tab.menu.i18n_key());
        let ctx = RenderCtx {
            menu: tab.menu,
            tab_title: title.as_str(),
            is_dark_mode: self.is_dark_mode,
            light_theme: self.light_theme,
            dark_theme: self.dark_theme,
        };
        puffin::profile_scope!("workbench_panel_shell");
        self.panels.render_for(ui, &ctx, self.notifications);
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        let tab_id = tab.id();
        if response.clicked() && self.active_tab != Some(tab_id) {
            self.actions.push(UiAction::ActivateTab(tab_id));
        }
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        self.actions.push(UiAction::CloseTab(tab.id()));
        OnCloseResponse::Ignore
    }

    fn on_add(&mut self, _surface: SurfaceIndex, _node: NodeIndex) {}

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }
}
