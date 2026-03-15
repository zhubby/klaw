use crate::domain::menu::WorkbenchMenu;
use crate::state::UiAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId {
    pub menu: WorkbenchMenu,
}

impl TabId {
    pub const fn from_menu(menu: WorkbenchMenu) -> Self {
        Self { menu }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchTab {
    pub id: TabId,
    pub menu: WorkbenchMenu,
    pub title: String,
    pub closable: bool,
}

impl WorkbenchTab {
    pub fn from_menu(menu: WorkbenchMenu) -> Self {
        Self {
            id: TabId::from_menu(menu),
            menu,
            title: menu.default_tab_title().to_string(),
            closable: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkbenchState {
    pub tabs: Vec<WorkbenchTab>,
    pub active_tab: Option<TabId>,
}

impl WorkbenchState {
    pub fn new_with_default(menu: WorkbenchMenu) -> Self {
        let tab = WorkbenchTab::from_menu(menu);
        Self {
            tabs: vec![tab.clone()],
            active_tab: Some(tab.id),
        }
    }

    pub fn apply(&mut self, action: UiAction) {
        match action {
            UiAction::OpenMenu(menu) => self.open_or_activate(menu),
            UiAction::ActivateTab(tab_id) => self.activate(tab_id),
            UiAction::CloseTab(tab_id) => self.close(tab_id),
            UiAction::CloseWindow
            | UiAction::ToggleFullscreen
            | UiAction::MinimizeWindow
            | UiAction::ZoomWindow
            | UiAction::StartWindowDrag
            | UiAction::ShowAbout
            | UiAction::HideAbout
            | UiAction::CycleTheme => {}
        }
    }

    fn open_or_activate(&mut self, menu: WorkbenchMenu) {
        let target = TabId::from_menu(menu);
        if self.tabs.iter().any(|tab| tab.id == target) {
            self.active_tab = Some(target);
            return;
        }

        let tab = WorkbenchTab::from_menu(menu);
        self.active_tab = Some(tab.id);
        self.tabs.push(tab);
    }

    fn activate(&mut self, tab_id: TabId) {
        if self.tabs.iter().any(|tab| tab.id == tab_id) {
            self.active_tab = Some(tab_id);
        }
    }

    fn close(&mut self, tab_id: TabId) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };

        self.tabs.remove(index);

        if self.active_tab != Some(tab_id) {
            return;
        }

        if self.tabs.is_empty() {
            self.active_tab = None;
            return;
        }

        let next_index = index.saturating_sub(1).min(self.tabs.len() - 1);
        self.active_tab = Some(self.tabs[next_index].id);
    }

    pub fn active_tab(&self) -> Option<&WorkbenchTab> {
        self.active_tab
            .and_then(|active_id| self.tabs.iter().find(|tab| tab.id == active_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_menu_creates_and_activates_new_tab() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));

        assert_eq!(state.tabs.len(), 2);
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

        assert_eq!(state.tabs.len(), 2);
        assert_eq!(
            state.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Provider))
        );
    }

    #[test]
    fn close_active_tab_switches_focus_to_previous_tab() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::OpenMenu(WorkbenchMenu::Provider));
        state.apply(UiAction::OpenMenu(WorkbenchMenu::Channel));
        state.apply(UiAction::CloseTab(TabId::from_menu(WorkbenchMenu::Channel)));

        assert_eq!(
            state.active_tab,
            Some(TabId::from_menu(WorkbenchMenu::Provider))
        );
        assert_eq!(state.tabs.len(), 2);
    }

    #[test]
    fn close_last_tab_enters_empty_state() {
        let mut state = WorkbenchState::new_with_default(WorkbenchMenu::Profile);

        state.apply(UiAction::CloseTab(TabId::from_menu(WorkbenchMenu::Profile)));

        assert!(state.tabs.is_empty());
        assert!(state.active_tab.is_none());
    }
}
