//! Terminal ANSI 16-color palette presets matching each Klaw UI theme preset.
//!
//! Each palette maps a UI theme (Catppuccin Mocha, Macchiato, Frappé, Latte, Crab,
//! Blackpink, or the defaults) to the corresponding ANSI 16-color set used by
//! `egui_term::ColorPalette`. Dim colors intentionally mirror normal colors following
//! the Catppuccin convention where dim rendering is handled by the renderer
//! (`linear_multiply` in egui_term).

use egui_term::ColorPalette;
use klaw_ui_kit::{DarkThemePreset, LightThemePreset};

// ---------------------------------------------------------------------------
// Dark palettes
// ---------------------------------------------------------------------------

/// Catppuccin Mocha ANSI palette (dark default).
fn mocha_palette() -> ColorPalette {
    ColorPalette {
        foreground: String::from("#cdd6f4"),
        background: String::from("#1e1e2e"),
        black: String::from("#45475a"),
        red: String::from("#f38ba8"),
        green: String::from("#a6e3a1"),
        yellow: String::from("#f9e2af"),
        blue: String::from("#89b4fa"),
        magenta: String::from("#f5c2e7"),
        cyan: String::from("#94e2d5"),
        white: String::from("#bac2de"),
        bright_black: String::from("#585b70"),
        bright_red: String::from("#f38ba8"),
        bright_green: String::from("#a6e3a1"),
        bright_yellow: String::from("#f9e2af"),
        bright_blue: String::from("#89b4fa"),
        bright_magenta: String::from("#f5c2e7"),
        bright_cyan: String::from("#94e2d5"),
        bright_white: String::from("#a6adc8"),
        bright_foreground: Some(String::from("#cdd6f4")),
        dim_foreground: String::from("#7f849c"),
        dim_black: String::from("#45475a"),
        dim_red: String::from("#f38ba8"),
        dim_green: String::from("#a6e3a1"),
        dim_yellow: String::from("#f9e2af"),
        dim_blue: String::from("#89b4fa"),
        dim_magenta: String::from("#f5c2e7"),
        dim_cyan: String::from("#94e2d5"),
        dim_white: String::from("#bac2de"),
    }
}

/// Catppuccin Macchiato ANSI palette (dark).
fn macchiato_palette() -> ColorPalette {
    ColorPalette {
        foreground: String::from("#cad3f5"),
        background: String::from("#24273a"),
        black: String::from("#494d64"),
        red: String::from("#ed8796"),
        green: String::from("#a6da95"),
        yellow: String::from("#eed49f"),
        blue: String::from("#8aadf4"),
        magenta: String::from("#f5bde6"),
        cyan: String::from("#8bd5ca"),
        white: String::from("#b8c0e0"),
        bright_black: String::from("#5b6078"),
        bright_red: String::from("#ed8796"),
        bright_green: String::from("#a6da95"),
        bright_yellow: String::from("#eed49f"),
        bright_blue: String::from("#8aadf4"),
        bright_magenta: String::from("#f5bde6"),
        bright_cyan: String::from("#8bd5ca"),
        bright_white: String::from("#a5adcb"),
        bright_foreground: Some(String::from("#cad3f5")),
        dim_foreground: String::from("#8087a2"),
        dim_black: String::from("#494d64"),
        dim_red: String::from("#ed8796"),
        dim_green: String::from("#a6da95"),
        dim_yellow: String::from("#eed49f"),
        dim_blue: String::from("#8aadf4"),
        dim_magenta: String::from("#f5bde6"),
        dim_cyan: String::from("#8bd5ca"),
        dim_white: String::from("#b8c0e0"),
    }
}

/// Catppuccin Frappé ANSI palette (dark).
fn frappe_palette() -> ColorPalette {
    ColorPalette {
        foreground: String::from("#c6d0f5"),
        background: String::from("#303446"),
        black: String::from("#51576d"),
        red: String::from("#e78284"),
        green: String::from("#a6d189"),
        yellow: String::from("#e5c890"),
        blue: String::from("#8caaee"),
        magenta: String::from("#f4b8e4"),
        cyan: String::from("#81c8be"),
        white: String::from("#b5bfe2"),
        bright_black: String::from("#626880"),
        bright_red: String::from("#e78284"),
        bright_green: String::from("#a6d189"),
        bright_yellow: String::from("#e5c890"),
        bright_blue: String::from("#8caaee"),
        bright_magenta: String::from("#f4b8e4"),
        bright_cyan: String::from("#81c8be"),
        bright_white: String::from("#a5adce"),
        bright_foreground: Some(String::from("#c6d0f5")),
        dim_foreground: String::from("#838ba7"),
        dim_black: String::from("#51576d"),
        dim_red: String::from("#e78284"),
        dim_green: String::from("#a6d189"),
        dim_yellow: String::from("#e5c890"),
        dim_blue: String::from("#8caaee"),
        dim_magenta: String::from("#f4b8e4"),
        dim_cyan: String::from("#81c8be"),
        dim_white: String::from("#b5bfe2"),
    }
}

/// Blackpink ANSI palette (dark) — derived from `BlackpinkTheme` in klaw-ui-kit.
fn blackpink_palette() -> ColorPalette {
    ColorPalette {
        foreground: String::from("#FBF3F8"),
        background: String::from("#0B080D"),
        black: String::from("#18011B"),
        red: String::from("#FF779E"),
        green: String::from("#A6E3A1"),
        yellow: String::from("#FFC56E"),
        blue: String::from("#82B4FA"),
        magenta: String::from("#FF69B4"),
        cyan: String::from("#93D3C3"),
        white: String::from("#B8C0D0"),
        bright_black: String::from("#241422"),
        bright_red: String::from("#FFA6D5"),
        bright_green: String::from("#AAC474"),
        bright_yellow: String::from("#FECA88"),
        bright_blue: String::from("#89B4FA"),
        bright_magenta: String::from("#FFA6D5"),
        bright_cyan: String::from("#8BD5CA"),
        bright_white: String::from("#A5ADCB"),
        bright_foreground: Some(String::from("#FBF3F8")),
        dim_foreground: String::from("#B85190"),
        dim_black: String::from("#18011B"),
        dim_red: String::from("#FF779E"),
        dim_green: String::from("#A6E3A1"),
        dim_yellow: String::from("#FFC56E"),
        dim_blue: String::from("#82B4FA"),
        dim_magenta: String::from("#FF69B4"),
        dim_cyan: String::from("#93D3C3"),
        dim_white: String::from("#B8C0D0"),
    }
}

/// Default dark palette — egui_term's built-in default, suitable for the
/// Default dark theme preset.
fn default_dark_palette() -> ColorPalette {
    ColorPalette::default()
}

// ---------------------------------------------------------------------------
// Light palettes
// ---------------------------------------------------------------------------

/// Catppuccin Latte ANSI palette (light).
fn latte_palette() -> ColorPalette {
    ColorPalette {
        foreground: String::from("#4c4f69"),
        background: String::from("#eff1f5"),
        black: String::from("#bcc0cc"),
        red: String::from("#d20f39"),
        green: String::from("#40a02b"),
        yellow: String::from("#df8e1d"),
        blue: String::from("#1e66f5"),
        magenta: String::from("#ea76cb"),
        cyan: String::from("#179299"),
        white: String::from("#5c5f77"),
        bright_black: String::from("#acb0be"),
        bright_red: String::from("#d20f39"),
        bright_green: String::from("#40a02b"),
        bright_yellow: String::from("#df8e1d"),
        bright_blue: String::from("#1e66f5"),
        bright_magenta: String::from("#ea76cb"),
        bright_cyan: String::from("#179299"),
        bright_white: String::from("#6c6f85"),
        bright_foreground: Some(String::from("#4c4f69")),
        dim_foreground: String::from("#8c8fa1"),
        dim_black: String::from("#bcc0cc"),
        dim_red: String::from("#d20f39"),
        dim_green: String::from("#40a02b"),
        dim_yellow: String::from("#df8e1d"),
        dim_blue: String::from("#1e66f5"),
        dim_magenta: String::from("#ea76cb"),
        dim_cyan: String::from("#179299"),
        dim_white: String::from("#5c5f77"),
    }
}

/// Crab ANSI palette (light) — derived from `CrabTheme` in klaw-ui-kit.
fn crab_palette() -> ColorPalette {
    ColorPalette {
        foreground: String::from("#603038"),
        background: String::from("#FFFCF8"),
        // normal: CrabTheme surface mappings
        black: String::from("#CCA172"), // overlay1
        red: String::from("#C05A44"),   // error
        green: String::from("#5F6F3A"),
        yellow: String::from("#D0A058"),  // warn
        blue: String::from("#E87050"),    // accent
        magenta: String::from("#603038"), // text
        cyan: String::from("#4D7770"),
        white: String::from("#603038"), // text
        // bright: lighter / complementary mappings
        bright_black: String::from("#F4E8DA"), // surface2
        bright_red: String::from("#E87050"),   // accent
        bright_green: String::from("#90A959"),
        bright_yellow: String::from("#F4BF75"),
        bright_blue: String::from("#82B8C8"),
        bright_magenta: String::from("#AA759F"),
        bright_cyan: String::from("#75B5AA"),
        bright_white: String::from("#8E8E8E"),
        bright_foreground: None,
        dim_foreground: String::from("#603038"),
        // dim = normal (Catppuccin convention)
        dim_black: String::from("#CCA172"),
        dim_red: String::from("#C05A44"),
        dim_green: String::from("#5F6F3A"),
        dim_yellow: String::from("#D0A058"),
        dim_blue: String::from("#E87050"),
        dim_magenta: String::from("#603038"),
        dim_cyan: String::from("#4D7770"),
        dim_white: String::from("#603038"),
    }
}

/// Default light palette — classic light terminal colours with a light background.
fn default_light_palette() -> ColorPalette {
    ColorPalette {
        foreground: String::from("#383a42"),
        background: String::from("#fafafa"),
        black: String::from("#383a42"),
        red: String::from("#e45649"),
        green: String::from("#50a14f"),
        yellow: String::from("#c18401"),
        blue: String::from("#4078f2"),
        magenta: String::from("#a626a4"),
        cyan: String::from("#0184bc"),
        white: String::from("#a0a1a7"),
        bright_black: String::from("#4f525e"),
        bright_red: String::from("#e06c75"),
        bright_green: String::from("#98c379"),
        bright_yellow: String::from("#e5c07b"),
        bright_blue: String::from("#61afef"),
        bright_magenta: String::from("#c678dd"),
        bright_cyan: String::from("#56b6c2"),
        bright_white: String::from("#e5e5e5"),
        bright_foreground: None,
        dim_foreground: String::from("#a0a1a7"),
        // dim = normal (Catppuccin convention)
        dim_black: String::from("#383a42"),
        dim_red: String::from("#e45649"),
        dim_green: String::from("#50a14f"),
        dim_yellow: String::from("#c18401"),
        dim_blue: String::from("#4078f2"),
        dim_magenta: String::from("#a626a4"),
        dim_cyan: String::from("#0184bc"),
        dim_white: String::from("#a0a1a7"),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return the ANSI 16-color [`ColorPalette`] that matches the current theme.
///
/// `is_dark_mode` should reflect whether the system / user preference resolves
/// to dark mode (for `ThemeMode::System`, query `egui::Context` outside this
/// function and pass the result as a bool).
///
/// When `is_dark_mode` is true, `dark_theme` selects the palette; otherwise
/// `light_theme` selects it.
#[must_use]
pub fn palette_for_theme(
    is_dark_mode: bool,
    light_theme: LightThemePreset,
    dark_theme: DarkThemePreset,
) -> ColorPalette {
    if is_dark_mode {
        match dark_theme {
            DarkThemePreset::Default => default_dark_palette(),
            DarkThemePreset::Frappe => frappe_palette(),
            DarkThemePreset::Macchiato => macchiato_palette(),
            DarkThemePreset::Mocha => mocha_palette(),
            DarkThemePreset::Blackpink => blackpink_palette(),
        }
    } else {
        match light_theme {
            LightThemePreset::Default => default_light_palette(),
            LightThemePreset::Latte => latte_palette(),
            LightThemePreset::Crab => crab_palette(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Dark presets produce distinct palettes (background differs from default) --

    #[test]
    fn mocha_background_differs_from_default_dark() {
        let mocha = mocha_palette();
        let default = default_dark_palette();
        assert_ne!(mocha.background, default.background);
        assert_eq!(mocha.background, "#1e1e2e");
    }

    #[test]
    fn macchiato_background_differs_from_default_dark() {
        let macchiato = macchiato_palette();
        let default = default_dark_palette();
        assert_ne!(macchiato.background, default.background);
        assert_eq!(macchiato.background, "#24273a");
    }

    #[test]
    fn frappe_background_differs_from_default_dark() {
        let frappe = frappe_palette();
        let default = default_dark_palette();
        assert_ne!(frappe.background, default.background);
        assert_eq!(frappe.background, "#303446");
    }

    #[test]
    fn blackpink_background_differs_from_default_dark() {
        let blackpink = blackpink_palette();
        let default = default_dark_palette();
        assert_ne!(blackpink.background, default.background);
        assert_eq!(blackpink.background, "#0B080D");
    }

    #[test]
    fn dark_presets_have_distinct_backgrounds_from_each_other() {
        let backgrounds: Vec<String> = [
            default_dark_palette().background,
            mocha_palette().background,
            macchiato_palette().background,
            frappe_palette().background,
            blackpink_palette().background,
        ]
        .into();
        // All five backgrounds should be unique.
        for (i, bg_i) in backgrounds.iter().enumerate() {
            for (j, bg_j) in backgrounds.iter().enumerate() {
                if i != j {
                    assert_ne!(bg_i, bg_j, "dark preset backgrounds must differ");
                }
            }
        }
    }

    // -- Light presets produce distinct palettes --

    #[test]
    fn latte_background_differs_from_default_light() {
        let latte = latte_palette();
        let default = default_light_palette();
        assert_ne!(latte.background, default.background);
        assert_eq!(latte.background, "#eff1f5");
    }

    #[test]
    fn crab_background_differs_from_default_light() {
        let crab = crab_palette();
        let default = default_light_palette();
        assert_ne!(crab.background, default.background);
        assert_eq!(crab.background, "#FFFCF8");
    }

    #[test]
    fn light_presets_have_distinct_backgrounds_from_each_other() {
        let backgrounds: Vec<String> = [
            default_light_palette().background,
            latte_palette().background,
            crab_palette().background,
        ]
        .into();
        for (i, bg_i) in backgrounds.iter().enumerate() {
            for (j, bg_j) in backgrounds.iter().enumerate() {
                if i != j {
                    assert_ne!(bg_i, bg_j, "light preset backgrounds must differ");
                }
            }
        }
    }

    // -- palette_for_theme switches correctly between light and dark --

    #[test]
    fn palette_for_theme_returns_dark_palette_when_is_dark_mode_true() {
        let palette = palette_for_theme(true, LightThemePreset::Latte, DarkThemePreset::Mocha);
        assert_eq!(palette.background, "#1e1e2e"); // Mocha background
    }

    #[test]
    fn palette_for_theme_returns_light_palette_when_is_dark_mode_false() {
        let palette = palette_for_theme(false, LightThemePreset::Latte, DarkThemePreset::Mocha);
        assert_eq!(palette.background, "#eff1f5"); // Latte background
    }

    #[test]
    fn palette_for_theme_switches_all_dark_presets() {
        for (preset, expected_bg) in [
            (DarkThemePreset::Default, "#181818"),
            (DarkThemePreset::Mocha, "#1e1e2e"),
            (DarkThemePreset::Macchiato, "#24273a"),
            (DarkThemePreset::Frappe, "#303446"),
            (DarkThemePreset::Blackpink, "#0B080D"),
        ] {
            let palette = palette_for_theme(true, LightThemePreset::Default, preset);
            assert_eq!(
                palette.background, expected_bg,
                "dark preset {preset:?} background mismatch"
            );
        }
    }

    #[test]
    fn palette_for_theme_switches_all_light_presets() {
        for (preset, expected_bg) in [
            (LightThemePreset::Default, "#fafafa"),
            (LightThemePreset::Latte, "#eff1f5"),
            (LightThemePreset::Crab, "#FFFCF8"),
        ] {
            let palette = palette_for_theme(false, preset, DarkThemePreset::Default);
            assert_eq!(
                palette.background, expected_bg,
                "light preset {preset:?} background mismatch"
            );
        }
    }

    // -- Catppuccin Mocha official values --

    #[test]
    fn mocha_foreground_and_background_match_official_values() {
        let mocha = mocha_palette();
        assert_eq!(mocha.foreground, "#cdd6f4");
        assert_eq!(mocha.background, "#1e1e2e");
    }

    #[test]
    fn mocha_normal_colors_match_official_values() {
        let mocha = mocha_palette();
        assert_eq!(mocha.black, "#45475a");
        assert_eq!(mocha.red, "#f38ba8");
        assert_eq!(mocha.green, "#a6e3a1");
        assert_eq!(mocha.yellow, "#f9e2af");
        assert_eq!(mocha.blue, "#89b4fa");
        assert_eq!(mocha.magenta, "#f5c2e7");
        assert_eq!(mocha.cyan, "#94e2d5");
        assert_eq!(mocha.white, "#bac2de");
    }

    #[test]
    fn mocha_bright_colors_match_official_values() {
        let mocha = mocha_palette();
        assert_eq!(mocha.bright_black, "#585b70");
        assert_eq!(mocha.bright_white, "#a6adc8");
    }

    #[test]
    fn mocha_dim_foreground_and_bright_foreground_match_official() {
        let mocha = mocha_palette();
        assert_eq!(mocha.dim_foreground, "#7f849c");
        assert_eq!(mocha.bright_foreground, Some(String::from("#cdd6f4")));
    }

    #[test]
    fn mocha_dim_colors_equal_normal_colors() {
        let mocha = mocha_palette();
        assert_eq!(mocha.dim_black, mocha.black);
        assert_eq!(mocha.dim_red, mocha.red);
        assert_eq!(mocha.dim_green, mocha.green);
        assert_eq!(mocha.dim_yellow, mocha.yellow);
        assert_eq!(mocha.dim_blue, mocha.blue);
        assert_eq!(mocha.dim_magenta, mocha.magenta);
        assert_eq!(mocha.dim_cyan, mocha.cyan);
        assert_eq!(mocha.dim_white, mocha.white);
    }
}
