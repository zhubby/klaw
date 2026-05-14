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

    #[test]
    fn gui_profile_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(translator.text("profile-reload"), "Reload");
        assert_eq!(translator.text("profile-create-file"), "Create File");
        assert_eq!(
            translator.text("profile-workspace-markdown-files"),
            "Workspace Markdown Files"
        );
        assert_eq!(
            translator.text("profile-no-markdown-files"),
            "No markdown files found in the workspace directory."
        );
        assert_eq!(
            translator.text("profile-system-prompt-preview"),
            "System Prompt Preview"
        );
        assert_eq!(translator.text("profile-save"), "Save");
        assert_eq!(translator.text("profile-cancel"), "Cancel");
        assert_eq!(translator.text("profile-reset-btn"), "Reset");
        assert_eq!(translator.text("profile-default"), "Default");
        assert_eq!(translator.text("profile-create-btn"), "Create");
        assert_eq!(translator.text("profile-delete"), "Delete");
        assert_eq!(translator.text("profile-preview"), "Preview");
    }

    #[test]
    fn gui_profile_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("profile-reload"), "重载");
        assert_eq!(translator.text("profile-create-file"), "创建文件");
        assert_eq!(
            translator.text("profile-workspace-markdown-files"),
            "工作区 Markdown 文件"
        );
        assert_eq!(
            translator.text("profile-no-markdown-files"),
            "在工作区目录中未找到 Markdown 文件。"
        );
        assert_eq!(
            translator.text("profile-system-prompt-preview"),
            "系统提示词预览"
        );
        assert_eq!(translator.text("profile-save"), "保存");
        assert_eq!(translator.text("profile-cancel"), "取消");
        assert_eq!(translator.text("profile-reset-btn"), "重置");
        assert_eq!(translator.text("profile-default"), "默认");
        assert_eq!(translator.text("profile-create-btn"), "创建");
        assert_eq!(translator.text("profile-delete"), "删除");
        assert_eq!(translator.text("profile-preview"), "预览");
    }

    #[test]
    fn gui_profile_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "profile-markdown-files-count",
                HashMap::from([("count", "3".to_string())])
            ),
            "Markdown Files: 3"
        );
        assert_eq!(
            translator.text_args(
                "profile-edit-title",
                HashMap::from([("name", "system.md".to_string())])
            ),
            "Edit system.md"
        );
        assert_eq!(
            translator.text_args(
                "profile-path-hint",
                HashMap::from([("path", "/tmp/ws".to_string())])
            ),
            "Workspace: /tmp/ws"
        );
        assert_eq!(
            translator.text_args(
                "profile-notify-saved",
                HashMap::from([("name", "system.md".to_string())])
            ),
            "Saved system.md"
        );
    }

    #[test]
    fn gui_profile_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "profile-markdown-files-count",
                HashMap::from([("count", "3".to_string())])
            ),
            "Markdown 文件：3"
        );
        assert_eq!(
            translator.text_args(
                "profile-edit-title",
                HashMap::from([("name", "system.md".to_string())])
            ),
            "编辑 system.md"
        );
        assert_eq!(
            translator.text_args(
                "profile-path-hint",
                HashMap::from([("path", "/tmp/ws".to_string())])
            ),
            "工作区：/tmp/ws"
        );
        assert_eq!(
            translator.text_args(
                "profile-notify-saved",
                HashMap::from([("name", "system.md".to_string())])
            ),
            "已保存 system.md"
        );
    }

    #[test]
    fn gui_settings_panel_translates_section_titles_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(translator.text("setting-section-general"), "General");
        assert_eq!(
            translator.text("setting-section-security"),
            "Security & Privacy"
        );
        assert_eq!(translator.text("setting-section-network"), "Network");
        assert_eq!(translator.text("setting-section-sync"), "Sync");
    }

    #[test]
    fn gui_settings_panel_translates_section_titles_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("setting-section-general"), "通用");
        assert_eq!(translator.text("setting-section-security"), "安全与隐私");
        assert_eq!(translator.text("setting-section-network"), "网络");
        assert_eq!(translator.text("setting-section-sync"), "同步");
    }

    #[test]
    fn gui_settings_panel_translates_common_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(translator.text("setting-yes"), "Yes");
        assert_eq!(translator.text("setting-no"), "No");
        assert_eq!(translator.text("setting-cancel"), "Cancel");
        assert_eq!(translator.text("setting-enabled"), "enabled");
        assert_eq!(translator.text("setting-disabled"), "disabled");
        assert_eq!(
            translator.text("setting-subtitle"),
            "Configure application preferences"
        );
    }

    #[test]
    fn gui_settings_panel_translates_common_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("setting-yes"), "是");
        assert_eq!(translator.text("setting-no"), "否");
        assert_eq!(translator.text("setting-cancel"), "取消");
        assert_eq!(translator.text("setting-enabled"), "已启用");
        assert_eq!(translator.text("setting-disabled"), "已禁用");
        assert_eq!(translator.text("setting-subtitle"), "配置应用偏好");
    }

    #[test]
    fn gui_settings_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "setting-save-error",
                HashMap::from([("error", "disk error".to_string())])
            ),
            "Save error: disk error"
        );
        assert_eq!(
            translator.text_args(
                "setting-theme-mode-current",
                HashMap::from([("mode", "Dark".to_string())])
            ),
            "Current theme mode: Dark (change from the bottom status bar)."
        );
    }

    #[test]
    fn gui_settings_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "setting-save-error",
                HashMap::from([("error", "磁盘错误".to_string())])
            ),
            "保存错误：磁盘错误"
        );
        assert_eq!(
            translator.text_args(
                "setting-theme-mode-current",
                HashMap::from([("mode", "深色".to_string())])
            ),
            "当前主题模式：深色（可在底部状态栏更改）。"
        );
    }

    #[test]
    fn gui_system_panel_translates_view_tabs_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("system-view-host-information"),
            "Host Information"
        );
        assert_eq!(
            translator.text("system-view-program-disk-usage"),
            "Program Disk Usage"
        );
        assert_eq!(translator.text("system-view-environment"), "Environment");
    }

    #[test]
    fn gui_system_panel_translates_view_tabs_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("system-view-host-information"), "主机信息");
        assert_eq!(
            translator.text("system-view-program-disk-usage"),
            "程序磁盘使用"
        );
        assert_eq!(translator.text("system-view-environment"), "环境");
    }

    #[test]
    fn gui_system_panel_translates_dir_titles_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(translator.text("system-dir-tmp"), "Temporary");
        assert_eq!(translator.text("system-dir-workspace"), "Workspace");
        assert_eq!(translator.text("system-dir-sessions"), "Sessions");
        assert_eq!(translator.text("system-dir-logs"), "Logs");
        assert_eq!(
            translator.text("system-dir-skills-registry"),
            "Skills Registry"
        );
        assert_eq!(translator.text("system-dir-models"), "Models");
    }

    #[test]
    fn gui_system_panel_translates_dir_titles_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("system-dir-tmp"), "临时文件");
        assert_eq!(translator.text("system-dir-workspace"), "工作区");
        assert_eq!(translator.text("system-dir-sessions"), "会话");
        assert_eq!(translator.text("system-dir-logs"), "日志");
        assert_eq!(translator.text("system-dir-skills-registry"), "技能仓库");
        assert_eq!(translator.text("system-dir-models"), "模型");
    }

    #[test]
    fn gui_system_panel_translates_host_info_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(translator.text("system-cpu-usage"), "CPU Usage");
        assert_eq!(translator.text("system-memory-usage"), "Memory Usage");
        assert_eq!(
            translator.text("system-system-information"),
            "System Information"
        );
        assert_eq!(translator.text("system-host-app-uptime"), "App Uptime");
        assert_eq!(translator.text("system-host-name"), "Host Name");
        assert_eq!(translator.text("system-host-os-name"), "OS Name");
        assert_eq!(translator.text("system-host-total-memory"), "Total Memory");
        assert_eq!(translator.text("system-host-na"), "N/A");
        assert_eq!(translator.text("system-host-loading"), "Loading...");
    }

    #[test]
    fn gui_system_panel_translates_host_info_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("system-cpu-usage"), "CPU 使用率");
        assert_eq!(translator.text("system-memory-usage"), "内存使用率");
        assert_eq!(translator.text("system-system-information"), "系统信息");
        assert_eq!(translator.text("system-host-app-uptime"), "应用运行时间");
        assert_eq!(translator.text("system-host-na"), "无");
        assert_eq!(translator.text("system-host-loading"), "加载中...");
    }

    #[test]
    fn gui_system_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "system-cpu-cores-info",
                HashMap::from([("logical", "8".to_string()), ("physical", "4".to_string())])
            ),
            "8 logical / 4 physical cores"
        );
        assert_eq!(
            translator.text_args(
                "system-memory-free",
                HashMap::from([("free", "2.00 GB".to_string())])
            ),
            "Free: 2.00 GB"
        );
        assert_eq!(
            translator.text_args(
                "system-cpu-frequency-mhz",
                HashMap::from([("freq", "2400".to_string())])
            ),
            "2400 MHz"
        );
        assert_eq!(
            translator.text_args(
                "system-confirm-clear-title",
                HashMap::from([("title", "Sessions".to_string())])
            ),
            "Clear Sessions directory"
        );
        assert_eq!(
            translator.text_args(
                "system-notify-dir-cleared",
                HashMap::from([("title", "Logs".to_string())])
            ),
            "Logs directory cleared"
        );
    }

    #[test]
    fn gui_system_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "system-cpu-cores-info",
                HashMap::from([("logical", "8".to_string()), ("physical", "4".to_string())])
            ),
            "8 逻辑 / 4 物理 核心数"
        );
        assert_eq!(
            translator.text_args(
                "system-memory-free",
                HashMap::from([("free", "2.00 GB".to_string())])
            ),
            "可用: 2.00 GB"
        );
        assert_eq!(
            translator.text_args(
                "system-confirm-clear-title",
                HashMap::from([("title", "会话".to_string())])
            ),
            "清除 会话 目录"
        );
        assert_eq!(
            translator.text_args(
                "system-notify-dir-cleared",
                HashMap::from([("title", "日志".to_string())])
            ),
            "日志 目录已清除"
        );
    }

    #[test]
    fn gui_acp_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("acp-panel-description"),
            "ACP lets klaw call external ACP-compatible coding agents through adapter commands."
        );
        assert_eq!(
            translator.text("acp-notify-config-loaded"),
            "ACP config loaded from disk"
        );
        assert_eq!(translator.text("acp-stats-enabled"), "Enabled");
        assert_eq!(translator.text("acp-col-id"), "ID");
        assert_eq!(translator.text("acp-enabled-status-yes"), "yes");
        assert_eq!(translator.text("acp-enabled-status-no"), "no");
        assert_eq!(translator.text("acp-form-title-add"), "Add ACP Agent");
        assert_eq!(translator.text("acp-form-label-id"), "ID");
        assert_eq!(
            translator.text("acp-delete-dialog-title"),
            "Delete ACP Agent"
        );
        assert_eq!(translator.text("acp-value-not-set"), "(not set)");
        assert_eq!(translator.text("acp-test-prompt-title"), "ACP Test Prompt");
    }

    #[test]
    fn gui_acp_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("acp-stats-enabled"), "已启用");
        assert_eq!(translator.text("acp-stats-running"), "运行中");
        assert_eq!(translator.text("acp-col-id"), "ID");
        assert_eq!(translator.text("acp-col-status"), "状态");
        assert_eq!(translator.text("acp-enabled-status-yes"), "是");
        assert_eq!(translator.text("acp-form-title-add"), "添加 ACP 代理");
        assert_eq!(translator.text("acp-delete-dialog-title"), "删除 ACP 代理");
        assert_eq!(translator.text("acp-value-not-set"), "(未设置)");
        assert_eq!(translator.text("acp-test-prompt-title"), "ACP 测试提示");
    }

    #[test]
    fn gui_llm_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(translator.text("llm-btn-refresh"), "Refresh");
        assert_eq!(translator.text("llm-filter-session"), "Session");
        assert_eq!(translator.text("llm-filter-all"), "All");
        assert_eq!(translator.text("llm-col-model"), "Model");
        assert_eq!(translator.text("llm-col-status"), "Status");
        assert_eq!(translator.text("llm-title-detail"), "LLM Audit Detail");
        assert_eq!(translator.text("llm-tab-request"), "Request");
        assert_eq!(translator.text("llm-status-loading"), "Loading...");
        assert_eq!(translator.text("llm-sort-time-asc"), "Time ↑");
    }

    #[test]
    fn gui_llm_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("llm-btn-refresh"), "刷新");
        assert_eq!(translator.text("llm-filter-session"), "会话");
        assert_eq!(translator.text("llm-filter-all"), "全部");
        assert_eq!(translator.text("llm-col-model"), "模型");
        assert_eq!(translator.text("llm-col-status"), "状态");
        assert_eq!(translator.text("llm-title-detail"), "LLM 审计详情");
        assert_eq!(translator.text("llm-tab-request"), "请求");
        assert_eq!(translator.text("llm-status-loading"), "加载中...");
        assert_eq!(translator.text("llm-sort-time-asc"), "时间 ↑");
    }

    #[test]
    fn gui_mcp_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("mcp-notify-config-loaded"),
            "MCP config loaded from disk"
        );
        assert_eq!(
            translator.text("mcp-label-no-servers"),
            "No MCP servers configured."
        );
        assert_eq!(translator.text("mcp-col-id"), "ID");
        assert_eq!(translator.text("mcp-col-status"), "Status");
        assert_eq!(translator.text("mcp-label-enabled-yes"), "yes");
        assert_eq!(translator.text("mcp-form-title-add"), "Add MCP Server");
        assert_eq!(translator.text("mcp-mode-stdio"), "stdio");
        assert_eq!(translator.text("mcp-state-running"), "running");
        assert_eq!(translator.text("mcp-detail-heading"), "MCP Server Detail");
    }

    #[test]
    fn gui_mcp_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("mcp-notify-config-loaded"),
            "MCP 配置已从磁盘加载"
        );
        assert_eq!(
            translator.text("mcp-label-no-servers"),
            "未配置 MCP 服务器。"
        );
        assert_eq!(translator.text("mcp-col-id"), "ID");
        assert_eq!(translator.text("mcp-col-status"), "状态");
        assert_eq!(translator.text("mcp-label-enabled-yes"), "是");
        assert_eq!(translator.text("mcp-form-title-add"), "添加 MCP 服务器");
        assert_eq!(translator.text("mcp-mode-stdio"), "stdio");
        assert_eq!(translator.text("mcp-state-running"), "运行中");
        assert_eq!(translator.text("mcp-detail-heading"), "MCP 服务器详情");
    }

    #[test]
    fn gui_local_model_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("local-model-subtitle"),
            "Browse, install, and manage local LLM models stored on your device."
        );
        assert_eq!(
            translator.text("local-model-installed-label"),
            "Installed Models"
        );
        assert_eq!(
            translator.text("local-model-no-models"),
            "No local models installed yet."
        );
        assert_eq!(translator.text("local-model-col-name"), "Name");
        assert_eq!(translator.text("local-model-col-size"), "Size");
        assert_eq!(translator.text("local-model-col-created"), "Created");
        assert_eq!(
            translator.text("local-model-col-default-file"),
            "Default Model File"
        );
        assert_eq!(
            translator.text("local-model-window-install"),
            "Install Model"
        );
        assert_eq!(
            translator.text("local-model-window-downloading"),
            "Downloading Model"
        );
        assert_eq!(
            translator.text("local-model-window-delete"),
            "Delete Local Model"
        );
        assert_eq!(
            translator.text("local-model-notify-config-loaded"),
            "Local model config loaded from disk"
        );
    }

    #[test]
    fn gui_local_model_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("local-model-subtitle"),
            "浏览、安装和管理存储在设备上的本地 LLM 模型。"
        );
        assert_eq!(translator.text("local-model-installed-label"), "已安装模型");
        assert_eq!(
            translator.text("local-model-no-models"),
            "尚未安装本地模型。"
        );
        assert_eq!(translator.text("local-model-col-name"), "名称");
        assert_eq!(translator.text("local-model-col-size"), "大小");
        assert_eq!(translator.text("local-model-col-created"), "创建时间");
        assert_eq!(
            translator.text("local-model-col-default-file"),
            "默认模型文件"
        );
        assert_eq!(translator.text("local-model-window-install"), "安装模型");
        assert_eq!(
            translator.text("local-model-window-downloading"),
            "正在下载模型"
        );
        assert_eq!(translator.text("local-model-window-delete"), "删除本地模型");
        assert_eq!(
            translator.text("local-model-notify-config-loaded"),
            "本地模型配置已从磁盘加载"
        );
    }

    #[test]
    fn gui_local_model_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "local-model-btn-refresh",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ Refresh"
        );
        assert_eq!(
            translator.text_args(
                "local-model-btn-install",
                HashMap::from([("icon", "⬇".to_string())])
            ),
            "⬇ Install Model"
        );
        assert_eq!(
            translator.text_args(
                "local-model-btn-open-dir",
                HashMap::from([("icon", "📂".to_string())])
            ),
            "📂 Open Models Directory"
        );
        assert_eq!(
            translator.text_args(
                "local-model-notify-load-failed",
                HashMap::from([("error", "disk error".to_string())])
            ),
            "Failed to load config: disk error"
        );
        assert_eq!(
            translator.text_args(
                "local-model-download-file-label",
                HashMap::from([
                    ("index", "1".to_string()),
                    ("total", "3".to_string()),
                    ("name", "model.bin".to_string())
                ])
            ),
            "File 1 / 3: model.bin"
        );
        assert_eq!(
            translator.text_args(
                "local-model-delete-confirm-message",
                HashMap::from([("model_id", "gpt2".to_string())])
            ),
            "Delete model 'gpt2'?"
        );
    }

    #[test]
    fn gui_local_model_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "local-model-btn-refresh",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ 刷新"
        );
        assert_eq!(
            translator.text_args(
                "local-model-btn-install",
                HashMap::from([("icon", "⬇".to_string())])
            ),
            "⬇ 安装模型"
        );
        assert_eq!(
            translator.text_args(
                "local-model-btn-open-dir",
                HashMap::from([("icon", "📂".to_string())])
            ),
            "📂 打开模型目录"
        );
        assert_eq!(
            translator.text_args(
                "local-model-notify-load-failed",
                HashMap::from([("error", "磁盘错误".to_string())])
            ),
            "加载配置失败: 磁盘错误"
        );
        assert_eq!(
            translator.text_args(
                "local-model-download-file-label",
                HashMap::from([
                    ("index", "1".to_string()),
                    ("total", "3".to_string()),
                    ("name", "model.bin".to_string())
                ])
            ),
            "文件 1 / 3: model.bin"
        );
        assert_eq!(
            translator.text_args(
                "local-model-delete-confirm-message",
                HashMap::from([("model_id", "gpt2".to_string())])
            ),
            "删除模型 'gpt2'？"
        );
    }

    #[test]
    fn gui_provider_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("provider-no-providers"),
            "No providers configured."
        );
        assert_eq!(translator.text("provider-col-id"), "ID");
        assert_eq!(translator.text("provider-col-name"), "Name");
        assert_eq!(translator.text("provider-col-base-url"), "Base URL");
        assert_eq!(translator.text("provider-col-wire-api"), "Wire API");
        assert_eq!(
            translator.text("provider-col-default-model"),
            "Default Model"
        );
        assert_eq!(translator.text("provider-col-stream"), "Stream");
        assert_eq!(translator.text("provider-col-tokenizer"), "Tokenizer");
        assert_eq!(translator.text("provider-col-auth"), "Auth");
        assert_eq!(translator.text("provider-badge-config"), "config");
        assert_eq!(translator.text("provider-badge-runtime"), "runtime");
        assert_eq!(translator.text("provider-auth-api-key"), "api_key");
        assert_eq!(translator.text("provider-auth-none"), "none");
        assert_eq!(translator.text("provider-stream-yes"), "yes");
        assert_eq!(translator.text("provider-stream-no"), "no");
        assert_eq!(translator.text("provider-form-title-add"), "Add Provider");
        assert_eq!(translator.text("provider-form-title-edit"), "Edit Provider");
        assert_eq!(
            translator.text("provider-form-persisted-info"),
            "Provider configuration is persisted to config.toml."
        );
        assert_eq!(translator.text("provider-form-id"), "Provider ID");
        assert_eq!(translator.text("provider-form-name"), "Display Name");
        assert_eq!(translator.text("provider-delete-title"), "Delete Provider");
    }

    #[test]
    fn gui_provider_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("provider-no-providers"), "未配置提供商。");
        assert_eq!(translator.text("provider-col-id"), "ID");
        assert_eq!(translator.text("provider-col-name"), "名称");
        assert_eq!(translator.text("provider-col-base-url"), "基础 URL");
        assert_eq!(translator.text("provider-col-wire-api"), "传输协议");
        assert_eq!(translator.text("provider-col-default-model"), "默认模型");
        assert_eq!(translator.text("provider-col-stream"), "流式");
        assert_eq!(translator.text("provider-col-tokenizer"), "分词器");
        assert_eq!(translator.text("provider-col-auth"), "认证");
        assert_eq!(translator.text("provider-badge-config"), "配置");
        assert_eq!(translator.text("provider-badge-runtime"), "运行时");
        assert_eq!(translator.text("provider-auth-api-key"), "API 密钥");
        assert_eq!(translator.text("provider-auth-none"), "无");
        assert_eq!(translator.text("provider-stream-yes"), "是");
        assert_eq!(translator.text("provider-stream-no"), "否");
        assert_eq!(translator.text("provider-form-title-add"), "添加提供商");
        assert_eq!(translator.text("provider-form-title-edit"), "编辑提供商");
        assert_eq!(
            translator.text("provider-form-persisted-info"),
            "提供商配置保存在 config.toml 中。"
        );
        assert_eq!(translator.text("provider-form-id"), "提供商 ID");
        assert_eq!(translator.text("provider-form-name"), "显示名称");
        assert_eq!(translator.text("provider-delete-title"), "删除提供商");
    }

    #[test]
    fn gui_provider_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "provider-label-config-default",
                HashMap::from([("provider", "openai".to_string())])
            ),
            "Config default: openai"
        );
        assert_eq!(
            translator.text_args(
                "provider-label-runtime-active",
                HashMap::from([("provider", "openai".to_string())])
            ),
            "Runtime active: openai"
        );
        assert_eq!(
            translator.text_args(
                "provider-btn-add",
                HashMap::from([("icon", "+".to_string())])
            ),
            "+ Add Provider"
        );
        assert_eq!(
            translator.text_args(
                "provider-btn-reload",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ Reload"
        );
        assert_eq!(
            translator.text_args(
                "provider-auth-env",
                HashMap::from([("key", "OPENAI_API_KEY".to_string())])
            ),
            "env: OPENAI_API_KEY"
        );
        assert_eq!(
            translator.text_args(
                "provider-delete-message",
                HashMap::from([("provider_id", "openai".to_string())])
            ),
            "Are you sure you want to delete provider 'openai'?"
        );
        assert_eq!(
            translator.text_args(
                "provider-delete-btn",
                HashMap::from([("icon", "🗑".to_string())])
            ),
            "🗑 Delete"
        );
    }

    #[test]
    fn gui_provider_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "provider-label-config-default",
                HashMap::from([("provider", "openai".to_string())])
            ),
            "配置默认: openai"
        );
        assert_eq!(
            translator.text_args(
                "provider-label-runtime-active",
                HashMap::from([("provider", "openai".to_string())])
            ),
            "运行时活跃: openai"
        );
        assert_eq!(
            translator.text_args(
                "provider-btn-add",
                HashMap::from([("icon", "+".to_string())])
            ),
            "+ 添加提供商"
        );
        assert_eq!(
            translator.text_args(
                "provider-btn-reload",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ 重载"
        );
        assert_eq!(
            translator.text_args(
                "provider-auth-env",
                HashMap::from([("key", "OPENAI_API_KEY".to_string())])
            ),
            "环境变量: OPENAI_API_KEY"
        );
        assert_eq!(
            translator.text_args(
                "provider-delete-message",
                HashMap::from([("provider_id", "openai".to_string())])
            ),
            "确定要删除提供商 'openai' 吗？"
        );
        assert_eq!(
            translator.text_args(
                "provider-delete-btn",
                HashMap::from([("icon", "🗑".to_string())])
            ),
            "🗑 删除"
        );
    }

    #[test]
    fn gui_tool_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("tool-subtitle"),
            "Manage tool enablement and per-tool settings."
        );
        assert_eq!(translator.text("tool-status-enabled"), "Enabled");
        assert_eq!(translator.text("tool-status-disabled"), "Disabled");
        assert_eq!(
            translator.text("tool-status-sync-pending"),
            "Runtime sync pending..."
        );
        assert_eq!(translator.text("tool-col-tool"), "Tool");
        assert_eq!(translator.text("tool-col-status"), "Status");
        assert_eq!(translator.text("tool-col-description"), "Description");
        assert_eq!(translator.text("tool-inspect-description"), "Description");
        assert_eq!(translator.text("tool-inspect-schema"), "Schema");
        assert_eq!(
            translator.text("tool-inspect-metadata-unavailable"),
            "Runtime metadata unavailable for this tool."
        );
        assert_eq!(
            translator.text("tool-notify-config-loaded"),
            "Tool config loaded from disk"
        );
    }

    #[test]
    fn gui_tool_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(translator.text("tool-subtitle"), "管理工具启停与各项设置。");
        assert_eq!(translator.text("tool-status-enabled"), "已启用");
        assert_eq!(translator.text("tool-status-disabled"), "已禁用");
        assert_eq!(
            translator.text("tool-status-sync-pending"),
            "运行时同步等待中..."
        );
        assert_eq!(translator.text("tool-col-tool"), "工具");
        assert_eq!(translator.text("tool-col-status"), "状态");
        assert_eq!(translator.text("tool-col-description"), "描述");
        assert_eq!(translator.text("tool-inspect-description"), "描述");
        assert_eq!(translator.text("tool-inspect-schema"), "模式");
        assert_eq!(
            translator.text("tool-inspect-metadata-unavailable"),
            "该工具无运行时元数据。"
        );
        assert_eq!(
            translator.text("tool-notify-config-loaded"),
            "工具配置已从磁盘加载"
        );
    }

    #[test]
    fn gui_tool_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "tool-btn-reload",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ Reload"
        );
        assert_eq!(
            translator.text_args(
                "tool-form-title",
                HashMap::from([("name", "Bash".to_string())])
            ),
            "Edit Tool: Bash"
        );
        assert_eq!(
            translator.text_args(
                "tool-toggle-title",
                HashMap::from([("kind", "Bash".to_string())])
            ),
            "Edit Tool: Bash"
        );
        assert_eq!(
            translator.text_args(
                "tool-inspect-title",
                HashMap::from([("name", "Bash".to_string())])
            ),
            "Inspect Tool: Bash"
        );
        assert_eq!(
            translator.text_args(
                "tool-notify-load-failed",
                HashMap::from([("error", "disk error".to_string())])
            ),
            "Failed to load config: disk error"
        );
        assert_eq!(
            translator.text_args(
                "tool-notify-synced",
                HashMap::from([("count", "5".to_string())])
            ),
            "Tool config saved and runtime synced (5 tools active)"
        );
        assert_eq!(
            translator.text_args(
                "tool-log-window-title",
                HashMap::from([("name", "Bash".to_string())])
            ),
            "Tool Logs: Bash"
        );
    }

    #[test]
    fn gui_tool_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "tool-btn-reload",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ 刷新"
        );
        assert_eq!(
            translator.text_args(
                "tool-form-title",
                HashMap::from([("name", "Bash".to_string())])
            ),
            "编辑工具: Bash"
        );
        assert_eq!(
            translator.text_args(
                "tool-toggle-title",
                HashMap::from([("kind", "Bash".to_string())])
            ),
            "编辑工具: Bash"
        );
        assert_eq!(
            translator.text_args(
                "tool-inspect-title",
                HashMap::from([("name", "Bash".to_string())])
            ),
            "查看工具详情: Bash"
        );
        assert_eq!(
            translator.text_args(
                "tool-notify-load-failed",
                HashMap::from([("error", "磁盘错误".to_string())])
            ),
            "加载配置失败: 磁盘错误"
        );
        assert_eq!(
            translator.text_args(
                "tool-notify-synced",
                HashMap::from([("count", "5".to_string())])
            ),
            "工具配置已保存并同步运行时（5 个工具活跃）"
        );
        assert_eq!(
            translator.text_args(
                "tool-log-window-title",
                HashMap::from([("name", "Bash".to_string())])
            ),
            "工具日志: Bash"
        );
    }

    #[test]
    fn gui_skills_reg_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("skills-reg-no-registries"),
            "No skill registries configured."
        );
        assert_eq!(translator.text("skills-reg-col-name"), "Name");
        assert_eq!(translator.text("skills-reg-col-address"), "Address");
        assert_eq!(translator.text("skills-reg-col-synced"), "Synced");
        assert_eq!(
            translator.text("skills-reg-config-title"),
            "Skills Registry Config"
        );
        assert_eq!(
            translator.text("skills-reg-form-title-add"),
            "Add Skills Registry"
        );
        assert_eq!(
            translator.text("skills-reg-form-title-edit"),
            "Edit Skills Registry"
        );
        assert_eq!(
            translator.text("skills-reg-delete-title"),
            "Delete Skills Registry"
        );
        assert_eq!(
            translator.text("skills-reg-notify-config-loaded"),
            "Skills registry config loaded from disk"
        );
        assert_eq!(
            translator.text("skills-reg-error-name-empty"),
            "Skills registry name cannot be empty"
        );
        assert_eq!(
            translator.text("skills-reg-error-address-empty"),
            "Skills registry address cannot be empty"
        );
    }

    #[test]
    fn gui_skills_reg_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("skills-reg-no-registries"),
            "未配置技能注册源。"
        );
        assert_eq!(translator.text("skills-reg-col-name"), "名称");
        assert_eq!(translator.text("skills-reg-col-address"), "地址");
        assert_eq!(translator.text("skills-reg-col-synced"), "已同步");
        assert_eq!(translator.text("skills-reg-config-title"), "技能注册源配置");
        assert_eq!(
            translator.text("skills-reg-form-title-add"),
            "添加技能注册源"
        );
        assert_eq!(
            translator.text("skills-reg-form-title-edit"),
            "编辑技能注册源"
        );
        assert_eq!(translator.text("skills-reg-delete-title"), "删除技能注册源");
        assert_eq!(
            translator.text("skills-reg-notify-config-loaded"),
            "技能注册源配置已从磁盘加载"
        );
        assert_eq!(
            translator.text("skills-reg-error-name-empty"),
            "技能注册源名称不能为空"
        );
        assert_eq!(
            translator.text("skills-reg-error-address-empty"),
            "技能注册源地址不能为空"
        );
    }

    #[test]
    fn gui_skills_reg_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "skills-reg-label-registries-count",
                HashMap::from([("count", "2".to_string())])
            ),
            "Registries: 2"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-btn-config",
                HashMap::from([("icon", "⚙".to_string())])
            ),
            "⚙ Config"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-btn-reload",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ Reload"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-btn-add",
                HashMap::from([("icon", "+".to_string())])
            ),
            "+ Add Skills Registry"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-error-name-duplicate",
                HashMap::from([("name", "my-reg".to_string())])
            ),
            "Skills registry 'my-reg' already exists, choose another name"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-delete-message",
                HashMap::from([("registry_name", "my-reg".to_string())])
            ),
            "Are you sure you want to delete registry 'my-reg'?"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-notify-sync-success",
                HashMap::from([
                    ("registry_name", "my-reg".to_string()),
                    ("added", "3".to_string()),
                    ("removed", "1".to_string())
                ])
            ),
            "Registry `my-reg` synced: added 3, removed 1"
        );
    }

    #[test]
    fn gui_skills_reg_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "skills-reg-label-registries-count",
                HashMap::from([("count", "2".to_string())])
            ),
            "注册源: 2"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-btn-config",
                HashMap::from([("icon", "⚙".to_string())])
            ),
            "⚙ 配置"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-btn-reload",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ 刷新"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-btn-add",
                HashMap::from([("icon", "+".to_string())])
            ),
            "+ 添加技能注册源"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-error-name-duplicate",
                HashMap::from([("name", "my-reg".to_string())])
            ),
            "技能注册源 'my-reg' 已存在，请使用其他名称"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-delete-message",
                HashMap::from([("registry_name", "my-reg".to_string())])
            ),
            "确定要删除注册源 'my-reg' 吗？"
        );
        assert_eq!(
            translator.text_args(
                "skills-reg-notify-sync-success",
                HashMap::from([
                    ("registry_name", "my-reg".to_string()),
                    ("added", "3".to_string()),
                    ("removed", "1".to_string())
                ])
            ),
            "注册源 `my-reg` 已同步: 新增 3, 移除 1"
        );
    }

    #[test]
    fn gui_skills_mgr_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("skills-mgr-no-skills"),
            "No installed skills found."
        );
        assert_eq!(translator.text("skills-mgr-col-name"), "Name");
        assert_eq!(translator.text("skills-mgr-col-source"), "Source");
        assert_eq!(translator.text("skills-mgr-col-registry"), "Registry");
        assert_eq!(translator.text("skills-mgr-col-state"), "State");
        assert_eq!(translator.text("skills-mgr-source-local"), "local");
        assert_eq!(translator.text("skills-mgr-source-registry"), "registry");
        assert_eq!(translator.text("skills-mgr-state-stale"), "stale");
        assert_eq!(translator.text("skills-mgr-state-fresh"), "fresh");
        assert_eq!(translator.text("skills-mgr-install-title"), "Install Skill");
        assert_eq!(translator.text("skills-mgr-delete-title"), "Confirm Remove");
    }

    #[test]
    fn gui_skills_mgr_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("skills-mgr-no-skills"),
            "未找到已安装技能。"
        );
        assert_eq!(translator.text("skills-mgr-col-name"), "名称");
        assert_eq!(translator.text("skills-mgr-col-source"), "来源");
        assert_eq!(translator.text("skills-mgr-col-registry"), "注册源");
        assert_eq!(translator.text("skills-mgr-col-state"), "状态");
        assert_eq!(translator.text("skills-mgr-source-local"), "本地");
        assert_eq!(translator.text("skills-mgr-source-registry"), "注册源");
        assert_eq!(translator.text("skills-mgr-state-stale"), "过期");
        assert_eq!(translator.text("skills-mgr-state-fresh"), "最新");
        assert_eq!(translator.text("skills-mgr-install-title"), "安装技能");
        assert_eq!(translator.text("skills-mgr-delete-title"), "确认移除");
    }

    #[test]
    fn gui_skills_mgr_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "skills-mgr-label-installed-count",
                HashMap::from([("count", "5".to_string())])
            ),
            "Installed: 5"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-btn-refresh",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ Refresh"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-detail-title",
                HashMap::from([("name", "my-skill".to_string())])
            ),
            "Skill Detail: my-skill"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-delete-message",
                HashMap::from([("name", "my-skill".to_string())])
            ),
            "Are you sure you want to remove skill `my-skill`?"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-notify-local-install-success",
                HashMap::from([
                    ("skill_name", "my-skill".to_string()),
                    ("source_dir", "/src".to_string()),
                    ("target_dir", "/dest".to_string())
                ])
            ),
            "Installed local skill `my-skill` from /src to /dest"
        );
    }

    #[test]
    fn gui_skills_mgr_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "skills-mgr-label-installed-count",
                HashMap::from([("count", "5".to_string())])
            ),
            "已安装: 5"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-btn-refresh",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ 刷新"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-detail-title",
                HashMap::from([("name", "my-skill".to_string())])
            ),
            "技能详情: my-skill"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-delete-message",
                HashMap::from([("name", "my-skill".to_string())])
            ),
            "确定要移除技能 `my-skill` 吗？"
        );
        assert_eq!(
            translator.text_args(
                "skills-mgr-notify-local-install-success",
                HashMap::from([
                    ("skill_name", "my-skill".to_string()),
                    ("source_dir", "/src".to_string()),
                    ("target_dir", "/dest".to_string())
                ])
            ),
            "已从 /src 安装本地技能 `my-skill` 至 /dest"
        );
    }

    #[test]
    fn gui_panel_subtitles_translated_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("local-model-subtitle"),
            "Browse, install, and manage local LLM models stored on your device."
        );
        assert_eq!(
            translator.text("provider-subtitle"),
            "Configure model providers and set the default provider for the runtime."
        );
        assert_eq!(
            translator.text("skills-reg-subtitle"),
            "Manage skill registries and sync skills from remote repositories."
        );
        assert_eq!(
            translator.text("skills-mgr-subtitle"),
            "Install, view, and manage skills from registries or local sources."
        );
    }

    #[test]
    fn gui_panel_subtitles_translated_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("local-model-subtitle"),
            "浏览、安装和管理存储在设备上的本地 LLM 模型。"
        );
        assert_eq!(
            translator.text("provider-subtitle"),
            "配置模型提供商并设置运行时的默认提供商。"
        );
        assert_eq!(
            translator.text("skills-reg-subtitle"),
            "管理技能仓库并从远程仓库同步技能。"
        );
        assert_eq!(
            translator.text("skills-mgr-subtitle"),
            "从注册源或本地来源安装、查看和管理技能。"
        );
    }

    #[test]
    fn gui_channel_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("channel-subtitle"),
            "Manage channel connections to external messaging services (Dingtalk, Telegram, WebSocket)."
        );
        assert_eq!(
            translator.text("channel-restarting"),
            "Restarting channel..."
        );
        assert_eq!(
            translator.text("channel-synchronizing"),
            "Synchronizing channels..."
        );
        assert_eq!(
            translator.text("channel-no-channels"),
            "No channels configured."
        );
        assert_eq!(translator.text("channel-col-type"), "Type");
        assert_eq!(translator.text("channel-col-id"), "ID");
        assert_eq!(translator.text("channel-col-enabled"), "Enabled");
        assert_eq!(translator.text("channel-col-status"), "Status");
        assert_eq!(translator.text("channel-col-title"), "Title");
        assert_eq!(translator.text("channel-status-running"), "running");
        assert_eq!(translator.text("channel-status-stopped"), "stopped");
        assert_eq!(translator.text("channel-yes"), "yes");
        assert_eq!(translator.text("channel-no"), "no");
        assert_eq!(translator.text("channel-form-id"), "ID");
        assert_eq!(translator.text("channel-form-save"), "Save");
        assert_eq!(translator.text("channel-form-cancel"), "Cancel");
        assert_eq!(
            translator.text("channel-form-title-add-dingtalk"),
            "Add Dingtalk Channel"
        );
        assert_eq!(
            translator.text("channel-form-title-edit-dingtalk"),
            "Edit Dingtalk Channel"
        );
        assert_eq!(
            translator.text("channel-delete-info"),
            "This action cannot be undone."
        );
    }

    #[test]
    fn gui_channel_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("channel-subtitle"),
            "管理与外部消息服务（钉钉、Telegram、WebSocket）的通道连接。"
        );
        assert_eq!(translator.text("channel-restarting"), "正在重启通道...");
        assert_eq!(translator.text("channel-no-channels"), "未配置通道。");
        assert_eq!(translator.text("channel-col-type"), "类型");
        assert_eq!(translator.text("channel-col-enabled"), "启用");
        assert_eq!(translator.text("channel-status-running"), "运行中");
        assert_eq!(translator.text("channel-status-stopped"), "已停止");
        assert_eq!(translator.text("channel-yes"), "是");
        assert_eq!(translator.text("channel-no"), "否");
        assert_eq!(translator.text("channel-form-save"), "保存");
        assert_eq!(translator.text("channel-form-cancel"), "取消");
        assert_eq!(
            translator.text("channel-form-title-add-dingtalk"),
            "添加钉钉通道"
        );
        assert_eq!(translator.text("channel-delete-info"), "此操作无法撤销。");
    }

    #[test]
    fn gui_channel_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "channel-btn-disabled",
                HashMap::from([("icon", "\u{1F527}".to_string())])
            ),
            "\u{1F527} Set Disabled Channels"
        );
        assert_eq!(
            translator.text_args(
                "channel-btn-add-websocket",
                HashMap::from([("icon", "\u{1F4E1}".to_string())])
            ),
            "\u{1F4E1} Add WebSocket"
        );
        assert_eq!(
            translator.text_args(
                "channel-hover-last-event",
                HashMap::from([("event", "ping".to_string())])
            ),
            "last event: ping"
        );
        assert_eq!(
            translator.text_args(
                "channel-delete-title",
                HashMap::from([("kind", "Dingtalk".to_string())])
            ),
            "Delete Dingtalk Channel"
        );
        assert_eq!(
            translator.text_args(
                "channel-delete-message",
                HashMap::from([("id", "ops".to_string())])
            ),
            "Are you sure you want to delete channel 'ops'?"
        );
        assert_eq!(
            translator.text_args(
                "channel-delete-btn",
                HashMap::from([("icon", "\u{1F5D1}".to_string())])
            ),
            "\u{1F5D1} Delete"
        );
    }

    #[test]
    fn gui_channel_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "channel-btn-disabled",
                HashMap::from([("icon", "\u{1F527}".to_string())])
            ),
            "\u{1F527} 设置禁用通道"
        );
        assert_eq!(
            translator.text_args(
                "channel-btn-add-websocket",
                HashMap::from([("icon", "\u{1F4E1}".to_string())])
            ),
            "\u{1F4E1} 添加 WebSocket"
        );
        assert_eq!(
            translator.text_args(
                "channel-delete-title",
                HashMap::from([("kind", "钉钉".to_string())])
            ),
            "删除 钉钉 通道"
        );
        assert_eq!(
            translator.text_args(
                "channel-delete-message",
                HashMap::from([("id", "ops".to_string())])
            ),
            "确定要删除通道 'ops' 吗？"
        );
    }

    #[test]
    fn gui_webhook_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("webhook-subtitle"),
            "Manage webhook endpoints for inbound event and agent prompts."
        );
        assert_eq!(translator.text("webhook-no-rows"), "No webhook rows found.");
        assert_eq!(translator.text("webhook-filter-type"), "Type");
        assert_eq!(translator.text("webhook-filter-events"), "Events");
        assert_eq!(translator.text("webhook-filter-agents"), "Agents");
        assert_eq!(translator.text("webhook-filter-session"), "Session");
        assert_eq!(translator.text("webhook-filter-status"), "Status");
        assert_eq!(translator.text("webhook-filter-all"), "All");
        assert_eq!(translator.text("webhook-col-source"), "Source");
        assert_eq!(translator.text("webhook-col-hook-id"), "Hook ID");
        assert_eq!(translator.text("webhook-status-accepted"), "Accepted");
        assert_eq!(translator.text("webhook-status-processed"), "Processed");
        assert_eq!(translator.text("webhook-status-failed"), "Failed");
        assert_eq!(translator.text("webhook-config-title"), "Webhook Config");
        assert_eq!(
            translator.text("webhook-prompt-create-title"),
            "Create Prompt"
        );
        assert_eq!(translator.text("webhook-prompt-edit-title"), "Edit Prompt");
        assert_eq!(translator.text("webhook-inspect-title"), "Inspect Prompt");
        assert_eq!(translator.text("webhook-delete-title"), "Delete Prompt");
        assert_eq!(translator.text("webhook-trick-generate"), "Generate");
    }

    #[test]
    fn gui_webhook_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("webhook-subtitle"),
            "管理 Webhook 端点以接收传入事件和代理提示词。"
        );
        assert_eq!(
            translator.text("webhook-no-rows"),
            "未找到 Webhook 行数据。"
        );
        assert_eq!(translator.text("webhook-filter-type"), "类型");
        assert_eq!(translator.text("webhook-filter-events"), "事件");
        assert_eq!(translator.text("webhook-filter-agents"), "代理");
        assert_eq!(translator.text("webhook-filter-session"), "会话");
        assert_eq!(translator.text("webhook-filter-all"), "全部");
        assert_eq!(translator.text("webhook-col-source"), "来源");
        assert_eq!(translator.text("webhook-status-accepted"), "已接受");
        assert_eq!(translator.text("webhook-status-failed"), "失败");
        assert_eq!(translator.text("webhook-config-title"), "Webhook 配置");
        assert_eq!(translator.text("webhook-prompt-create-title"), "创建提示词");
        assert_eq!(translator.text("webhook-inspect-title"), "检查提示词");
        assert_eq!(translator.text("webhook-delete-title"), "删除提示词");
        assert_eq!(translator.text("webhook-trick-generate"), "生成");
    }

    #[test]
    fn gui_webhook_panel_translates_parameterized_keys_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "webhook-btn-refresh",
                HashMap::from([("icon", "\u{1F504}".to_string())])
            ),
            "\u{1F504} Refresh"
        );
        assert_eq!(
            translator.text_args(
                "webhook-btn-config",
                HashMap::from([("icon", "\u{1F39B}".to_string())])
            ),
            "\u{1F39B} Config"
        );
        assert_eq!(
            translator.text_args(
                "webhook-delete-message",
                HashMap::from([("hook_id", "order_sync".to_string())])
            ),
            "Delete prompt template 'order_sync'?"
        );
    }

    #[test]
    fn gui_webhook_panel_translates_parameterized_keys_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "webhook-btn-refresh",
                HashMap::from([("icon", "\u{1F504}".to_string())])
            ),
            "\u{1F504} 刷新"
        );
        assert_eq!(
            translator.text_args(
                "webhook-btn-config",
                HashMap::from([("icon", "\u{1F39B}".to_string())])
            ),
            "\u{1F39B} 配置"
        );
        assert_eq!(
            translator.text_args(
                "webhook-delete-message",
                HashMap::from([("hook_id", "order_sync".to_string())])
            ),
            "确定要删除提示词模板 'order_sync' 吗？"
        );
    }

    #[test]
    fn gui_gateway_panel_translates_labels_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text("gw-subtitle"),
            "Manage the embedded gateway service used by the GUI runtime."
        );
        assert_eq!(translator.text("gw-loading"), "Loading...");
        assert_eq!(
            translator.text("gw-status-refreshed"),
            "Gateway status refreshed"
        );
        assert_eq!(
            translator.text("gw-tailscale-status-refreshed"),
            "Tailscale status refreshed"
        );
        assert_eq!(translator.text("gw-notify-started"), "Gateway started");
        assert_eq!(translator.text("gw-notify-restarted"), "Gateway restarted");
        assert_eq!(
            translator.text("gw-notify-worker-closed"),
            "Gateway request worker closed unexpectedly"
        );
        assert_eq!(
            translator.text("gw-notify-config-store-unavailable"),
            "Configuration store is not available"
        );
        assert_eq!(
            translator.text("gw-notify-config-saved"),
            "Gateway config saved"
        );
        assert_eq!(
            translator.text("gw-notify-config-saved-restart"),
            "Gateway config saved. Restart gateway to apply changes."
        );
        assert_eq!(
            translator.text("gw-notify-config-reloaded"),
            "Config reloaded from disk"
        );
        // Status labels
        assert_eq!(translator.text("gw-status-configured"), "Configured");
        assert_eq!(translator.text("gw-status-enabled"), "Enabled");
        assert_eq!(translator.text("gw-status-disabled"), "Disabled");
        assert_eq!(translator.text("gw-status-runtime"), "Runtime");
        assert_eq!(translator.text("gw-status-running"), "running");
        assert_eq!(translator.text("gw-status-stopped"), "stopped");
        assert_eq!(translator.text("gw-status-auth"), "Auth");
        assert_eq!(translator.text("gw-status-auth-configured"), "Configured");
        assert_eq!(
            translator.text("gw-status-auth-not-configured"),
            "Not Configured"
        );
        assert_eq!(translator.text("gw-status-listen-ip"), "Listen IP");
        assert_eq!(translator.text("gw-status-address"), "Address");
        assert_eq!(translator.text("gw-status-started-at"), "Started At");
        // Tailscale
        assert_eq!(translator.text("gw-ts-heading"), "Tailscale");
        assert_eq!(
            translator.text("gw-ts-subtitle"),
            "Expose the gateway via Tailscale Serve (tailnet only) or Funnel (public internet)."
        );
        assert_eq!(translator.text("gw-ts-mode"), "Mode");
        assert_eq!(translator.text("gw-ts-mode-off"), "Off");
        assert_eq!(translator.text("gw-ts-mode-serve"), "Serve (tailnet)");
        assert_eq!(translator.text("gw-ts-mode-funnel"), "Funnel (public)");
        assert_eq!(translator.text("gw-ts-host-status"), "Host Status");
        assert_eq!(translator.text("gw-ts-host-connected"), "Connected");
        assert_eq!(translator.text("gw-ts-host-disconnected"), "Disconnected");
        // Config window
        assert_eq!(translator.text("gw-cfg-title"), "Gateway Config");
        assert_eq!(translator.text("gw-cfg-basic"), "Basic");
        assert_eq!(translator.text("gw-cfg-enabled"), "Enabled");
        assert_eq!(
            translator.text("gw-cfg-enabled-hint"),
            "Enable or disable the gateway service."
        );
        assert_eq!(translator.text("gw-cfg-listen-ip"), "Listen IP");
        assert_eq!(
            translator.text("gw-cfg-listen-ip-hint"),
            "The IP address the gateway binds to. Use 0.0.0.0 for all interfaces."
        );
        assert_eq!(translator.text("gw-cfg-listen-port"), "Listen Port");
        assert_eq!(
            translator.text("gw-cfg-listen-port-hint"),
            "Port number for the gateway. 0 means auto-select."
        );
        assert_eq!(translator.text("gw-cfg-port-auto"), "(0 = auto)");
        assert_eq!(translator.text("gw-cfg-auth"), "Auth");
        assert_eq!(translator.text("gw-cfg-auth-enabled"), "Enabled");
        assert_eq!(
            translator.text("gw-cfg-auth-enabled-hint"),
            "Require authentication token for gateway connections."
        );
        assert_eq!(translator.text("gw-cfg-auth-token"), "Token");
        assert_eq!(
            translator.text("gw-cfg-auth-token-hint"),
            "Secret token used to authenticate gateway clients."
        );
        assert_eq!(translator.text("gw-btn-generate"), "Generate");
        assert_eq!(translator.text("gw-btn-reload"), "Reload");
        assert_eq!(translator.text("gw-btn-save"), "Save");
        assert_eq!(
            translator.text("gw-notify-auth-token-empty"),
            "Gateway auth token is empty"
        );
        assert_eq!(
            translator.text("gw-notify-auth-token-copied"),
            "Gateway auth token copied"
        );
    }

    #[test]
    fn gui_gateway_panel_translates_labels_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text("gw-subtitle"),
            "管理 GUI 运行时使用的嵌入式网关服务。"
        );
        assert_eq!(translator.text("gw-loading"), "加载中...");
        assert_eq!(translator.text("gw-status-refreshed"), "网关状态已刷新");
        assert_eq!(translator.text("gw-notify-started"), "网关已启动");
        assert_eq!(translator.text("gw-notify-restarted"), "网关已重启");
        assert_eq!(
            translator.text("gw-notify-config-store-unavailable"),
            "配置存储不可用"
        );
        assert_eq!(translator.text("gw-notify-config-saved"), "网关配置已保存");
        assert_eq!(
            translator.text("gw-notify-config-reloaded"),
            "配置已从磁盘重新加载"
        );
        // Status labels
        assert_eq!(translator.text("gw-status-configured"), "已配置");
        assert_eq!(translator.text("gw-status-enabled"), "已启用");
        assert_eq!(translator.text("gw-status-disabled"), "已禁用");
        assert_eq!(translator.text("gw-status-runtime"), "运行状态");
        assert_eq!(translator.text("gw-status-running"), "运行中");
        assert_eq!(translator.text("gw-status-stopped"), "已停止");
        assert_eq!(translator.text("gw-status-auth"), "认证");
        assert_eq!(translator.text("gw-status-auth-configured"), "已配置");
        assert_eq!(translator.text("gw-status-auth-not-configured"), "未配置");
        // Tailscale
        assert_eq!(translator.text("gw-ts-heading"), "Tailscale");
        assert_eq!(translator.text("gw-ts-mode-off"), "关闭");
        assert_eq!(translator.text("gw-ts-mode-serve"), "Serve（仅 tailnet）");
        assert_eq!(translator.text("gw-ts-mode-funnel"), "Funnel（公共）");
        assert_eq!(translator.text("gw-ts-host-connected"), "已连接");
        assert_eq!(translator.text("gw-ts-host-disconnected"), "已断开");
        // Config window
        assert_eq!(translator.text("gw-cfg-title"), "网关配置");
        assert_eq!(translator.text("gw-cfg-basic"), "基本");
        assert_eq!(translator.text("gw-cfg-enabled"), "已启用");
        assert_eq!(
            translator.text("gw-cfg-enabled-hint"),
            "启用或禁用网关服务。"
        );
        assert_eq!(translator.text("gw-cfg-listen-ip"), "监听 IP");
        assert_eq!(
            translator.text("gw-cfg-listen-ip-hint"),
            "网关绑定的 IP 地址。使用 0.0.0.0 监听所有接口。"
        );
        assert_eq!(translator.text("gw-cfg-listen-port"), "监听端口");
        assert_eq!(
            translator.text("gw-cfg-listen-port-hint"),
            "网关的端口号。0 表示自动选择。"
        );
        assert_eq!(translator.text("gw-cfg-port-auto"), "(0 = 自动)");
        assert_eq!(translator.text("gw-cfg-auth"), "认证");
        assert_eq!(translator.text("gw-cfg-auth-enabled"), "已启用");
        assert_eq!(
            translator.text("gw-cfg-auth-enabled-hint"),
            "要求网关连接使用认证令牌。"
        );
        assert_eq!(translator.text("gw-cfg-auth-token"), "令牌");
        assert_eq!(
            translator.text("gw-cfg-auth-token-hint"),
            "用于认证网关客户端的密钥令牌。"
        );
        assert_eq!(translator.text("gw-btn-generate"), "生成");
        assert_eq!(translator.text("gw-btn-reload"), "重载");
        assert_eq!(translator.text("gw-btn-save"), "保存");
    }

    #[test]
    fn gui_gateway_panel_translates_notifications_with_args_in_english() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
        assert_eq!(
            translator.text_args(
                "gw-status-unavailable",
                HashMap::from([("error", "timeout".to_string())])
            ),
            "Gateway status unavailable: timeout"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-started-at",
                HashMap::from([("url", "ws://127.0.0.1:8080/ws/chat".to_string())])
            ),
            "Gateway started at ws://127.0.0.1:8080/ws/chat"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-restarted-at",
                HashMap::from([("url", "ws://127.0.0.1:8080/ws/chat".to_string())])
            ),
            "Gateway restarted at ws://127.0.0.1:8080/ws/chat"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-tailscale-mode-set",
                HashMap::from([("mode", "serve (tailnet only)".to_string())])
            ),
            "Tailscale mode set to serve (tailnet only)"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-load-failed",
                HashMap::from([("error", "timeout".to_string())])
            ),
            "Failed to load gateway status: timeout"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-start-failed",
                HashMap::from([("error", "refused".to_string())])
            ),
            "Failed to start gateway: refused"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-restart-failed",
                HashMap::from([("error", "refused".to_string())])
            ),
            "Failed to restart gateway: refused"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-tailscale-refresh-failed",
                HashMap::from([("error", "timeout".to_string())])
            ),
            "Failed to refresh tailscale status: timeout"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-save-failed",
                HashMap::from([("error", "invalid".to_string())])
            ),
            "Save failed: invalid"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-reload-failed",
                HashMap::from([("error", "io".to_string())])
            ),
            "Reload failed: io"
        );
        assert_eq!(
            translator.text_args("gw-btn-refresh", HashMap::from([("icon", "⟳".to_string())])),
            "⟳ Refresh"
        );
        assert_eq!(
            translator.text_args("gw-btn-config", HashMap::from([("icon", "⚙".to_string())])),
            "⚙ Config"
        );
        assert_eq!(
            translator.text_args("gw-btn-start", HashMap::from([("icon", "▶".to_string())])),
            "▶ Start"
        );
        assert_eq!(
            translator.text_args("gw-btn-restart", HashMap::from([("icon", "↺".to_string())])),
            "↺ Restart"
        );
        assert_eq!(
            translator.text_args(
                "gw-btn-refresh-ts",
                HashMap::from([("icon", "⟳".to_string())])
            ),
            "⟳ Refresh Tailscale"
        );
    }

    #[test]
    fn gui_gateway_panel_translates_notifications_with_args_in_chinese() {
        let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
        assert_eq!(
            translator.text_args(
                "gw-status-unavailable",
                HashMap::from([("error", "超时".to_string())])
            ),
            "网关状态不可用: 超时"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-started-at",
                HashMap::from([("url", "ws://127.0.0.1:8080/ws/chat".to_string())])
            ),
            "网关已启动于 ws://127.0.0.1:8080/ws/chat"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-restarted-at",
                HashMap::from([("url", "ws://127.0.0.1:8080/ws/chat".to_string())])
            ),
            "网关已重启于 ws://127.0.0.1:8080/ws/chat"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-tailscale-mode-set",
                HashMap::from([("mode", "serve（仅 tailnet）".to_string())])
            ),
            "Tailscale 模式已设置为 serve（仅 tailnet）"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-load-failed",
                HashMap::from([("error", "超时".to_string())])
            ),
            "加载网关状态失败: 超时"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-start-failed",
                HashMap::from([("error", "拒绝".to_string())])
            ),
            "启动网关失败: 拒绝"
        );
        assert_eq!(
            translator.text_args(
                "gw-notify-save-failed",
                HashMap::from([("error", "无效".to_string())])
            ),
            "保存失败: 无效"
        );
        assert_eq!(
            translator.text_args("gw-btn-refresh", HashMap::from([("icon", "⟳".to_string())])),
            "⟳ 刷新"
        );
        assert_eq!(
            translator.text_args("gw-btn-config", HashMap::from([("icon", "⚙".to_string())])),
            "⚙ 配置"
        );
    }
}
