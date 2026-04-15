pub mod fonts;
pub mod foundation;
pub mod notifications;
pub mod text_animator;
pub mod theme;
pub mod toggle;

pub use egui_theme_switch::{ThemeSwitch, global_theme_switch};
pub use fonts::install_fonts;
pub use foundation::{
    ThemeMode, theme_mode_from_preference, theme_preference, theme_preference_label,
};
pub use notifications::NotificationCenter;
pub use theme::{DarkThemePreset, LightThemePreset, apply_theme, dark_visuals, light_visuals};
