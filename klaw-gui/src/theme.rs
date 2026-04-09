use crate::state::{DarkThemePreset, LightThemePreset, UiState};
use klaw_ui_kit::theme_preference;

pub fn apply_theme(ctx: &egui::Context, state: &UiState) {
    ctx.set_theme(theme_preference(state.theme_mode));
    ctx.set_visuals_of(egui::Theme::Light, light_visuals(state.light_theme));
    ctx.set_visuals_of(egui::Theme::Dark, dark_visuals(state.dark_theme));
}

fn light_visuals(preset: LightThemePreset) -> egui::Visuals {
    match preset {
        LightThemePreset::Default => egui::Visuals::light(),
        LightThemePreset::Latte => catppuccin_visuals(catppuccin_egui::LATTE, false),
        LightThemePreset::Crab => crab_visuals(),
    }
}

fn dark_visuals(preset: DarkThemePreset) -> egui::Visuals {
    match preset {
        DarkThemePreset::Default => egui::Visuals::dark(),
        DarkThemePreset::Frappe => catppuccin_visuals(catppuccin_egui::FRAPPE, true),
        DarkThemePreset::Macchiato => catppuccin_visuals(catppuccin_egui::MACCHIATO, true),
        DarkThemePreset::Mocha => catppuccin_visuals(catppuccin_egui::MOCHA, true),
    }
}

fn catppuccin_visuals(theme: catppuccin_egui::Theme, dark_mode: bool) -> egui::Visuals {
    let mut style = egui::Style::default();
    style.visuals = if dark_mode {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    catppuccin_egui::set_style_theme(&mut style, theme);
    style.visuals
}

#[derive(Clone, Copy)]
struct CrabTheme {
    text: egui::Color32,
    base: egui::Color32,
    mantle: egui::Color32,
    crust: egui::Color32,
    surface0: egui::Color32,
    surface1: egui::Color32,
    surface2: egui::Color32,
    overlay1: egui::Color32,
    accent: egui::Color32,
    warn: egui::Color32,
    error: egui::Color32,
}

fn crab_visuals() -> egui::Visuals {
    let old = egui::Visuals::light();
    let theme = crab_theme();

    egui::Visuals {
        hyperlink_color: theme.accent,
        faint_bg_color: theme.surface0,
        extreme_bg_color: theme.crust,
        code_bg_color: theme.mantle,
        warn_fg_color: theme.warn,
        error_fg_color: theme.error,
        window_fill: theme.base,
        panel_fill: theme.base,
        window_stroke: egui::Stroke {
            color: theme.overlay1,
            ..old.window_stroke
        },
        widgets: egui::style::Widgets {
            noninteractive: crab_widget_visual(old.widgets.noninteractive, theme, theme.base),
            inactive: crab_widget_visual(old.widgets.inactive, theme, theme.surface0),
            hovered: crab_widget_visual(old.widgets.hovered, theme, theme.surface2),
            active: crab_widget_visual(old.widgets.active, theme, theme.surface1),
            open: crab_widget_visual(old.widgets.open, theme, theme.surface0),
        },
        selection: egui::style::Selection {
            bg_fill: theme.accent.linear_multiply(0.24),
            stroke: egui::Stroke {
                color: theme.text,
                ..old.selection.stroke
            },
        },
        window_shadow: egui::epaint::Shadow {
            color: egui::Color32::from_black_alpha(25),
            ..old.window_shadow
        },
        popup_shadow: egui::epaint::Shadow {
            color: egui::Color32::from_black_alpha(25),
            ..old.popup_shadow
        },
        dark_mode: false,
        ..old
    }
}

fn crab_widget_visual(
    old: egui::style::WidgetVisuals,
    theme: CrabTheme,
    bg_fill: egui::Color32,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill,
        weak_bg_fill: bg_fill,
        bg_stroke: egui::Stroke {
            color: theme.overlay1,
            ..old.bg_stroke
        },
        fg_stroke: egui::Stroke {
            color: theme.text,
            ..old.fg_stroke
        },
        ..old
    }
}

const fn crab_theme() -> CrabTheme {
    CrabTheme {
        text: egui::Color32::from_rgb(0x60, 0x30, 0x38),
        base: egui::Color32::from_rgb(0xFF, 0xFC, 0xF8),
        mantle: egui::Color32::from_rgb(0xFB, 0xF7, 0xF0),
        crust: egui::Color32::from_rgb(0xF6, 0xEF, 0xE4),
        surface0: egui::Color32::from_rgb(0xFD, 0xF8, 0xF1),
        surface1: egui::Color32::from_rgb(0xFA, 0xF2, 0xE7),
        surface2: egui::Color32::from_rgb(0xF4, 0xE8, 0xDA),
        overlay1: egui::Color32::from_rgb(0xCC, 0xA1, 0x72),
        accent: egui::Color32::from_rgb(0xE8, 0x70, 0x50),
        warn: egui::Color32::from_rgb(0xD0, 0xA0, 0x58),
        error: egui::Color32::from_rgb(0xC0, 0x5A, 0x44),
    }
}

#[cfg(test)]
mod tests {
    use super::{dark_visuals, light_visuals};
    use crate::state::{DarkThemePreset, LightThemePreset};

    #[test]
    fn non_default_theme_presets_override_base_visuals() {
        let default_light = light_visuals(LightThemePreset::Default);
        let latte = light_visuals(LightThemePreset::Latte);
        let crab = light_visuals(LightThemePreset::Crab);
        let default_dark = dark_visuals(DarkThemePreset::Default);
        let mocha = dark_visuals(DarkThemePreset::Mocha);

        assert_ne!(latte.panel_fill, default_light.panel_fill);
        assert_ne!(crab.panel_fill, default_light.panel_fill);
        assert_ne!(mocha.panel_fill, default_dark.panel_fill);
    }
}
