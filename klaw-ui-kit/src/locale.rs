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
}
