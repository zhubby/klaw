pub mod workbench;

use crate::domain::menu::WorkbenchMenu;
use workbench::TabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    OpenMenu(WorkbenchMenu),
    ActivateTab(TabId),
    CloseTab(TabId),
    CloseWindow,
    ToggleFullscreen,
    MinimizeWindow,
    ZoomWindow,
    StartWindowDrag,
    ShowAbout,
    HideAbout,
    CycleTheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn next(self) -> Self {
        match self {
            ThemeMode::System => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::System,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub workbench: workbench::WorkbenchState,
    pub theme_mode: ThemeMode,
    pub fullscreen: bool,
    pub show_about: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            workbench: workbench::WorkbenchState::new_with_default(WorkbenchMenu::Profile),
            theme_mode: ThemeMode::System,
            fullscreen: false,
            show_about: false,
        }
    }
}

impl UiState {
    pub fn apply(&mut self, action: UiAction) {
        match action {
            UiAction::OpenMenu(_) | UiAction::ActivateTab(_) | UiAction::CloseTab(_) => {
                self.workbench.apply(action);
            }
            UiAction::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
            }
            UiAction::ShowAbout => {
                self.show_about = true;
            }
            UiAction::HideAbout => {
                self.show_about = false;
            }
            UiAction::CycleTheme => {
                self.theme_mode = self.theme_mode.next();
            }
            UiAction::CloseWindow
            | UiAction::MinimizeWindow
            | UiAction::ZoomWindow
            | UiAction::StartWindowDrag => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ThemeMode;

    #[test]
    fn theme_mode_cycles_system_light_dark() {
        assert_eq!(ThemeMode::System.next(), ThemeMode::Light);
        assert_eq!(ThemeMode::Light.next(), ThemeMode::Dark);
        assert_eq!(ThemeMode::Dark.next(), ThemeMode::System);
    }
}
