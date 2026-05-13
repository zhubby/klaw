use i18n_embed::LanguageLoader;
use i18n_embed::fluent::FluentLanguageLoader;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

    #[must_use]
    pub fn text_args(&self, key: &str, args: HashMap<&str, String>) -> String {
        if self.loader.has(key) {
            self.loader.get_args(key, args)
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
    use super::{HashMap, LocaleDomain, Translator, UiLanguage};

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

    #[test]
    fn gui_text_args_resolves_single_parameter() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        let mut args = HashMap::new();
        args.insert("model", "gpt-4o".to_string());
        let result = translator.text_args("status-default-model", args);
        assert_eq!(result, "Default Model: gpt-4o");
    }

    #[test]
    fn gui_text_args_resolves_single_parameter_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        let mut args = HashMap::new();
        args.insert("model", "gpt-4o".to_string());
        let result = translator.text_args("status-default-model", args);
        assert_eq!(result, "默认模型：gpt-4o");
    }

    #[test]
    fn gui_text_args_resolves_multi_parameter() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        let mut args = HashMap::new();
        args.insert("version", "0.16.5".to_string());
        let result = translator.text_args("about-version", args);
        assert_eq!(result, "Version 0.16.5");
    }

    #[test]
    fn gui_text_args_resolves_multi_parameter_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        let mut args = HashMap::new();
        args.insert("version", "0.16.5".to_string());
        let result = translator.text_args("about-version", args);
        assert_eq!(result, "版本 0.16.5");
    }

    #[test]
    fn gui_text_args_resolves_about_git_commit() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        let mut args = HashMap::new();
        args.insert("sha", "abc123".to_string());
        let result = translator.text_args("about-git-commit", args);
        assert_eq!(result, "Git Commit abc123");
    }

    #[test]
    fn gui_text_args_resolves_update_available() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        let mut args = HashMap::new();
        args.insert("icon", "⬇".to_string());
        args.insert("version", "0.16.5".to_string());
        let result = translator.text_args("status-update-available", args);
        assert_eq!(result, "⬇ Update v0.16.5");
    }

    #[test]
    fn gui_text_args_missing_key_returns_key() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        let mut args = HashMap::new();
        args.insert("x", "y".to_string());
        let result = translator.text_args("nonexistent-key", args);
        assert_eq!(result, "nonexistent-key");
    }

    #[test]
    fn gui_config_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("config-save"), "保存");
        assert_eq!(translator.text("config-validate"), "验证");
        assert_eq!(translator.text("config-reset"), "重置");
        assert_eq!(translator.text("config-migrate"), "迁移");
        assert_eq!(translator.text("config-reload"), "重载");
        assert_eq!(translator.text("config-unsaved"), "● 未保存");
        assert_eq!(translator.text("config-saved"), "● 已保存");
        assert_eq!(translator.text("config-find"), "查找");
        assert_eq!(translator.text("config-search-hint"), "搜索 TOML");
        assert_eq!(
            translator.text("config-search-type-to-search"),
            "输入以搜索"
        );
        assert_eq!(translator.text("config-search-no-matches"), "0 个匹配");
        assert_eq!(translator.text("config-prev"), "上一个");
        assert_eq!(translator.text("config-next"), "下一个");
    }

    #[test]
    fn gui_config_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(translator.text("config-save"), "Save");
        assert_eq!(translator.text("config-validate"), "Validate");
        assert_eq!(translator.text("config-reset"), "Reset");
        assert_eq!(translator.text("config-migrate"), "Migrate");
        assert_eq!(translator.text("config-reload"), "Reload");
        assert_eq!(translator.text("config-unsaved"), "● Unsaved");
        assert_eq!(translator.text("config-saved"), "● Saved");
        assert_eq!(translator.text("config-find"), "Find");
        assert_eq!(translator.text("config-search-hint"), "Search TOML");
        assert_eq!(
            translator.text("config-search-type-to-search"),
            "Type to search"
        );
        assert_eq!(translator.text("config-search-no-matches"), "0 matches");
        assert_eq!(translator.text("config-prev"), "Prev");
        assert_eq!(translator.text("config-next"), "Next");
    }

    #[test]
    fn gui_config_panel_translates_notifications_with_args_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "config-notify-load-failed",
                HashMap::from([("error", "disk error".to_string())])
            ),
            "Failed to load config: disk error"
        );
        assert_eq!(
            translator.text_args(
                "config-notify-save-failed",
                HashMap::from([("error", "write error".to_string())])
            ),
            "Save failed: write error"
        );
        assert_eq!(
            translator.text_args(
                "config-path-hint",
                HashMap::from([("path", "/tmp/config.toml".to_string())])
            ),
            "Config file: /tmp/config.toml"
        );
    }

    #[test]
    fn gui_config_panel_translates_notifications_with_args_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "config-notify-load-failed",
                HashMap::from([("error", "磁盘错误".to_string())])
            ),
            "加载配置失败：磁盘错误"
        );
        assert_eq!(
            translator.text_args(
                "config-notify-save-failed",
                HashMap::from([("error", "写入错误".to_string())])
            ),
            "保存失败：写入错误"
        );
        assert_eq!(
            translator.text_args(
                "config-path-hint",
                HashMap::from([("path", "/tmp/config.toml".to_string())])
            ),
            "配置文件：/tmp/config.toml"
        );
    }

    #[test]
    fn gui_config_panel_translates_confirm_dialog_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("config-confirm-title"), "未保存的更改");
        assert_eq!(
            translator.text("config-confirm-message"),
            "当前编辑尚未保存。是否继续并覆盖编辑器内容？"
        );
        assert_eq!(translator.text("config-confirm-continue"), "继续");
        assert_eq!(translator.text("config-confirm-cancel"), "取消");
    }
}
