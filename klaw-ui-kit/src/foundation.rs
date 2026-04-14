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

#[must_use]
pub const fn theme_mode_from_preference(theme: egui::ThemePreference) -> ThemeMode {
    match theme {
        egui::ThemePreference::System => ThemeMode::System,
        egui::ThemePreference::Light => ThemeMode::Light,
        egui::ThemePreference::Dark => ThemeMode::Dark,
    }
}

#[must_use]
pub const fn theme_preference_label(theme: egui::ThemePreference) -> &'static str {
    theme_mode_from_preference(theme).label()
}

#[cfg(test)]
mod tests {
    use super::{ThemeMode, theme_mode_from_preference, theme_preference, theme_preference_label};

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

    #[test]
    fn theme_mode_from_preference_maps_all_preferences() {
        assert_eq!(
            theme_mode_from_preference(egui::ThemePreference::System),
            ThemeMode::System
        );
        assert_eq!(
            theme_mode_from_preference(egui::ThemePreference::Light),
            ThemeMode::Light
        );
        assert_eq!(
            theme_mode_from_preference(egui::ThemePreference::Dark),
            ThemeMode::Dark
        );
    }

    #[test]
    fn theme_preference_labels_match_expected_copy() {
        assert_eq!(
            theme_preference_label(egui::ThemePreference::System),
            "System"
        );
        assert_eq!(
            theme_preference_label(egui::ThemePreference::Light),
            "Light"
        );
        assert_eq!(theme_preference_label(egui::ThemePreference::Dark), "Dark");
    }
}
