use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[must_use]
pub const fn theme_preference(theme_mode: ThemeMode) -> egui::ThemePreference {
    match theme_mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
    }
}

#[cfg(test)]
mod tests {
    use super::{ThemeMode, theme_preference};

    #[test]
    fn theme_mode_labels_match_expected_copy() {
        assert_eq!(ThemeMode::System.label(), "System");
        assert_eq!(ThemeMode::Light.label(), "Light");
        assert_eq!(ThemeMode::Dark.label(), "Dark");
    }

    #[test]
    fn theme_preference_maps_all_modes() {
        assert_eq!(
            theme_preference(ThemeMode::System),
            egui::ThemePreference::System
        );
        assert_eq!(
            theme_preference(ThemeMode::Light),
            egui::ThemePreference::Light
        );
        assert_eq!(
            theme_preference(ThemeMode::Dark),
            egui::ThemePreference::Dark
        );
    }
}
