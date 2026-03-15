pub mod persistence;
pub mod workbench;

use crate::domain::menu::WorkbenchMenu;
use serde::{Deserialize, Serialize};
use workbench::TabId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    OpenMenu(WorkbenchMenu),
    ActivateTab(TabId),
    CloseTab(TabId),
    SetRuntimeProviderOverride(Option<String>),
    CloseWindow,
    ForcePersistLayout,
    ToggleFullscreen,
    MinimizeWindow,
    ZoomWindow,
    StartWindowDrag,
    ShowAbout,
    HideAbout,
    CycleTheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub workbench: workbench::WorkbenchState,
    pub theme_mode: ThemeMode,
    pub fullscreen: bool,
    #[serde(default)]
    pub runtime_provider_override: Option<String>,
    #[serde(default)]
    pub window_size: Option<WindowSize>,
    pub show_about: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            workbench: workbench::WorkbenchState::new_with_default(WorkbenchMenu::Profile),
            theme_mode: ThemeMode::System,
            fullscreen: false,
            runtime_provider_override: None,
            window_size: None,
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
            UiAction::SetRuntimeProviderOverride(provider_id) => {
                self.runtime_provider_override = provider_id;
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
            | UiAction::ForcePersistLayout
            | UiAction::MinimizeWindow
            | UiAction::ZoomWindow
            | UiAction::StartWindowDrag => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ThemeMode, UiAction, UiState};

    #[test]
    fn theme_mode_cycles_system_light_dark() {
        assert_eq!(ThemeMode::System.next(), ThemeMode::Light);
        assert_eq!(ThemeMode::Light.next(), ThemeMode::Dark);
        assert_eq!(ThemeMode::Dark.next(), ThemeMode::System);
    }

    #[test]
    fn runtime_provider_override_can_be_updated() {
        let mut state = UiState::default();
        state.apply(UiAction::SetRuntimeProviderOverride(Some(
            "anthropic".to_string(),
        )));
        assert_eq!(
            state.runtime_provider_override.as_deref(),
            Some("anthropic")
        );

        state.apply(UiAction::SetRuntimeProviderOverride(None));
        assert_eq!(state.runtime_provider_override, None);
    }
}
