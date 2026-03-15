use crate::state::ThemeMode;
use std::collections::HashSet;
use std::path::Path;

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    install_system_cjk_fallbacks(&mut fonts);
    ctx.set_fonts(fonts);
}

pub fn apply_theme(ctx: &egui::Context, theme_mode: ThemeMode) {
    let preference = match theme_mode {
        ThemeMode::System => egui::ThemePreference::System,
        ThemeMode::Light => egui::ThemePreference::Light,
        ThemeMode::Dark => egui::ThemePreference::Dark,
    };
    ctx.set_theme(preference);
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
    use super::is_preferred_cjk_family;

    #[test]
    fn preferred_cjk_family_match_is_case_insensitive() {
        assert!(is_preferred_cjk_family("PingFang SC"));
        assert!(is_preferred_cjk_family("Noto Sans CJK SC"));
        assert!(is_preferred_cjk_family("microsoft YaHei"));
        assert!(!is_preferred_cjk_family("Fira Code"));
    }
}
