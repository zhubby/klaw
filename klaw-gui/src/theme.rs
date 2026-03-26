use crate::state::{DarkThemePreset, LightThemePreset, ThemeMode, UiState};
use std::collections::HashSet;
use std::path::Path;

const LXGW_WENKAI_REGULAR_TTF: &[u8] =
    include_bytes!("../assets/fonts/lxgw-wenkai/LXGWWenKai-Regular.ttf");
const LXGW_WENKAI_MONO_REGULAR_TTF: &[u8] =
    include_bytes!("../assets/fonts/lxgw-wenkai/LXGWWenKaiMono-Regular.ttf");
const LXGW_WENKAI_PROPORTIONAL_NAME: &str = "lxgw-wenkai-regular";
const LXGW_WENKAI_MONOSPACE_NAME: &str = "lxgw-wenkai-mono-regular";

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    install_embedded_preferred_fonts(&mut fonts);
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    install_system_cjk_fallbacks(&mut fonts);
    ctx.set_fonts(fonts);
}

pub fn apply_theme(ctx: &egui::Context, state: &UiState) {
    let preference = match state.theme_mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
    };
    ctx.set_theme(preference);
    ctx.set_visuals_of(egui::Theme::Light, light_visuals(state.light_theme));
    ctx.set_visuals_of(egui::Theme::Dark, dark_visuals(state.dark_theme));
}

fn light_visuals(preset: LightThemePreset) -> egui::Visuals {
    match preset {
        LightThemePreset::Default => egui::Visuals::light(),
        LightThemePreset::Latte => catppuccin_visuals(catppuccin_egui::LATTE, false),
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

fn install_system_cjk_fallbacks(fonts: &mut egui::FontDefinitions) {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut loaded_paths = HashSet::new();
    let mut loaded_font_names = Vec::new();

    for face in db.faces() {
        if !face
            .families
            .iter()
            .any(|(name, _)| is_preferred_cjk_family(name))
        {
            continue;
        }

        let Some(path) = face_source_path(&face.source) else {
            continue;
        };
        if !loaded_paths.insert(path.to_path_buf()) {
            continue;
        }

        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };

        let name = format!("system-cjk-{}", loaded_font_names.len());
        fonts
            .font_data
            .insert(name.clone(), egui::FontData::from_owned(bytes).into());
        loaded_font_names.push(name);
    }

    if loaded_font_names.is_empty() {
        return;
    }

    {
        let proportional = fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default();
        for font_name in &loaded_font_names {
            if !proportional.contains(font_name) {
                proportional.push(font_name.clone());
            }
        }
    }

    {
        let monospace = fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default();
        for font_name in &loaded_font_names {
            if !monospace.contains(font_name) {
                monospace.push(font_name.clone());
            }
        }
    }
}

fn install_embedded_preferred_fonts(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert(
        LXGW_WENKAI_PROPORTIONAL_NAME.to_string(),
        egui::FontData::from_static(LXGW_WENKAI_REGULAR_TTF).into(),
    );
    fonts.font_data.insert(
        LXGW_WENKAI_MONOSPACE_NAME.to_string(),
        egui::FontData::from_static(LXGW_WENKAI_MONO_REGULAR_TTF).into(),
    );

    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    prepend_font_family(proportional, LXGW_WENKAI_PROPORTIONAL_NAME);

    let monospace = fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default();
    prepend_font_family(monospace, LXGW_WENKAI_MONOSPACE_NAME);
}

fn prepend_font_family(family: &mut Vec<String>, font_name: &str) {
    family.retain(|existing| existing != font_name);
    family.insert(0, font_name.to_string());
}

fn face_source_path(source: &fontdb::Source) -> Option<&Path> {
    match source {
        fontdb::Source::File(path) => Some(path.as_path()),
        fontdb::Source::SharedFile(path, _) => Some(path.as_path()),
        fontdb::Source::Binary(_) => None,
    }
}

fn is_preferred_cjk_family(family_name: &str) -> bool {
    let normalized = family_name.to_ascii_lowercase();
    [
        "pingfang",
        "hiragino sans gb",
        "songti",
        "heiti",
        "microsoft yahei",
        "simsun",
        "dengxian",
        "noto sans cjk",
        "source han sans",
        "wenquanyi",
    ]
    .iter()
    .any(|candidate| normalized.contains(candidate))
}

#[cfg(test)]
mod tests {
    use super::{
        LXGW_WENKAI_MONOSPACE_NAME, LXGW_WENKAI_PROPORTIONAL_NAME, dark_visuals,
        install_embedded_preferred_fonts, is_preferred_cjk_family, light_visuals,
    };
    use crate::state::{DarkThemePreset, LightThemePreset};

    #[test]
    fn preferred_cjk_family_match_is_case_insensitive() {
        assert!(is_preferred_cjk_family("PingFang SC"));
        assert!(is_preferred_cjk_family("Noto Sans CJK SC"));
        assert!(is_preferred_cjk_family("microsoft YaHei"));
        assert!(!is_preferred_cjk_family("Fira Code"));
    }

    #[test]
    fn embedded_preferred_fonts_are_prepended() {
        let mut fonts = egui::FontDefinitions::default();

        install_embedded_preferred_fonts(&mut fonts);

        assert_eq!(
            fonts.families[&egui::FontFamily::Proportional].first(),
            Some(&LXGW_WENKAI_PROPORTIONAL_NAME.to_string())
        );
        assert_eq!(
            fonts.families[&egui::FontFamily::Monospace].first(),
            Some(&LXGW_WENKAI_MONOSPACE_NAME.to_string())
        );
    }

    #[test]
    fn non_default_theme_presets_override_base_visuals() {
        let default_light = light_visuals(LightThemePreset::Default);
        let latte = light_visuals(LightThemePreset::Latte);
        let default_dark = dark_visuals(DarkThemePreset::Default);
        let mocha = dark_visuals(DarkThemePreset::Mocha);

        assert_ne!(latte.panel_fill, default_light.panel_fill);
        assert_ne!(mocha.panel_fill, default_dark.panel_fill);
    }
}
