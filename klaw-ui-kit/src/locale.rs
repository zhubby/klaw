use i18n_embed::LanguageLoader;
use i18n_embed::fluent::FluentLanguageLoader;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use unic_langid::{LanguageIdentifier, langid};

#[derive(RustEmbed)]
#[folder = "locales"]
struct LocaleAssets;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum UiLanguage {
    #[default]
    English,
    SimplifiedChinese,
}

impl UiLanguage {
    #[must_use]
    pub const fn available() -> &'static [Self] {
        &[Self::English, Self::SimplifiedChinese]
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
        }
    }

    #[must_use]
    fn language_identifier(self) -> LanguageIdentifier {
        match self {
            Self::English => langid!("en-US"),
            Self::SimplifiedChinese => langid!("zh-CN"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaleDomain {
    Gui,
    WebUi,
}

impl LocaleDomain {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Gui => "gui",
            Self::WebUi => "webui",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Translator {
    loader: &'static FluentLanguageLoader,
}

impl Translator {
    #[must_use]
    pub fn new(domain: LocaleDomain, language: UiLanguage) -> Self {
        Self {
            loader: cached_loader(domain, language),
        }
    }

    #[must_use]
    pub fn text(&self, key: &str) -> String {
        if self.loader.has(key) {
            self.loader.get(key)
        } else {
            key.to_string()
        }
    }
}

fn cached_loader(domain: LocaleDomain, language: UiLanguage) -> &'static FluentLanguageLoader {
    static GUI_ENGLISH: OnceLock<FluentLanguageLoader> = OnceLock::new();
    static GUI_SIMPLIFIED_CHINESE: OnceLock<FluentLanguageLoader> = OnceLock::new();
    static WEBUI_ENGLISH: OnceLock<FluentLanguageLoader> = OnceLock::new();
    static WEBUI_SIMPLIFIED_CHINESE: OnceLock<FluentLanguageLoader> = OnceLock::new();

    match (domain, language) {
        (LocaleDomain::Gui, UiLanguage::English) => {
            GUI_ENGLISH.get_or_init(|| load_translator(domain, language))
        }
        (LocaleDomain::Gui, UiLanguage::SimplifiedChinese) => {
            GUI_SIMPLIFIED_CHINESE.get_or_init(|| load_translator(domain, language))
        }
        (LocaleDomain::WebUi, UiLanguage::English) => {
            WEBUI_ENGLISH.get_or_init(|| load_translator(domain, language))
        }
        (LocaleDomain::WebUi, UiLanguage::SimplifiedChinese) => {
            WEBUI_SIMPLIFIED_CHINESE.get_or_init(|| load_translator(domain, language))
        }
    }
}

fn load_translator(domain: LocaleDomain, language: UiLanguage) -> FluentLanguageLoader {
    let loader =
        FluentLanguageLoader::new(domain.as_str(), UiLanguage::English.language_identifier());
    let mut languages = vec![language.language_identifier()];
    if language != UiLanguage::English {
        languages.push(UiLanguage::English.language_identifier());
    }
    let _ = loader.load_languages(&LocaleAssets, &languages);
    loader.set_use_isolating(false);
    loader
}

#[cfg(test)]
mod tests {
    use super::{LocaleDomain, Translator, UiLanguage};

    #[test]
    fn ui_language_defaults_to_english_and_exposes_labels() {
        assert_eq!(UiLanguage::default(), UiLanguage::English);
        assert_eq!(UiLanguage::English.label(), "English");
        assert_eq!(UiLanguage::SimplifiedChinese.label(), "简体中文");
    }

    #[test]
    fn gui_domain_translates_top_menu_copy() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);

        assert_eq!(translator.text("menu-file"), "文件");
        assert_eq!(translator.text("menu-force-persist-layout"), "强制保存布局");
    }

    #[test]
    fn webui_domain_keeps_independent_menu_copy() {
        let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

        assert_eq!(translator.text("menu-window"), "窗口");
        assert_eq!(translator.text("menu-tile-windows"), "平铺窗口");
    }

    #[test]
    fn missing_translation_falls_back_to_english_then_key() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);

        assert_eq!(translator.text("test-english-only"), "English only");
        assert_eq!(translator.text("missing-key"), "missing-key");
    }
}
