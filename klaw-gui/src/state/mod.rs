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
    SetThemeMode(ThemeMode),
    HideWindow,
    QuitApp,
    ForcePersistLayout,
    ToggleFullscreen,
    MinimizeWindow,
    ZoomWindow,
    StartWindowDrag,
    ShowAbout,
    HideAbout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub const fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "System",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LightThemePreset {
    #[default]
    Default,
    Latte,
}

impl LightThemePreset {
    pub const fn label(self) -> &'static str {
        match self {
            LightThemePreset::Default => "Default",
            LightThemePreset::Latte => "Latte",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DarkThemePreset {
    #[default]
    Default,
    Frappe,
    Macchiato,
    Mocha,
}

impl DarkThemePreset {
    pub const fn label(self) -> &'static str {
        match self {
            DarkThemePreset::Default => "Default",
            DarkThemePreset::Frappe => "Frappé",
            DarkThemePreset::Macchiato => "Macchiato",
            DarkThemePreset::Mocha => "Mocha",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogsLevelFilterState {
    pub trace: bool,
    pub debug: bool,
    pub info: bool,
    pub warn: bool,
    pub error: bool,
    pub unknown: bool,
}

impl Default for LogsLevelFilterState {
    fn default() -> Self {
        Self {
            trace: false,
            debug: false,
            info: true,
            warn: false,
            error: false,
            unknown: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LogsPanelState {
    #[serde(default)]
    pub level_filter: LogsLevelFilterState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub workbench: workbench::WorkbenchState,
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default)]
    pub light_theme: LightThemePreset,
    #[serde(default)]
    pub dark_theme: DarkThemePreset,
    pub fullscreen: bool,
    #[serde(default)]
    pub runtime_provider_override: Option<String>,
    #[serde(default)]
    pub window_size: Option<WindowSize>,
    #[serde(default)]
    pub logs_panel: LogsPanelState,
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
            light_theme: LightThemePreset::Default,
            dark_theme: DarkThemePreset::Default,
            fullscreen: false,
            runtime_provider_override: None,
            window_size: None,
            logs_panel: LogsPanelState::default(),
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
            UiAction::SetThemeMode(theme_mode) => {
                self.theme_mode = theme_mode;
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
            UiAction::HideWindow
            | UiAction::QuitApp
            | UiAction::ForcePersistLayout
            | UiAction::MinimizeWindow
            | UiAction::ZoomWindow
            | UiAction::StartWindowDrag => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DarkThemePreset, LightThemePreset, LogsLevelFilterState, ThemeMode, UiAction, UiState,
    };

    #[test]
    fn ui_state_defaults_to_default_theme_presets() {
        let state = UiState::default();

        assert_eq!(state.theme_mode, ThemeMode::System);
        assert_eq!(state.light_theme, LightThemePreset::Default);
        assert_eq!(state.dark_theme, DarkThemePreset::Default);
    }

    #[test]
    fn theme_mode_can_be_set_explicitly() {
        let mut state = UiState::default();

        state.apply(UiAction::SetThemeMode(ThemeMode::Dark));

        assert_eq!(state.theme_mode, ThemeMode::Dark);
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

    #[test]
    fn logs_panel_defaults_to_info_only() {
        let state = UiState::default();

        assert_eq!(
            state.logs_panel.level_filter,
            LogsLevelFilterState::default()
        );
        assert!(state.logs_panel.level_filter.info);
        assert!(!state.logs_panel.level_filter.trace);
        assert!(!state.logs_panel.level_filter.debug);
        assert!(!state.logs_panel.level_filter.warn);
        assert!(!state.logs_panel.level_filter.error);
        assert!(!state.logs_panel.level_filter.unknown);
    }
}
