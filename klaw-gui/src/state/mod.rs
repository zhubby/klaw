pub mod workbench;

use crate::domain::menu::WorkbenchMenu;
use workbench::TabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    OpenMenu(WorkbenchMenu),
    ActivateTab(TabId),
    CloseTab(TabId),
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub workbench: workbench::WorkbenchState,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            workbench: workbench::WorkbenchState::new_with_default(WorkbenchMenu::Profile),
        }
    }
}

impl UiState {
    pub fn apply(&mut self, action: UiAction) {
        self.workbench.apply(action);
    }
}
