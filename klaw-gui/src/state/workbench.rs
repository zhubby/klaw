use crate::domain::menu::WorkbenchMenu;
use crate::state::UiAction;
use egui_dock::{DockState, Node, SurfaceIndex, TabIndex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabId {
    pub menu: WorkbenchMenu,
}

impl TabId {
    pub const fn from_menu(menu: WorkbenchMenu) -> Self {
        Self { menu }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkbenchTab {
    pub menu: WorkbenchMenu,
}

impl WorkbenchTab {
    pub const fn from_menu(menu: WorkbenchMenu) -> Self {
        Self { menu }
    }

    pub const fn id(self) -> TabId {
        TabId::from_menu(self.menu)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchState {
    pub dock_state: DockState<WorkbenchTab>,
    pub active_tab: Option<TabId>,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        Self::new_with_default(WorkbenchMenu::Profile)
    }
}

impl WorkbenchState {
    pub fn new_with_default(menu: WorkbenchMenu) -> Self {
        let tab = WorkbenchTab::from_menu(menu);
        Self {
            dock_state: DockState::new(vec![tab]),
            active_tab: Some(tab.id()),
        }
    }

    pub fn apply(&mut self, action: UiAction) {
        match action {
            UiAction::OpenMenu(menu) => self.open_or_activate(menu),
            UiAction::ActivateTab(tab_id) => {
                self.activate(tab_id);
            }
            UiAction::CloseTab(tab_id) => self.close(tab_id),
            UiAction::SetRuntimeProviderOverride(_)
            | UiAction::SetThemeMode(_)
            | UiAction::HideWindow
            | UiAction::QuitApp
            | UiAction::ForcePersistLayout
            | UiAction::ToggleFullscreen
            | UiAction::MinimizeWindow
            | UiAction::ZoomWindow
            | UiAction::StartWindowDrag
            | UiAction::StartWindowResize(_)
            | UiAction::ShowAbout
            | UiAction::HideAbout => {}
        }
    }

    pub fn tab_count(&self) -> usize {
        self.dock_state.iter_all_tabs().count()
    }

    pub fn active_tab(&self) -> Option<WorkbenchTab> {
        let active_id = self.active_tab?;
        self.dock_state
            .iter_all_tabs()
            .find_map(|(_, tab)| (tab.id() == active_id).then_some(*tab))
    }

    pub fn sanitize_for_persistence(&mut self) {
        for (_, node) in self.dock_state.iter_all_nodes_mut() {
            match node {
                Node::Leaf(leaf) => {
                    if !rect_is_finite(leaf.rect) {
                        leaf.rect = egui::Rect::ZERO;
                    }
                    if !rect_is_finite(leaf.viewport) {
                        leaf.viewport = egui::Rect::ZERO;
                    }
                    if !leaf.scroll.is_finite() {
                        leaf.scroll = 0.0;
                    }
                }
                Node::Horizontal(split) | Node::Vertical(split) => {
                    if !rect_is_finite(split.rect) {
                        split.rect = egui::Rect::ZERO;
                    }
                    if !split.fraction.is_finite() {
                        split.fraction = 0.5;
                    }
                }
                Node::Empty => {}
            }
        }
    }

    fn open_or_activate(&mut self, menu: WorkbenchMenu) {
        let target = TabId::from_menu(menu);
        if self.activate(target) {
            return;
        }

        let tab = WorkbenchTab::from_menu(menu);
        self.dock_state.push_to_focused_leaf(tab);
        self.active_tab = Some(tab.id());
    }

    fn activate(&mut self, tab_id: TabId) -> bool {
        let Some((surface, node, tab)) = self.find_tab_position(tab_id.menu) else {
            return false;
        };

        self.dock_state
            .set_focused_node_and_surface((surface, node));
        self.dock_state.set_active_tab((surface, node, tab));
        self.active_tab = Some(tab_id);
        true
    }

    fn close(&mut self, tab_id: TabId) {
        let Some(position) = self.find_tab_position(tab_id.menu) else {
            if self.active_tab == Some(tab_id) {
                self.active_tab = self.first_tab_id();
            }
            return;
        };

        self.dock_state.remove_tab(position);

        if self.active_tab == Some(tab_id) {
            self.active_tab = self.first_tab_id();
        }
    }

    fn first_tab_id(&self) -> Option<TabId> {
        self.dock_state
            .iter_all_tabs()
            .next()
            .map(|(_, tab)| tab.id())
    }

    fn find_tab_position(
        &self,
        menu: WorkbenchMenu,
    ) -> Option<(SurfaceIndex, egui_dock::NodeIndex, TabIndex)> {
        for surface in 0..self.dock_state.surfaces_count() {
            let surface_index = SurfaceIndex(surface);
            let Some(tree) = self
                .dock_state
                .get_surface(surface_index)
                .and_then(|surface| surface.node_tree())
            else {
                continue;
            };
            if let Some((node_index, tab_index)) = tree.find_tab_from(|tab| tab.menu == menu) {
                return Some((surface_index, node_index, tab_index));
            }
        }
        None
    }
}

fn rect_is_finite(rect: egui::Rect) -> bool {
    rect.min.x.is_finite()
        && rect.min.y.is_finite()
        && rect.max.x.is_finite()
        && rect.max.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_menu_creates_and_activates_new_tab() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));

        assert_eq!(state.tab_count(), 2);
        assert_eq!(
            state.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Provider))
        );
    }

    #[test]
    fn open_menu_reuses_existing_tab() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));

        assert_eq!(state.tab_count(), 2);
        assert_eq!(
            state.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Provider))
        );
    }

    #[test]
    fn activating_tab_updates_active_tab_without_duplicate_tabs() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
        state.apply(UiAction::OpenMenu(WorkbenchMenu::Channel));
        state.apply(UiAction::ActivateTab(TabId::from_menu(
            WorkbenchMenu::Profile,
        )));

        assert_eq!(
            state.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Profile))
        );
        assert_eq!(state.tab_count(), 3);
    }

    #[test]
    fn close_active_tab_switches_focus_to_remaining_tab() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
        state.apply(UiAction::OpenMenu(WorkbenchMenu::Channel));
        state.apply(UiAction::CloseTab(TabId::from_menu(WorkbenchMenu::Channel)));

        assert!(state.active_tab.is_some());
        assert_ne!(
            state.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Channel))
        );
        assert_eq!(state.tab_count(), 2);
    }

    #[test]
    fn close_last_tab_enters_empty_state() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::CloseTab(TabId::from_menu(WorkbenchMenu::Profile)));

        assert_eq!(state.tab_count(), 0);
        assert!(state.active_tab.is_none());
    }
}
