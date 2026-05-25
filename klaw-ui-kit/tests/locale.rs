use klaw_ui_kit::{LocaleDomain, Translator, UiLanguage};
use std::collections::HashMap;

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
fn webui_settings_dialog_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("settings-title"), "Settings");
    assert_eq!(translator.text("settings-general"), "General Settings");
    assert_eq!(
        translator.text_args(
            "settings-current-theme-mode",
            HashMap::from([("mode", "System".to_string())])
        ),
        "Current theme mode: System"
    );
    assert_eq!(translator.text("settings-theme-mode"), "Theme Mode");
    assert_eq!(translator.text("settings-light-theme"), "Light Theme");
    assert_eq!(translator.text("settings-dark-theme"), "Dark Theme");
    assert_eq!(
        translator.text("settings-theme-default-hint"),
        "Default keeps the existing egui light/dark visuals."
    );
}

#[test]
fn webui_settings_dialog_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("settings-title"), "设置");
    assert_eq!(translator.text("settings-general"), "常规设置");
    assert_eq!(
        translator.text_args(
            "settings-current-theme-mode",
            HashMap::from([("mode", "系统".to_string())])
        ),
        "当前主题模式：系统"
    );
    assert_eq!(translator.text("settings-theme-mode"), "主题模式");
    assert_eq!(translator.text("settings-light-theme"), "亮色主题");
    assert_eq!(translator.text("settings-dark-theme"), "暗色主题");
    assert_eq!(
        translator.text("settings-theme-default-hint"),
        "默认使用 egui 自带的亮色/暗色外观。"
    );
}

#[test]
fn webui_about_dialog_translates_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("about-title"), "About Klaw");
    assert_eq!(
        translator.text_args(
            "about-version",
            HashMap::from([("version", "0.18.0".to_string())])
        ),
        "Version 0.18.0"
    );
    assert_eq!(translator.text("about-close"), "Close");
}

#[test]
fn webui_about_dialog_translates_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("about-title"), "关于 Klaw");
    assert_eq!(
        translator.text_args(
            "about-version",
            HashMap::from([("version", "0.18.0".to_string())])
        ),
        "版本 0.18.0"
    );
    assert_eq!(translator.text("about-close"), "关闭");
}

#[test]
fn webui_session_list_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("session-list-heading"), "Agents");
    assert_eq!(translator.text("session-list-empty"), "No agents yet.");
    assert_eq!(translator.text("session-visible"), "Window visible");
    assert_eq!(translator.text("session-hidden"), "Window hidden");
    assert_eq!(translator.text("session-rename"), "Rename");
    assert_eq!(translator.text("session-copy-id"), "Copy ID");
    assert_eq!(translator.text("session-delete"), "Delete");
    assert_eq!(translator.text("session-id-copied"), "Agent ID copied");
}

#[test]
fn webui_session_list_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("session-list-heading"), "代理");
    assert_eq!(translator.text("session-list-empty"), "暂无代理。");
    assert_eq!(translator.text("session-visible"), "窗口可见");
    assert_eq!(translator.text("session-hidden"), "窗口隐藏");
    assert_eq!(translator.text("session-rename"), "重命名");
    assert_eq!(translator.text("session-copy-id"), "复制 ID");
    assert_eq!(translator.text("session-delete"), "删除");
    assert_eq!(translator.text("session-id-copied"), "代理 ID 已复制");
}

#[test]
fn webui_dialogs_translate_labels_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    // Rename dialog
    assert_eq!(translator.text("rename-title"), "Rename Agent");
    assert_eq!(translator.text("rename-hint"), "Agent name");
    assert_eq!(translator.text("rename-save"), "Save");
    assert_eq!(translator.text("rename-cancel"), "Cancel");

    // Gateway dialog
    assert_eq!(translator.text("gateway-title"), "Gateway Token");
    assert_eq!(
        translator.text("gateway-hint"),
        "If gateway auth is enabled, enter the token here."
    );
    assert_eq!(
        translator.text("gateway-blank-hint"),
        "Leave it blank when auth is disabled."
    );
    assert_eq!(translator.text("gateway-token-hint"), "Gateway token");
    assert_eq!(
        translator.text("gateway-save-reconnect"),
        "Save & Reconnect"
    );
    assert_eq!(translator.text("gateway-clear"), "Clear");

    // Delete dialog
    assert_eq!(translator.text("delete-title"), "Delete Agent");
    let delete_msg = translator.text_args(
        "delete-confirmation",
        HashMap::from([("agent_name", "My Agent".to_string())]),
    );
    assert!(delete_msg.contains("My Agent"));
    assert!(delete_msg.contains("permanently"));
    assert!(delete_msg.starts_with("Delete agent"));
    assert!(delete_msg.ends_with("This cannot be undone."));
    assert_eq!(translator.text("delete-confirm"), "Delete");
    assert_eq!(translator.text("delete-cancel"), "Cancel");
}

#[test]
fn webui_dialogs_translate_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    // Rename dialog
    assert_eq!(translator.text("rename-title"), "重命名代理");
    assert_eq!(translator.text("rename-hint"), "代理名称");
    assert_eq!(translator.text("rename-save"), "保存");
    assert_eq!(translator.text("rename-cancel"), "取消");

    // Gateway dialog
    assert_eq!(translator.text("gateway-title"), "网关令牌");
    assert_eq!(
        translator.text("gateway-hint"),
        "如果网关认证已启用，请在此输入令牌。"
    );
    assert_eq!(
        translator.text("gateway-blank-hint"),
        "认证未启用时留空即可。"
    );
    assert_eq!(translator.text("gateway-token-hint"), "网关令牌");
    assert_eq!(translator.text("gateway-save-reconnect"), "保存并重新连接");
    assert_eq!(translator.text("gateway-clear"), "清除");

    // Delete dialog
    assert_eq!(translator.text("delete-title"), "删除代理");
    assert_eq!(
        translator.text_args(
            "delete-confirmation",
            HashMap::from([("agent_name", "我的代理".to_string())])
        ),
        "确定永久删除代理「我的代理」？此操作不可撤销。"
    );
    assert_eq!(translator.text("delete-confirm"), "删除");
    assert_eq!(translator.text("delete-cancel"), "取消");
}

#[test]
fn webui_composer_area_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(
        translator.text("composer-slash-hint"),
        "Type / to open command completion."
    );
    assert_eq!(translator.text("composer-connected-hint"), "Message Klaw…");
    assert_eq!(
        translator.text("composer-connecting-hint"),
        "Connecting to Klaw…"
    );
    assert_eq!(
        translator.text("composer-disconnected-hint"),
        "Reconnect to message Klaw…"
    );
    assert_eq!(
        translator.text("composer-error-hint"),
        "Fix the connection to keep chatting…"
    );
    assert_eq!(translator.text("upload"), "Upload");
    assert_eq!(translator.text("upload-hover"), "Upload and attach files");
    assert_eq!(
        translator.text_args("file-count", HashMap::from([("count", "3".to_string())])),
        "File (3)"
    );
    assert_eq!(translator.text("file-count-hover"), "Show uploaded files");
    assert_eq!(translator.text("send"), "Send");
    assert_eq!(translator.text("model-hint"), "Model");
    assert_eq!(translator.text("provider-hint"), "Provider");
    assert_eq!(translator.text("selecting-file"), "Selecting file…");
    assert_eq!(translator.text("uploading"), "Uploading…");
    assert_eq!(
        translator.text("send-card-failed"),
        "Failed to send card action."
    );
}

#[test]
fn webui_composer_area_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(
        translator.text("composer-slash-hint"),
        "输入 / 打开命令补全。"
    );
    assert_eq!(
        translator.text("composer-connected-hint"),
        "给 Klaw 发消息…"
    );
    assert_eq!(
        translator.text("composer-connecting-hint"),
        "正在连接 Klaw…"
    );
    assert_eq!(
        translator.text("composer-disconnected-hint"),
        "请重新连接后再给 Klaw 发消息…"
    );
    assert_eq!(
        translator.text("composer-error-hint"),
        "请修复连接后继续对话…"
    );
    assert_eq!(translator.text("upload"), "上传");
    assert_eq!(translator.text("upload-hover"), "上传并附加文件");
    assert_eq!(
        translator.text_args("file-count", HashMap::from([("count", "3".to_string())])),
        "文件 (3)"
    );
    assert_eq!(translator.text("file-count-hover"), "显示已上传文件");
    assert_eq!(translator.text("send"), "发送");
    assert_eq!(translator.text("model-hint"), "模型");
    assert_eq!(translator.text("provider-hint"), "提供商");
    assert_eq!(translator.text("selecting-file"), "选择文件…");
    assert_eq!(translator.text("uploading"), "上传中…");
    assert_eq!(translator.text("send-card-failed"), "发送卡片操作失败。");
}

#[test]
fn webui_workbench_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(
        translator.text("workbench-connect-heading"),
        "Connect to Klaw Gateway"
    );
    assert_eq!(
        translator.text("workbench-connect-body"),
        "Connect successfully before loading agents."
    );
    assert_eq!(translator.text("workbench-connect-button"), "Connect");
    assert_eq!(
        translator.text("workbench-loading"),
        "Loading agents from Klaw gateway…"
    );
    assert_eq!(
        translator.text("workbench-no-agents"),
        "No agents yet. Click New Agent to start."
    );
    assert_eq!(translator.text("workbench-heading"), "Agent Workspace");
    assert_eq!(
        translator.text("workbench-subheading"),
        "Each agent opens as its own egui window."
    );
}

#[test]
fn webui_workbench_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(
        translator.text("workbench-connect-heading"),
        "连接到 Klaw 网关"
    );
    assert_eq!(
        translator.text("workbench-connect-body"),
        "请先成功连接后再加载代理。"
    );
    assert_eq!(translator.text("workbench-connect-button"), "连接");
    assert_eq!(
        translator.text("workbench-loading"),
        "正在从 Klaw 网关加载代理…"
    );
    assert_eq!(
        translator.text("workbench-no-agents"),
        "暂无代理。点击新建代理开始。"
    );
    assert_eq!(translator.text("workbench-heading"), "代理工作区");
    assert_eq!(
        translator.text("workbench-subheading"),
        "每个代理以独立窗口打开。"
    );
}

#[test]
fn webui_statusbar_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("statusbar-theme-mode"), "Theme Mode");
    assert_eq!(
        translator.text_args(
            "statusbar-agents",
            HashMap::from([("total", "5".to_string()), ("open", "3".to_string())])
        ),
        "Agents: 5/3"
    );
    assert_eq!(
        translator.text("statusbar-agents-hover"),
        "Total agent windows / currently open windows."
    );
    assert_eq!(translator.text("statusbar-stream"), "Stream");
    assert_eq!(
        translator.text("statusbar-stream-on-hover"),
        "On: stream replies live. Off: wait for a full reply and play fade-in."
    );
    assert_eq!(
        translator.text("statusbar-fps-hover"),
        "Approximate live frame rate from the latest egui frame delta."
    );
    assert_eq!(
        translator.text("statusbar-activity-hover"),
        "Current activity for the active agent."
    );
    assert_eq!(
        translator.text("statusbar-messages-hover"),
        "Messages currently loaded in the active agent window."
    );
    assert_eq!(
        translator.text("statusbar-no-active-agent"),
        "No active agent"
    );
}

#[test]
fn webui_statusbar_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("statusbar-theme-mode"), "主题模式");
    assert_eq!(
        translator.text_args(
            "statusbar-agents",
            HashMap::from([("total", "5".to_string()), ("open", "3".to_string())])
        ),
        "代理：5/3"
    );
    assert_eq!(
        translator.text("statusbar-agents-hover"),
        "代理总数 / 当前打开的窗口数。"
    );
    assert_eq!(translator.text("statusbar-stream"), "流式");
    assert_eq!(
        translator.text("statusbar-stream-on-hover"),
        "开启：实时流式回复。关闭：等待完整回复后淡入显示。"
    );
    assert_eq!(
        translator.text("statusbar-fps-hover"),
        "基于最新 egui 帧间隔的近似实时帧率。"
    );
    assert_eq!(
        translator.text("statusbar-activity-hover"),
        "当前活跃代理的活动状态。"
    );
    assert_eq!(
        translator.text("statusbar-messages-hover"),
        "活跃代理窗口中已加载的消息数。"
    );
    assert_eq!(translator.text("statusbar-no-active-agent"), "无活跃代理");
}

#[test]
fn webui_empty_state_translates_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(
        translator.text("empty-connected-title"),
        "Start a conversation with Klaw"
    );
    assert_eq!(
        translator.text("empty-connected-body"),
        "Send a message below to begin this chat."
    );
    assert_eq!(
        translator.text("empty-connecting-title"),
        "Connecting to Klaw"
    );
    assert_eq!(
        translator.text("empty-connecting-body"),
        "Waiting for the chat room to come online."
    );
    assert_eq!(
        translator.text("empty-disconnected-title"),
        "Reconnect to Klaw"
    );
    assert_eq!(
        translator.text("empty-disconnected-body"),
        "Reconnect from the toolbar, then send your next message."
    );
    assert_eq!(translator.text("empty-error-title"), "Connection error");
    assert_eq!(
        translator.text_args(
            "empty-error-body",
            HashMap::from([("error", "send failed".to_string())])
        ),
        "Klaw could not keep the chat connection alive: send failed"
    );
}

#[test]
fn webui_empty_state_translates_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("empty-connected-title"), "开始与 Klaw 对话");
    assert_eq!(
        translator.text("empty-connected-body"),
        "在下方发送消息开始此对话。"
    );
    assert_eq!(translator.text("empty-connecting-title"), "正在连接 Klaw");
    assert_eq!(
        translator.text("empty-connecting-body"),
        "正在等待聊天服务上线。"
    );
    assert_eq!(translator.text("empty-disconnected-title"), "重新连接 Klaw");
    assert_eq!(
        translator.text("empty-disconnected-body"),
        "请从工具栏重新连接，然后发送您的下一条消息。"
    );
    assert_eq!(translator.text("empty-error-title"), "连接错误");
    assert_eq!(
        translator.text_args(
            "empty-error-body",
            HashMap::from([("error", "发送失败".to_string())])
        ),
        "Klaw 无法保持聊天连接：发送失败"
    );
}

#[test]
fn webui_card_messages_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("card-approval-badge"), "Approval");
    assert_eq!(translator.text("card-question-badge"), "Question");
    assert_eq!(translator.text("card-approval-title"), "Approval Required");
    assert_eq!(translator.text("card-question-title"), "Question");
    assert_eq!(translator.text("card-command-label"), "Command");
    assert_eq!(
        translator.text_args(
            "card-approval-id",
            HashMap::from([("id", "abc123".to_string())])
        ),
        "Approval ID: abc123"
    );
    assert_eq!(
        translator.text_args(
            "card-selected-answer",
            HashMap::from([("answer", "Option A".to_string())])
        ),
        "Selected: Option A"
    );
}

#[test]
fn webui_card_messages_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("card-approval-badge"), "审批");
    assert_eq!(translator.text("card-question-badge"), "问题");
    assert_eq!(translator.text("card-approval-title"), "需要审批");
    assert_eq!(translator.text("card-question-title"), "问题");
    assert_eq!(translator.text("card-command-label"), "命令");
    assert_eq!(
        translator.text_args(
            "card-approval-id",
            HashMap::from([("id", "abc123".to_string())])
        ),
        "审批 ID：abc123"
    );
    assert_eq!(
        translator.text_args(
            "card-selected-answer",
            HashMap::from([("answer", "选项 A".to_string())])
        ),
        "已选择：选项 A"
    );
}

#[test]
fn webui_file_dialog_translates_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("file-dialog-title"), "Uploaded Files");
    assert_eq!(
        translator.text("file-dialog-hint"),
        "Right-click a row to preview or remove it from this page."
    );
    assert_eq!(translator.text("file-dialog-empty"), "No uploaded files.");
    assert_eq!(translator.text("file-dialog-col-name"), "File Name");
    assert_eq!(translator.text("file-dialog-col-archive-id"), "Archive ID");
    assert_eq!(translator.text("file-dialog-col-size"), "Size");
}

#[test]
fn webui_file_dialog_translates_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("file-dialog-title"), "已上传文件");
    assert_eq!(
        translator.text("file-dialog-hint"),
        "右键点击行可预览或从当前页面移除。"
    );
    assert_eq!(translator.text("file-dialog-empty"), "无已上传文件。");
    assert_eq!(translator.text("file-dialog-col-name"), "文件名");
    assert_eq!(translator.text("file-dialog-col-archive-id"), "存档 ID");
    assert_eq!(translator.text("file-dialog-col-size"), "大小");
}

#[test]
fn webui_attachment_context_menu_translates_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("attachment-preview"), "Preview");
    assert_eq!(translator.text("attachment-download"), "Download");
    assert_eq!(translator.text("attachment-delete"), "Delete");
}

#[test]
fn webui_attachment_context_menu_translates_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("attachment-preview"), "预览");
    assert_eq!(translator.text("attachment-download"), "下载");
    assert_eq!(translator.text("attachment-delete"), "删除");
}

#[test]
fn webui_archive_preview_translates_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("archive-preview-title"), "Resource Preview");
    assert_eq!(translator.text("archive-preview-close"), "Close");
    assert_eq!(translator.text("archive-preview-download"), "Download");
}

#[test]
fn webui_archive_preview_translates_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("archive-preview-title"), "资源预览");
    assert_eq!(translator.text("archive-preview-close"), "关闭");
    assert_eq!(translator.text("archive-preview-download"), "下载");
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
    assert_eq!(
        translator.text("system-disk-usage-chart-title"),
        "Usage Breakdown"
    );
    assert_eq!(
        translator.text_args(
            "system-disk-usage-chart-total",
            HashMap::from([("total", "3.00 KB".to_string())])
        ),
        "Total tracked usage: 3.00 KB"
    );
    assert_eq!(
        translator.text("system-disk-usage-chart-loading"),
        "Calculating usage..."
    );
    assert_eq!(
        translator.text("system-disk-usage-chart-empty"),
        "No tracked usage to chart yet."
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
    assert_eq!(translator.text("system-disk-usage-chart-title"), "使用占比");
    assert_eq!(
        translator.text_args(
            "system-disk-usage-chart-total",
            HashMap::from([("total", "3.00 KB".to_string())])
        ),
        "已跟踪总用量: 3.00 KB"
    );
    assert_eq!(
        translator.text("system-disk-usage-chart-loading"),
        "正在计算用量..."
    );
    assert_eq!(
        translator.text("system-disk-usage-chart-empty"),
        "暂无可展示的已跟踪用量。"
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
    assert_eq!(
        translator.text("archive-subtitle"),
        "Browse, filter, and preview archived files and attachments stored in the workspace."
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
    assert_eq!(
        translator.text("archive-subtitle"),
        "浏览、筛选和预览工作区中存储的归档文件和附件。"
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
fn gui_voice_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("voice-subtitle"),
        "Manage voice providers and run split STT/TTS voice tests."
    );
    assert_eq!(translator.text("voice-btn-config"), "{$icon} Config");
    assert_eq!(translator.text("voice-btn-reload"), "{$icon} Reload");
    assert_eq!(
        translator.text("voice-section-current-config"),
        "Current Config"
    );
    assert_eq!(translator.text("voice-col-enabled"), "Enabled");
    assert_eq!(translator.text("voice-col-stt-provider"), "STT Provider");
    assert_eq!(translator.text("voice-col-tts-provider"), "TTS Provider");
    assert_eq!(
        translator.text("voice-col-default-language"),
        "Default Language"
    );
    assert_eq!(
        translator.text("voice-col-default-voice-id"),
        "Default Voice ID"
    );
    assert_eq!(translator.text("voice-section-voice-tests"), "Voice Tests");
    assert_eq!(translator.text("voice-stt-tab"), "STT Test");
    assert_eq!(translator.text("voice-tts-tab"), "TTS Test");
    assert_eq!(
        translator.text("voice-stt-subtitle"),
        "Capture live microphone audio and send it to the configured STT provider."
    );
    assert_eq!(
        translator.text("voice-tts-subtitle"),
        "Enter text, synthesize it through the configured TTS provider, save it into tmp, and play it back inside the GUI."
    );
    assert_eq!(translator.text("voice-config-title"), "Voice Config");
    assert_eq!(
        translator.text("voice-config-subtitle"),
        "Edit voice provider configuration stored in config.toml."
    );
    assert_eq!(translator.text("voice-config-tab-general"), "General");
    assert_eq!(translator.text("voice-config-tab-deepgram"), "Deepgram");
    assert_eq!(translator.text("voice-config-tab-assemblyai"), "AssemblyAI");
    assert_eq!(translator.text("voice-config-tab-elevenlabs"), "ElevenLabs");
    assert_eq!(translator.text("voice-cfg-label-enabled"), "Enabled");
    assert_eq!(
        translator.text("voice-cfg-hint-enabled"),
        "Enable voice runtime"
    );
    assert_eq!(translator.text("voice-cfg-label-api-key"), "API Key");
    assert_eq!(translator.text("voice-cfg-label-base-url"), "Base URL");
    assert_eq!(
        translator.text("voice-cfg-label-streaming-base-url"),
        "Streaming Base URL"
    );
    assert_eq!(translator.text("voice-cfg-label-stt-model"), "STT Model");
    assert_eq!(
        translator.text("voice-cfg-label-default-model"),
        "Default Model"
    );
    assert_eq!(
        translator.text("voice-cfg-label-provider-default-voice-id"),
        "Provider Default Voice ID"
    );
    assert_eq!(
        translator.text("voice-notify-config-saved"),
        "Voice config saved"
    );
    assert_eq!(
        translator.text("voice-notify-config-reloaded"),
        "Voice config reloaded from disk"
    );
    assert_eq!(
        translator.text("voice-notify-store-unavailable"),
        "Configuration store is not available"
    );
}

#[test]
fn gui_voice_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("voice-subtitle"),
        "管理语音提供商，并分别运行 STT/TTS 语音测试。"
    );
    assert_eq!(translator.text("voice-btn-config"), "{$icon} 配置");
    assert_eq!(translator.text("voice-btn-reload"), "{$icon} 重载");
    assert_eq!(translator.text("voice-section-current-config"), "当前配置");
    assert_eq!(translator.text("voice-col-enabled"), "已启用");
    assert_eq!(translator.text("voice-col-stt-provider"), "STT 提供商");
    assert_eq!(translator.text("voice-col-tts-provider"), "TTS 提供商");
    assert_eq!(translator.text("voice-col-default-language"), "默认语言");
    assert_eq!(translator.text("voice-col-default-voice-id"), "默认语音 ID");
    assert_eq!(translator.text("voice-section-voice-tests"), "语音测试");
    assert_eq!(translator.text("voice-stt-tab"), "STT 测试");
    assert_eq!(translator.text("voice-tts-tab"), "TTS 测试");
    assert_eq!(
        translator.text("voice-stt-subtitle"),
        "捕获麦克风音频并发送到已配置的 STT 提供商。"
    );
    assert_eq!(
        translator.text("voice-tts-subtitle"),
        "输入文本，通过已配置的 TTS 提供商合成语音，保存到临时文件并在 GUI 中播放。"
    );
    assert_eq!(translator.text("voice-config-title"), "语音配置");
    assert_eq!(
        translator.text("voice-config-subtitle"),
        "编辑存储在 config.toml 中的语音提供商配置。"
    );
    assert_eq!(translator.text("voice-config-tab-general"), "通用");
    assert_eq!(translator.text("voice-config-tab-deepgram"), "Deepgram");
    assert_eq!(translator.text("voice-config-tab-assemblyai"), "AssemblyAI");
    assert_eq!(translator.text("voice-config-tab-elevenlabs"), "ElevenLabs");
    assert_eq!(translator.text("voice-cfg-label-enabled"), "已启用");
    assert_eq!(translator.text("voice-cfg-hint-enabled"), "启用语音运行时");
    assert_eq!(translator.text("voice-cfg-label-api-key"), "API Key");
    assert_eq!(translator.text("voice-cfg-label-base-url"), "Base URL");
    assert_eq!(
        translator.text("voice-cfg-label-streaming-base-url"),
        "Streaming Base URL"
    );
    assert_eq!(translator.text("voice-cfg-label-stt-model"), "STT 模型");
    assert_eq!(translator.text("voice-cfg-label-default-model"), "默认模型");
    assert_eq!(
        translator.text("voice-cfg-label-provider-default-voice-id"),
        "提供商默认语音 ID"
    );
    assert_eq!(
        translator.text("voice-notify-config-saved"),
        "语音配置已保存"
    );
    assert_eq!(
        translator.text("voice-notify-config-reloaded"),
        "语音配置已从磁盘重载"
    );
    assert_eq!(
        translator.text("voice-notify-store-unavailable"),
        "配置存储不可用"
    );
}

#[test]
fn gui_voice_panel_translates_parameterized_keys_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "voice-btn-config",
            HashMap::from([("icon", "⚙".to_string())])
        ),
        "⚙ Config"
    );
    assert_eq!(
        translator.text_args(
            "voice-btn-reload",
            HashMap::from([("icon", "↻".to_string())])
        ),
        "↻ Reload"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-btn-start",
            HashMap::from([("icon", "🎤".to_string())])
        ),
        "🎤 Start Recording"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-btn-stop",
            HashMap::from([("icon", "⏹".to_string())])
        ),
        "⏹ Stop Recording"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-btn-generate",
            HashMap::from([("icon", "〰".to_string())])
        ),
        "〰 Generate Audio"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-btn-play",
            HashMap::from([("icon", "▶".to_string())])
        ),
        "▶ Play"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-btn-stop",
            HashMap::from([("icon", "⏹".to_string())])
        ),
        "⏹ Stop"
    );
    assert_eq!(
        translator.text_args(
            "voice-notify-save-failed",
            HashMap::from([("error", "bad data".to_string())])
        ),
        "Save failed: bad data"
    );
    assert_eq!(
        translator.text_args(
            "voice-notify-reload-failed",
            HashMap::from([("error", "disk error".to_string())])
        ),
        "Reload failed: disk error"
    );
    assert_eq!(
        translator.text_args(
            "voice-notify-tts-completed",
            HashMap::from([("path", "/tmp/audio.mp3".to_string())])
        ),
        "Voice synthesis completed and saved to /tmp/audio.mp3"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-recording-detail",
            HashMap::from([
                ("device", "MacBook Mic".to_string()),
                ("sample_rate", "48000".to_string()),
                ("channels", "1".to_string()),
                ("elapsed", "500".to_string()),
            ])
        ),
        "Recording from MacBook Mic at 48000 Hz / 1 ch for 500 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-transcribing-detail",
            HashMap::from([
                ("duration", "2000".to_string()),
                ("queued", "300".to_string()),
            ])
        ),
        "Transcribing 2000 ms recording… queued for 300 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-duration-ms",
            HashMap::from([("value", "2000".to_string())])
        ),
        "2000 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-audio-format-detail",
            HashMap::from([
                ("sample_rate", "48000".to_string()),
                ("channels", "2".to_string()),
                ("samples", "96000".to_string()),
            ])
        ),
        "48000 Hz / 2 ch / 96000 samples"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-provider-duration-value",
            HashMap::from([("value", "1500".to_string())])
        ),
        "1500 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-synthesizing-detail",
            HashMap::from([("queued", "800".to_string())])
        ),
        "Synthesizing audio… queued for 800 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-output-size-detail",
            HashMap::from([("value", "1024".to_string())])
        ),
        "1024 bytes"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-provider-duration-value",
            HashMap::from([("value", "3200".to_string())])
        ),
        "3200 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-playback-playing",
            HashMap::from([("path", "/tmp/out.wav".to_string())])
        ),
        "Playing /tmp/out.wav"
    );
}

#[test]
fn gui_voice_panel_translates_parameterized_keys_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "voice-btn-config",
            HashMap::from([("icon", "⚙".to_string())])
        ),
        "⚙ 配置"
    );
    assert_eq!(
        translator.text_args(
            "voice-btn-reload",
            HashMap::from([("icon", "↻".to_string())])
        ),
        "↻ 重载"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-btn-start",
            HashMap::from([("icon", "🎤".to_string())])
        ),
        "🎤 开始录音"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-btn-stop",
            HashMap::from([("icon", "⏹".to_string())])
        ),
        "⏹ 停止录音"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-btn-generate",
            HashMap::from([("icon", "〰".to_string())])
        ),
        "〰 生成音频"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-btn-play",
            HashMap::from([("icon", "▶".to_string())])
        ),
        "▶ 播放"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-btn-stop",
            HashMap::from([("icon", "⏹".to_string())])
        ),
        "⏹ 停止"
    );
    assert_eq!(
        translator.text_args(
            "voice-notify-save-failed",
            HashMap::from([("error", "bad data".to_string())])
        ),
        "保存失败: bad data"
    );
    assert_eq!(
        translator.text_args(
            "voice-notify-reload-failed",
            HashMap::from([("error", "disk error".to_string())])
        ),
        "重载失败: disk error"
    );
    assert_eq!(
        translator.text_args(
            "voice-notify-tts-completed",
            HashMap::from([("path", "/tmp/audio.mp3".to_string())])
        ),
        "语音合成已完成并保存至 /tmp/audio.mp3"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-recording-detail",
            HashMap::from([
                ("device", "MacBook Mic".to_string()),
                ("sample_rate", "48000".to_string()),
                ("channels", "1".to_string()),
                ("elapsed", "500".to_string()),
            ])
        ),
        "从 MacBook Mic 录音，48000 Hz / 1 通道，已录 500 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-transcribing-detail",
            HashMap::from([
                ("duration", "2000".to_string()),
                ("queued", "300".to_string()),
            ])
        ),
        "转录 2000 ms 录音… 已排队 300 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-duration-ms",
            HashMap::from([("value", "2000".to_string())])
        ),
        "2000 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-audio-format-detail",
            HashMap::from([
                ("sample_rate", "48000".to_string()),
                ("channels", "2".to_string()),
                ("samples", "96000".to_string()),
            ])
        ),
        "48000 Hz / 2 通道 / 96000 样本"
    );
    assert_eq!(
        translator.text_args(
            "voice-stt-provider-duration-value",
            HashMap::from([("value", "1500".to_string())])
        ),
        "1500 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-synthesizing-detail",
            HashMap::from([("queued", "800".to_string())])
        ),
        "合成音频… 已排队 800 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-output-size-detail",
            HashMap::from([("value", "1024".to_string())])
        ),
        "1024 字节"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-provider-duration-value",
            HashMap::from([("value", "3200".to_string())])
        ),
        "3200 ms"
    );
    assert_eq!(
        translator.text_args(
            "voice-tts-playback-playing",
            HashMap::from([("path", "/tmp/out.wav".to_string())])
        ),
        "播放 /tmp/out.wav"
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

#[test]
fn gui_cron_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("cron-subtitle"),
        "Schedule and manage recurring automated tasks."
    );
    assert_eq!(translator.text("cron-form-title-edit"), "Edit Cron Job");
    assert_eq!(translator.text("cron-form-title-add"), "Add Cron Job");
    assert_eq!(translator.text("cron-col-id"), "ID");
    assert_eq!(translator.text("cron-col-name"), "Name");
    assert_eq!(translator.text("cron-col-kind"), "Kind");
    assert_eq!(translator.text("cron-col-expr"), "Expr");
    assert_eq!(translator.text("cron-col-enabled"), "Enabled");
    assert_eq!(translator.text("cron-col-next-run"), "Next Run At");
    assert_eq!(translator.text("cron-col-last-run"), "Last Run At");
    assert_eq!(translator.text("cron-col-updated-at"), "Updated At");
    assert_eq!(
        translator.text("cron-no-rows"),
        "No cron jobs found in database."
    );
    assert_eq!(translator.text("cron-delete-title"), "Delete cron job");
    assert_eq!(translator.text("cron-delete-btn"), "Delete");
    assert_eq!(translator.text("cron-delete-cancel"), "Cancel");
    assert_eq!(translator.text("cron-form-id"), "ID");
    assert_eq!(translator.text("cron-form-name"), "Name");
    assert_eq!(translator.text("cron-form-schedule-kind"), "Schedule Kind");
    assert_eq!(translator.text("cron-form-schedule-expr"), "Schedule Expr");
    assert_eq!(
        translator.text("cron-form-schedule-expr-hint"),
        "The schedule expression (e.g. 5m for every, or standard cron format)."
    );
    assert_eq!(translator.text("cron-form-timezone"), "Timezone");
    assert_eq!(translator.text("cron-form-enabled"), "Enabled");
    assert_eq!(
        translator.text("cron-form-enabled-hint"),
        "Enable or disable this cron job."
    );
    assert_eq!(translator.text("cron-form-payload"), "Payload JSON");
    assert_eq!(
        translator.text("cron-form-payload-hint"),
        "JSON payload matching the InboundMessage schema."
    );
    assert_eq!(translator.text("cron-form-save"), "Save");
    assert_eq!(translator.text("cron-form-cancel"), "Cancel");
    assert_eq!(translator.text("cron-runs-refresh"), "Refresh Runs");
    assert_eq!(translator.text("cron-runs-run-now"), "Run Now");
    assert_eq!(translator.text("cron-runs-no-rows"), "No task runs found.");
    assert_eq!(translator.text("cron-runs-col-id"), "Run ID");
    assert_eq!(translator.text("cron-runs-col-status"), "Status");
    assert_eq!(translator.text("cron-runs-col-scheduled"), "Scheduled At");
    assert_eq!(translator.text("cron-runs-col-started"), "Started At");
    assert_eq!(translator.text("cron-runs-col-finished"), "Finished At");
    assert_eq!(translator.text("cron-runs-col-error"), "Error");
    assert_eq!(translator.text("cron-status-pending"), "pending");
    assert_eq!(translator.text("cron-status-running"), "running");
    assert_eq!(translator.text("cron-status-success"), "success");
    assert_eq!(translator.text("cron-status-failed"), "failed");
    assert_eq!(translator.text("cron-kind-cron"), "cron");
    assert_eq!(translator.text("cron-kind-every"), "every");
    assert_eq!(translator.text("cron-enabled-yes"), "yes");
    assert_eq!(translator.text("cron-enabled-no"), "no");
    assert_eq!(translator.text("cron-filter-name"), "Name");
    assert_eq!(translator.text("cron-filter-kind"), "Kind");
    assert_eq!(translator.text("cron-filter-kind-all"), "All");
    assert_eq!(translator.text("cron-filter-kind-cron"), "cron");
    assert_eq!(translator.text("cron-filter-kind-every"), "every");
    assert_eq!(translator.text("cron-filter-created-from"), "Created From");
    assert_eq!(translator.text("cron-filter-created-to"), "Created To");
    assert_eq!(translator.text("cron-filter-page"), "Page");
    assert_eq!(translator.text("cron-filter-size"), "Size");
    assert_eq!(translator.text("cron-sort-updated-desc"), "Updated At ↓");
    assert_eq!(translator.text("cron-sort-created-desc"), "Created At ↓");
    assert_eq!(translator.text("cron-sort-updated-asc"), "Updated At ↑");
    assert_eq!(translator.text("cron-sort-created-asc"), "Created At ↑");
    assert_eq!(
        translator.text("cron-notify-form-unavailable"),
        "Cron form is not available"
    );
    assert_eq!(
        translator.text("cron-notify-id-empty"),
        "Cron ID cannot be empty"
    );
    assert_eq!(
        translator.text("cron-notify-name-empty"),
        "Cron name cannot be empty"
    );
    assert_eq!(
        translator.text("cron-notify-expr-empty"),
        "Schedule expression cannot be empty"
    );
    assert_eq!(
        translator.text("cron-notify-payload-empty"),
        "Payload JSON cannot be empty"
    );
    assert_eq!(
        translator.text("cron-notify-timezone-empty"),
        "Timezone cannot be empty"
    );
    assert_eq!(translator.text("cron-notify-updated"), "Cron job updated");
    assert_eq!(translator.text("cron-notify-created"), "Cron job created");
    assert_eq!(translator.text("cron-notify-enabled"), "Cron enabled");
    assert_eq!(translator.text("cron-notify-disabled"), "Cron disabled");
    assert_eq!(translator.text("cron-notify-deleted"), "Cron job deleted");
    assert_eq!(
        translator.text("cron-notify-already-running"),
        "A cron run is already in progress"
    );
}

#[test]
fn gui_cron_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("cron-subtitle"),
        "计划和管理周期性自动化任务。"
    );
    assert_eq!(translator.text("cron-form-title-edit"), "编辑定时任务");
    assert_eq!(translator.text("cron-form-title-add"), "添加定时任务");
    assert_eq!(translator.text("cron-col-id"), "ID");
    assert_eq!(translator.text("cron-col-name"), "名称");
    assert_eq!(translator.text("cron-col-kind"), "类型");
    assert_eq!(translator.text("cron-col-expr"), "表达式");
    assert_eq!(translator.text("cron-col-enabled"), "启用");
    assert_eq!(translator.text("cron-col-next-run"), "下次运行时间");
    assert_eq!(translator.text("cron-col-last-run"), "上次运行时间");
    assert_eq!(translator.text("cron-col-updated-at"), "更新时间");
    assert_eq!(translator.text("cron-no-rows"), "数据库中未找到定时任务。");
    assert_eq!(translator.text("cron-delete-title"), "删除定时任务");
    assert_eq!(translator.text("cron-delete-btn"), "删除");
    assert_eq!(translator.text("cron-delete-cancel"), "取消");
    assert_eq!(translator.text("cron-form-id"), "ID");
    assert_eq!(translator.text("cron-form-name"), "名称");
    assert_eq!(translator.text("cron-form-schedule-kind"), "调度类型");
    assert_eq!(translator.text("cron-form-schedule-expr"), "调度表达式");
    assert_eq!(
        translator.text("cron-form-schedule-expr-hint"),
        "调度表达式（如 5m 表示 every 间隔，或标准 cron 格式）。"
    );
    assert_eq!(translator.text("cron-form-timezone"), "时区");
    assert_eq!(translator.text("cron-form-enabled"), "启用");
    assert_eq!(
        translator.text("cron-form-enabled-hint"),
        "启用或禁用此定时任务。"
    );
    assert_eq!(translator.text("cron-form-payload"), "Payload JSON");
    assert_eq!(
        translator.text("cron-form-payload-hint"),
        "符合 InboundMessage 格式的 JSON 载荷。"
    );
    assert_eq!(translator.text("cron-form-save"), "保存");
    assert_eq!(translator.text("cron-form-cancel"), "取消");
    assert_eq!(translator.text("cron-runs-refresh"), "刷新运行记录");
    assert_eq!(translator.text("cron-runs-run-now"), "立即运行");
    assert_eq!(translator.text("cron-runs-no-rows"), "未找到任务运行记录。");
    assert_eq!(translator.text("cron-runs-col-id"), "运行 ID");
    assert_eq!(translator.text("cron-runs-col-status"), "状态");
    assert_eq!(translator.text("cron-runs-col-scheduled"), "计划时间");
    assert_eq!(translator.text("cron-runs-col-started"), "开始时间");
    assert_eq!(translator.text("cron-runs-col-finished"), "完成时间");
    assert_eq!(translator.text("cron-runs-col-error"), "错误");
    assert_eq!(translator.text("cron-status-pending"), "待执行");
    assert_eq!(translator.text("cron-status-running"), "运行中");
    assert_eq!(translator.text("cron-status-success"), "成功");
    assert_eq!(translator.text("cron-status-failed"), "失败");
    assert_eq!(translator.text("cron-kind-cron"), "cron");
    assert_eq!(translator.text("cron-kind-every"), "every");
    assert_eq!(translator.text("cron-enabled-yes"), "是");
    assert_eq!(translator.text("cron-enabled-no"), "否");
    assert_eq!(translator.text("cron-filter-name"), "名称");
    assert_eq!(translator.text("cron-filter-kind"), "类型");
    assert_eq!(translator.text("cron-filter-kind-all"), "全部");
    assert_eq!(translator.text("cron-filter-kind-cron"), "cron");
    assert_eq!(translator.text("cron-filter-kind-every"), "every");
    assert_eq!(translator.text("cron-filter-created-from"), "创建起始");
    assert_eq!(translator.text("cron-filter-created-to"), "创建截止");
    assert_eq!(translator.text("cron-filter-page"), "页码");
    assert_eq!(translator.text("cron-filter-size"), "每页数量");
    assert_eq!(translator.text("cron-sort-updated-desc"), "更新时间 ↓");
    assert_eq!(translator.text("cron-sort-created-desc"), "创建时间 ↓");
    assert_eq!(translator.text("cron-sort-updated-asc"), "更新时间 ↑");
    assert_eq!(translator.text("cron-sort-created-asc"), "创建时间 ↑");
    assert_eq!(
        translator.text("cron-notify-form-unavailable"),
        "定时任务表单不可用"
    );
    assert_eq!(
        translator.text("cron-notify-id-empty"),
        "定时任务 ID 不能为空"
    );
    assert_eq!(
        translator.text("cron-notify-name-empty"),
        "定时任务名称不能为空"
    );
    assert_eq!(
        translator.text("cron-notify-expr-empty"),
        "调度表达式不能为空"
    );
    assert_eq!(
        translator.text("cron-notify-payload-empty"),
        "Payload JSON 不能为空"
    );
    assert_eq!(
        translator.text("cron-notify-timezone-empty"),
        "时区不能为空"
    );
    assert_eq!(translator.text("cron-notify-updated"), "定时任务已更新");
    assert_eq!(translator.text("cron-notify-created"), "定时任务已创建");
    assert_eq!(translator.text("cron-notify-enabled"), "定时任务已启用");
    assert_eq!(translator.text("cron-notify-disabled"), "定时任务已禁用");
    assert_eq!(translator.text("cron-notify-deleted"), "定时任务已删除");
    assert_eq!(
        translator.text("cron-notify-already-running"),
        "定时任务运行已在进行中"
    );
}

#[test]
fn gui_cron_panel_translates_notifications_with_args_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "cron-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ Refresh"
    );
    assert_eq!(
        translator.text_args("cron-btn-add", HashMap::from([("icon", "+".to_string())])),
        "+ Add Cron Job"
    );
    assert_eq!(
        translator.text_args(
            "cron-label-total",
            HashMap::from([("count", "3".to_string())])
        ),
        "Total: 3"
    );
    assert_eq!(
        translator.text_args(
            "cron-label-running",
            HashMap::from([("id", "job-1".to_string())])
        ),
        "Running: job-1"
    );
    assert_eq!(
        translator.text_args(
            "cron-delete-prompt",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "Delete cron job 'abc-123'?"
    );
    assert_eq!(
        translator.text_args(
            "cron-runs-title",
            HashMap::from([("id", "job-1".to_string())])
        ),
        "Task Runs: job-1"
    );
    assert_eq!(
        translator.text_args("cron-ctx-runs", HashMap::from([("icon", "📋".to_string())])),
        "📋 Runs"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-run-now",
            HashMap::from([("icon", "▶".to_string())])
        ),
        "▶ Run Now"
    );
    assert_eq!(
        translator.text_args("cron-ctx-edit", HashMap::from([("icon", "✏".to_string())])),
        "✏ Edit"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-disable",
            HashMap::from([("icon", "⚡".to_string())])
        ),
        "⚡ Disable"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-enable",
            HashMap::from([("icon", "⚡".to_string())])
        ),
        "⚡ Enable"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-delete",
            HashMap::from([("icon", "🗑".to_string())])
        ),
        "🗑 Delete"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 Copy ID"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-executed",
            HashMap::from([("id", "msg-99".to_string())])
        ),
        "Cron executed: msg-99"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-run-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to run cron now: timeout"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-list-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to list cron jobs: timeout"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-runs-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to load task runs: timeout"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-payload-invalid",
            HashMap::from([("error", "bad json".to_string())])
        ),
        "Payload JSON is invalid: bad json"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-payload-invalid-schema",
            HashMap::from([("error", "missing field".to_string())])
        ),
        "Payload JSON must be a valid InboundMessage-like object: missing field"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-schedule-invalid",
            HashMap::from([("error", "bad expr".to_string())])
        ),
        "Invalid schedule: bad expr"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-update-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to update cron job: io"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-create-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to create cron job: io"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-set-enabled-failed",
            HashMap::from([("error", "reject".to_string())])
        ),
        "Failed to set enabled: reject"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-delete-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to delete cron job: io"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-running-bg",
            HashMap::from([("id", "job-42".to_string())])
        ),
        "Running cron 'job-42' in background..."
    );
}

#[test]
fn gui_cron_panel_translates_notifications_with_args_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "cron-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ 刷新"
    );
    assert_eq!(
        translator.text_args("cron-btn-add", HashMap::from([("icon", "+".to_string())])),
        "+ 添加定时任务"
    );
    assert_eq!(
        translator.text_args(
            "cron-label-total",
            HashMap::from([("count", "3".to_string())])
        ),
        "总计: 3"
    );
    assert_eq!(
        translator.text_args(
            "cron-label-running",
            HashMap::from([("id", "job-1".to_string())])
        ),
        "运行中: job-1"
    );
    assert_eq!(
        translator.text_args(
            "cron-delete-prompt",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "确定删除定时任务 'abc-123' 吗？"
    );
    assert_eq!(
        translator.text_args(
            "cron-runs-title",
            HashMap::from([("id", "job-1".to_string())])
        ),
        "任务运行记录: job-1"
    );
    assert_eq!(
        translator.text_args("cron-ctx-runs", HashMap::from([("icon", "📋".to_string())])),
        "📋 运行记录"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-run-now",
            HashMap::from([("icon", "▶".to_string())])
        ),
        "▶ 立即运行"
    );
    assert_eq!(
        translator.text_args("cron-ctx-edit", HashMap::from([("icon", "✏".to_string())])),
        "✏ 编辑"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-disable",
            HashMap::from([("icon", "⚡".to_string())])
        ),
        "⚡ 禁用"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-enable",
            HashMap::from([("icon", "⚡".to_string())])
        ),
        "⚡ 启用"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-delete",
            HashMap::from([("icon", "🗑".to_string())])
        ),
        "🗑 删除"
    );
    assert_eq!(
        translator.text_args(
            "cron-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 复制 ID"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-executed",
            HashMap::from([("id", "msg-99".to_string())])
        ),
        "定时任务已执行: msg-99"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-run-failed",
            HashMap::from([("error", "超时".to_string())])
        ),
        "立即运行定时任务失败: 超时"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-list-failed",
            HashMap::from([("error", "超时".to_string())])
        ),
        "加载定时任务列表失败: 超时"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-runs-failed",
            HashMap::from([("error", "超时".to_string())])
        ),
        "加载任务运行记录失败: 超时"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-payload-invalid",
            HashMap::from([("error", "无效".to_string())])
        ),
        "Payload JSON 格式无效: 无效"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-payload-invalid-schema",
            HashMap::from([("error", "缺少字段".to_string())])
        ),
        "Payload JSON 必须是有效的 InboundMessage 对象: 缺少字段"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-schedule-invalid",
            HashMap::from([("error", "无效".to_string())])
        ),
        "调度表达式无效: 无效"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-update-failed",
            HashMap::from([("error", "拒绝".to_string())])
        ),
        "更新定时任务失败: 拒绝"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-create-failed",
            HashMap::from([("error", "拒绝".to_string())])
        ),
        "创建定时任务失败: 拒绝"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-set-enabled-failed",
            HashMap::from([("error", "拒绝".to_string())])
        ),
        "设置启用状态失败: 拒绝"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-delete-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "删除定时任务失败: io"
    );
    assert_eq!(
        translator.text_args(
            "cron-notify-running-bg",
            HashMap::from([("id", "job-42".to_string())])
        ),
        "正在后台运行定时任务 'job-42'..."
    );
}

#[test]
fn gui_approval_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("approval-subtitle"),
        "Review and manage pending tool approval requests."
    );
    assert_eq!(
        translator.text("approval-filter-session-key"),
        "Session Key"
    );
    assert_eq!(translator.text("approval-filter-session-key-all"), "All");
    assert_eq!(translator.text("approval-filter-tool-name"), "Tool Name");
    assert_eq!(translator.text("approval-filter-tool-name-all"), "All");
    assert_eq!(translator.text("approval-filter-status"), "Status");
    assert_eq!(translator.text("approval-filter-status-all"), "All");
    assert_eq!(translator.text("approval-filter-preview"), "Preview");
    assert_eq!(translator.text("approval-filter-page"), "Page");
    assert_eq!(translator.text("approval-filter-size"), "Size");
    assert_eq!(translator.text("approval-col-id"), "ID");
    assert_eq!(translator.text("approval-col-session"), "Session");
    assert_eq!(translator.text("approval-col-tool"), "Tool");
    assert_eq!(translator.text("approval-col-risk"), "Risk");
    assert_eq!(translator.text("approval-col-status"), "Status");
    assert_eq!(translator.text("approval-col-requested-by"), "Requested By");
    assert_eq!(translator.text("approval-col-approved-by"), "Approved By");
    assert_eq!(translator.text("approval-col-expires-at"), "Expires At");
    assert_eq!(translator.text("approval-col-preview"), "Preview");
    assert_eq!(translator.text("approval-status-pending"), "pending");
    assert_eq!(translator.text("approval-status-approved"), "approved");
    assert_eq!(translator.text("approval-status-rejected"), "rejected");
    assert_eq!(translator.text("approval-status-expired"), "expired");
    assert_eq!(translator.text("approval-status-consumed"), "consumed");
    assert_eq!(translator.text("approval-no-rows"), "No approvals found.");
    assert_eq!(translator.text("approval-detail-id"), "ID:");
    assert_eq!(translator.text("approval-detail-session"), "Session:");
    assert_eq!(translator.text("approval-detail-tool"), "Tool:");
    assert_eq!(translator.text("approval-detail-risk-level"), "Risk Level:");
    assert_eq!(translator.text("approval-detail-status"), "Status:");
    assert_eq!(
        translator.text("approval-detail-requested-by"),
        "Requested By:"
    );
    assert_eq!(
        translator.text("approval-detail-approved-by"),
        "Approved By:"
    );
    assert_eq!(
        translator.text("approval-detail-justification"),
        "Justification:"
    );
    assert_eq!(translator.text("approval-detail-expires-at"), "Expires At:");
    assert_eq!(translator.text("approval-detail-created-at"), "Created At:");
    assert_eq!(translator.text("approval-detail-updated-at"), "Updated At:");
    assert_eq!(
        translator.text("approval-detail-consumed-at"),
        "Consumed At:"
    );
    assert_eq!(
        translator.text("approval-detail-command-preview"),
        "Command Preview:"
    );
    assert_eq!(
        translator.text("approval-detail-command-text"),
        "Command Text:"
    );
    assert_eq!(translator.text("approval-detail-na"), "-");
}

#[test]
fn gui_approval_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("approval-subtitle"),
        "查看和管理待审批的工具执行请求。"
    );
    assert_eq!(translator.text("approval-filter-session-key"), "会话密钥");
    assert_eq!(translator.text("approval-filter-session-key-all"), "全部");
    assert_eq!(translator.text("approval-filter-tool-name"), "工具名称");
    assert_eq!(translator.text("approval-filter-tool-name-all"), "全部");
    assert_eq!(translator.text("approval-filter-status"), "状态");
    assert_eq!(translator.text("approval-filter-status-all"), "全部");
    assert_eq!(translator.text("approval-filter-preview"), "预览");
    assert_eq!(translator.text("approval-filter-page"), "页码");
    assert_eq!(translator.text("approval-filter-size"), "每页数量");
    assert_eq!(translator.text("approval-col-id"), "ID");
    assert_eq!(translator.text("approval-col-session"), "会话");
    assert_eq!(translator.text("approval-col-tool"), "工具");
    assert_eq!(translator.text("approval-col-risk"), "风险");
    assert_eq!(translator.text("approval-col-status"), "状态");
    assert_eq!(translator.text("approval-col-requested-by"), "请求人");
    assert_eq!(translator.text("approval-col-approved-by"), "审批人");
    assert_eq!(translator.text("approval-col-expires-at"), "过期时间");
    assert_eq!(translator.text("approval-col-preview"), "预览");
    assert_eq!(translator.text("approval-status-pending"), "待审批");
    assert_eq!(translator.text("approval-status-approved"), "已批准");
    assert_eq!(translator.text("approval-status-rejected"), "已拒绝");
    assert_eq!(translator.text("approval-status-expired"), "已过期");
    assert_eq!(translator.text("approval-status-consumed"), "已消费");
    assert_eq!(translator.text("approval-no-rows"), "未找到审批记录。");
    assert_eq!(translator.text("approval-detail-id"), "ID:");
    assert_eq!(translator.text("approval-detail-session"), "会话:");
    assert_eq!(translator.text("approval-detail-tool"), "工具:");
    assert_eq!(translator.text("approval-detail-risk-level"), "风险等级:");
    assert_eq!(translator.text("approval-detail-status"), "状态:");
    assert_eq!(translator.text("approval-detail-requested-by"), "请求人:");
    assert_eq!(translator.text("approval-detail-approved-by"), "审批人:");
    assert_eq!(translator.text("approval-detail-justification"), "理由:");
    assert_eq!(translator.text("approval-detail-expires-at"), "过期时间:");
    assert_eq!(translator.text("approval-detail-created-at"), "创建时间:");
    assert_eq!(translator.text("approval-detail-updated-at"), "更新时间:");
    assert_eq!(translator.text("approval-detail-consumed-at"), "消费时间:");
    assert_eq!(
        translator.text("approval-detail-command-preview"),
        "命令预览:"
    );
    assert_eq!(translator.text("approval-detail-command-text"), "命令文本:");
    assert_eq!(translator.text("approval-detail-na"), "-");
}

#[test]
fn gui_approval_panel_translates_notifications_with_args_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "approval-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ Refresh"
    );
    assert_eq!(
        translator.text_args(
            "approval-label-count",
            HashMap::from([("count", "5".to_string())])
        ),
        "Approvals: 5"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-view",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 View"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-approve",
            HashMap::from([("icon", "✅".to_string())])
        ),
        "✅ Approve"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-reject",
            HashMap::from([("icon", "❌".to_string())])
        ),
        "❌ Reject"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-consume",
            HashMap::from([("icon", "🗑".to_string())])
        ),
        "🗑 Consume"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 Copy ID"
    );
    assert_eq!(
        translator.text_args(
            "approval-detail-title",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "Approval: abc-123"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-filters-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to load filters: timeout"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-list-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to load approvals: io"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-resolved",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "Approval abc-123 updated"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-resolve-failed",
            HashMap::from([("error", "reject".to_string())])
        ),
        "Failed to update approval: reject"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-consumed",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "Approval abc-123 consumed"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-consume-failed",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "Approval abc-123 was not consumed"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-consume-op-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to consume approval: io"
    );
}

#[test]
fn gui_approval_panel_translates_notifications_with_args_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "approval-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ 刷新"
    );
    assert_eq!(
        translator.text_args(
            "approval-label-count",
            HashMap::from([("count", "5".to_string())])
        ),
        "审批: 5"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-view",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 查看"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-approve",
            HashMap::from([("icon", "✅".to_string())])
        ),
        "✅ 批准"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-reject",
            HashMap::from([("icon", "❌".to_string())])
        ),
        "❌ 拒绝"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-consume",
            HashMap::from([("icon", "🗑".to_string())])
        ),
        "🗑 消费"
    );
    assert_eq!(
        translator.text_args(
            "approval-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 复制 ID"
    );
    assert_eq!(
        translator.text_args(
            "approval-detail-title",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "审批: abc-123"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-filters-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "加载筛选器失败: timeout"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-list-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "加载审批列表失败: io"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-resolved",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "审批 abc-123 已更新"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-resolve-failed",
            HashMap::from([("error", "reject".to_string())])
        ),
        "更新审批失败: reject"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-consumed",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "审批 abc-123 已消费"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-consume-failed",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "审批 abc-123 未被消费"
    );
    assert_eq!(
        translator.text_args(
            "approval-notify-consume-op-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "消费审批失败: io"
    );
}

#[test]
fn gui_heartbeat_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("hb-subtitle"),
        "Configure periodic heartbeat check-in jobs for active sessions."
    );
    assert_eq!(translator.text("hb-filter-start-date"), "Start Date");
    assert_eq!(translator.text("hb-filter-end-date"), "End Date");
    assert_eq!(translator.text("hb-filter-page"), "Page");
    assert_eq!(translator.text("hb-filter-size"), "Size");
    assert_eq!(translator.text("hb-col-id"), "ID");
    assert_eq!(translator.text("hb-col-session"), "Session");
    assert_eq!(translator.text("hb-col-channel"), "Channel");
    assert_eq!(translator.text("hb-col-enabled"), "Enabled");
    assert_eq!(translator.text("hb-col-every"), "Every");
    assert_eq!(translator.text("hb-col-recent-msgs"), "Recent Msgs");
    assert_eq!(translator.text("hb-col-next-run"), "Next Run At");
    assert_eq!(translator.text("hb-col-last-run"), "Last Run At");
    assert_eq!(translator.text("hb-col-updated-at"), "Updated At");
    assert_eq!(translator.text("hb-enabled-yes"), "yes");
    assert_eq!(translator.text("hb-enabled-no"), "no");
    assert_eq!(
        translator.text("hb-no-rows"),
        "No heartbeat jobs found in database."
    );
    assert_eq!(translator.text("hb-delete-title"), "Delete heartbeat job");
    assert_eq!(translator.text("hb-delete-btn"), "Delete");
    assert_eq!(translator.text("hb-delete-cancel"), "Cancel");
    assert_eq!(translator.text("hb-form-title-edit"), "Edit Heartbeat Job");
    assert_eq!(translator.text("hb-form-title-add"), "Add Heartbeat Job");
    assert_eq!(translator.text("hb-form-id"), "ID");
    assert_eq!(translator.text("hb-form-session-key"), "Session Key");
    assert_eq!(
        translator.text("hb-form-session-select"),
        "Select a session"
    );
    assert_eq!(translator.text("hb-form-channel"), "Channel");
    assert_eq!(translator.text("hb-form-chat-id"), "Chat ID");
    assert_eq!(translator.text("hb-form-enabled"), "Enabled");
    assert_eq!(
        translator.text("hb-form-enabled-hint"),
        "Enable or disable this heartbeat job."
    );
    assert_eq!(translator.text("hb-form-every"), "Every");
    assert_eq!(
        translator.text("hb-form-every-hint"),
        "Interval for heartbeat execution (e.g. 30m, 1h, 2h)."
    );
    assert_eq!(translator.text("hb-form-timezone"), "Timezone");
    assert_eq!(
        translator.text("hb-form-silent-ack-token"),
        "Silent Ack Token"
    );
    assert_eq!(
        translator.text("hb-form-silent-ack-token-hint"),
        "Token used to identify silent acknowledgments."
    );
    assert_eq!(
        translator.text("hb-form-recent-messages"),
        "Recent Messages"
    );
    assert_eq!(
        translator.text("hb-form-recent-messages-hint"),
        "Number of recent messages to include in heartbeat context."
    );
    assert_eq!(
        translator.text("hb-form-no-sessions"),
        "No indexed sessions found. Heartbeat must target an existing session."
    );
    assert_eq!(translator.text("hb-form-save"), "Save");
    assert_eq!(translator.text("hb-form-cancel"), "Cancel");
    assert_eq!(translator.text("hb-runs-refresh"), "Refresh Runs");
    assert_eq!(translator.text("hb-runs-run-now"), "Run Now");
    assert_eq!(
        translator.text("hb-runs-no-rows"),
        "No heartbeat runs found."
    );
    assert_eq!(translator.text("hb-runs-col-id"), "Run ID");
    assert_eq!(translator.text("hb-runs-col-status"), "Status");
    assert_eq!(translator.text("hb-runs-col-scheduled"), "Scheduled At");
    assert_eq!(translator.text("hb-runs-col-started"), "Started At");
    assert_eq!(translator.text("hb-runs-col-finished"), "Finished At");
    assert_eq!(translator.text("hb-runs-col-error"), "Error");
    assert_eq!(translator.text("hb-status-pending"), "pending");
    assert_eq!(translator.text("hb-status-running"), "running");
    assert_eq!(translator.text("hb-status-success"), "success");
    assert_eq!(translator.text("hb-status-failed"), "failed");
    assert_eq!(translator.text("hb-config-title"), "Heartbeat Config");
    assert_eq!(translator.text("hb-config-form-defaults"), "Form Defaults");
    assert_eq!(
        translator.text("hb-config-enabled-default"),
        "Enabled by default"
    );
    assert_eq!(
        translator.text("hb-config-enabled-default-hint"),
        "New heartbeat jobs will be enabled by default."
    );
    assert_eq!(
        translator.text("hb-config-recent-messages"),
        "Recent messages"
    );
    assert_eq!(
        translator.text("hb-config-info"),
        "Only the default enabled state and recent-message window are kept locally in the GUI.\\nOther heartbeat fields use built-in defaults."
    );
    assert_eq!(translator.text("hb-label-running"), "Running heartbeat...");
    assert_eq!(
        translator.text("hb-notify-form-unavailable"),
        "Heartbeat form is not available"
    );
    assert_eq!(
        translator.text("hb-notify-id-empty"),
        "Heartbeat ID cannot be empty"
    );
    assert_eq!(
        translator.text("hb-notify-session-empty"),
        "Session key cannot be empty"
    );
    assert_eq!(
        translator.text("hb-notify-channel-empty"),
        "Channel cannot be empty"
    );
    assert_eq!(
        translator.text("hb-notify-chat-id-empty"),
        "Chat ID cannot be empty"
    );
    assert_eq!(
        translator.text("hb-notify-every-empty"),
        "Every cannot be empty"
    );
    assert_eq!(
        translator.text("hb-notify-ack-token-empty"),
        "Silent Ack Token cannot be empty"
    );
    assert_eq!(
        translator.text("hb-notify-recent-msgs-zero"),
        "Recent Messages must be greater than zero"
    );
    assert_eq!(
        translator.text("hb-notify-timezone-empty"),
        "Timezone cannot be empty"
    );
    assert_eq!(
        translator.text("hb-notify-updated"),
        "Heartbeat job updated"
    );
    assert_eq!(
        translator.text("hb-notify-created"),
        "Heartbeat job created"
    );
    assert_eq!(translator.text("hb-notify-enabled"), "Heartbeat enabled");
    assert_eq!(translator.text("hb-notify-disabled"), "Heartbeat disabled");
    assert_eq!(
        translator.text("hb-notify-deleted"),
        "Heartbeat job deleted"
    );
    assert_eq!(
        translator.text("hb-notify-already-running"),
        "A heartbeat run is already in progress"
    );
}

#[test]
fn gui_heartbeat_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("hb-subtitle"),
        "配置活动会话的周期性心跳签到任务。"
    );
    assert_eq!(translator.text("hb-filter-start-date"), "开始日期");
    assert_eq!(translator.text("hb-filter-end-date"), "结束日期");
    assert_eq!(translator.text("hb-filter-page"), "页码");
    assert_eq!(translator.text("hb-filter-size"), "每页数量");
    assert_eq!(translator.text("hb-col-id"), "ID");
    assert_eq!(translator.text("hb-col-session"), "会话");
    assert_eq!(translator.text("hb-col-channel"), "通道");
    assert_eq!(translator.text("hb-col-enabled"), "启用");
    assert_eq!(translator.text("hb-col-every"), "间隔");
    assert_eq!(translator.text("hb-col-recent-msgs"), "近期消息");
    assert_eq!(translator.text("hb-col-next-run"), "下次运行时间");
    assert_eq!(translator.text("hb-col-last-run"), "上次运行时间");
    assert_eq!(translator.text("hb-col-updated-at"), "更新时间");
    assert_eq!(translator.text("hb-enabled-yes"), "是");
    assert_eq!(translator.text("hb-enabled-no"), "否");
    assert_eq!(translator.text("hb-no-rows"), "数据库中未找到心跳任务。");
    assert_eq!(translator.text("hb-delete-title"), "删除心跳任务");
    assert_eq!(translator.text("hb-delete-btn"), "删除");
    assert_eq!(translator.text("hb-delete-cancel"), "取消");
    assert_eq!(translator.text("hb-form-title-edit"), "编辑心跳任务");
    assert_eq!(translator.text("hb-form-title-add"), "添加心跳任务");
    assert_eq!(translator.text("hb-form-id"), "ID");
    assert_eq!(translator.text("hb-form-session-key"), "会话密钥");
    assert_eq!(translator.text("hb-form-session-select"), "选择一个会话");
    assert_eq!(translator.text("hb-form-channel"), "通道");
    assert_eq!(translator.text("hb-form-chat-id"), "聊天 ID");
    assert_eq!(translator.text("hb-form-enabled"), "启用");
    assert_eq!(
        translator.text("hb-form-enabled-hint"),
        "启用或禁用此心跳任务。"
    );
    assert_eq!(translator.text("hb-form-every"), "间隔");
    assert_eq!(
        translator.text("hb-form-every-hint"),
        "心跳执行的间隔时间（例如 30m、1h、2h）。"
    );
    assert_eq!(translator.text("hb-form-timezone"), "时区");
    assert_eq!(translator.text("hb-form-silent-ack-token"), "静默确认令牌");
    assert_eq!(
        translator.text("hb-form-silent-ack-token-hint"),
        "用于识别静默确认的令牌。"
    );
    assert_eq!(translator.text("hb-form-recent-messages"), "近期消息数");
    assert_eq!(
        translator.text("hb-form-recent-messages-hint"),
        "心跳上下文中包含的近期消息数量。"
    );
    assert_eq!(
        translator.text("hb-form-no-sessions"),
        "未找到已索引的会话。心跳任务必须指向一个现有会话。"
    );
    assert_eq!(translator.text("hb-form-save"), "保存");
    assert_eq!(translator.text("hb-form-cancel"), "取消");
    assert_eq!(translator.text("hb-runs-refresh"), "刷新运行记录");
    assert_eq!(translator.text("hb-runs-run-now"), "立即运行");
    assert_eq!(translator.text("hb-runs-no-rows"), "未找到心跳运行记录。");
    assert_eq!(translator.text("hb-runs-col-id"), "运行 ID");
    assert_eq!(translator.text("hb-runs-col-status"), "状态");
    assert_eq!(translator.text("hb-runs-col-scheduled"), "计划时间");
    assert_eq!(translator.text("hb-runs-col-started"), "开始时间");
    assert_eq!(translator.text("hb-runs-col-finished"), "完成时间");
    assert_eq!(translator.text("hb-runs-col-error"), "错误");
    assert_eq!(translator.text("hb-status-pending"), "待执行");
    assert_eq!(translator.text("hb-status-running"), "运行中");
    assert_eq!(translator.text("hb-status-success"), "成功");
    assert_eq!(translator.text("hb-status-failed"), "失败");
    assert_eq!(translator.text("hb-config-title"), "心跳配置");
    assert_eq!(translator.text("hb-config-form-defaults"), "表单默认值");
    assert_eq!(translator.text("hb-config-enabled-default"), "默认启用");
    assert_eq!(
        translator.text("hb-config-enabled-default-hint"),
        "新建的心跳任务将默认启用。"
    );
    assert_eq!(translator.text("hb-config-recent-messages"), "近期消息数");
    assert_eq!(
        translator.text("hb-config-info"),
        "仅默认启用状态和近期消息窗口保存在 GUI 本地。\\n其他心跳字段使用内置默认值。"
    );
    assert_eq!(translator.text("hb-label-running"), "正在运行心跳...");
    assert_eq!(
        translator.text("hb-notify-form-unavailable"),
        "心跳表单不可用"
    );
    assert_eq!(translator.text("hb-notify-id-empty"), "心跳 ID 不能为空");
    assert_eq!(
        translator.text("hb-notify-session-empty"),
        "会话密钥不能为空"
    );
    assert_eq!(translator.text("hb-notify-channel-empty"), "通道不能为空");
    assert_eq!(
        translator.text("hb-notify-chat-id-empty"),
        "聊天 ID 不能为空"
    );
    assert_eq!(translator.text("hb-notify-every-empty"), "间隔不能为空");
    assert_eq!(
        translator.text("hb-notify-ack-token-empty"),
        "静默确认令牌不能为空"
    );
    assert_eq!(
        translator.text("hb-notify-recent-msgs-zero"),
        "近期消息数必须大于零"
    );
    assert_eq!(translator.text("hb-notify-timezone-empty"), "时区不能为空");
    assert_eq!(translator.text("hb-notify-updated"), "心跳任务已更新");
    assert_eq!(translator.text("hb-notify-created"), "心跳任务已创建");
    assert_eq!(translator.text("hb-notify-enabled"), "心跳已启用");
    assert_eq!(translator.text("hb-notify-disabled"), "心跳已禁用");
    assert_eq!(translator.text("hb-notify-deleted"), "心跳任务已删除");
    assert_eq!(
        translator.text("hb-notify-already-running"),
        "心跳运行已在进行中"
    );
}

#[test]
fn gui_heartbeat_panel_translates_notifications_with_args_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args("hb-btn-refresh", HashMap::from([("icon", "⟳".to_string())])),
        "⟳ Refresh"
    );
    assert_eq!(
        translator.text_args("hb-btn-add", HashMap::from([("icon", "+".to_string())])),
        "+ Add Heartbeat Job"
    );
    assert_eq!(
        translator.text_args("hb-btn-config", HashMap::from([("icon", "⚙".to_string())])),
        "⚙ Config"
    );
    assert_eq!(
        translator.text_args("hb-label-jobs", HashMap::from([("count", "5".to_string())])),
        "Jobs: 5"
    );
    assert_eq!(
        translator.text_args("hb-ctx-runs", HashMap::from([("icon", "📋".to_string())])),
        "📋 Runs"
    );
    assert_eq!(
        translator.text_args("hb-ctx-run-now", HashMap::from([("icon", "▶".to_string())])),
        "▶ Run Now"
    );
    assert_eq!(
        translator.text_args("hb-ctx-edit", HashMap::from([("icon", "✏".to_string())])),
        "✏ Edit"
    );
    assert_eq!(
        translator.text_args(
            "hb-ctx-disable",
            HashMap::from([("icon", "⚡".to_string())])
        ),
        "⚡ Disable"
    );
    assert_eq!(
        translator.text_args("hb-ctx-enable", HashMap::from([("icon", "⚡".to_string())])),
        "⚡ Enable"
    );
    assert_eq!(
        translator.text_args("hb-ctx-delete", HashMap::from([("icon", "🗑".to_string())])),
        "🗑 Delete"
    );
    assert_eq!(
        translator.text_args(
            "hb-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 Copy ID"
    );
    assert_eq!(
        translator.text_args(
            "hb-delete-prompt",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "Delete heartbeat job 'abc-123'?"
    );
    assert_eq!(
        translator.text_args(
            "hb-runs-title",
            HashMap::from([("id", "job-1".to_string())])
        ),
        "Heartbeat Runs: job-1"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-sessions-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to list sessions: timeout"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-jobs-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to list heartbeat jobs: io"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-runs-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to load heartbeat runs: timeout"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-update-failed",
            HashMap::from([("error", "reject".to_string())])
        ),
        "Failed to update heartbeat job: reject"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-create-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to create heartbeat job: io"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-set-enabled-failed",
            HashMap::from([("error", "reject".to_string())])
        ),
        "Failed to set enabled: reject"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-delete-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to delete heartbeat job: io"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-running-bg",
            HashMap::from([("id", "job-42".to_string())])
        ),
        "Running heartbeat 'job-42' in background..."
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-executed",
            HashMap::from([("id", "msg-99".to_string())])
        ),
        "Heartbeat executed: msg-99"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-run-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to run heartbeat now: timeout"
    );
}

#[test]
fn gui_heartbeat_panel_translates_notifications_with_args_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args("hb-btn-refresh", HashMap::from([("icon", "⟳".to_string())])),
        "⟳ 刷新"
    );
    assert_eq!(
        translator.text_args("hb-btn-add", HashMap::from([("icon", "+".to_string())])),
        "+ 添加心跳任务"
    );
    assert_eq!(
        translator.text_args("hb-btn-config", HashMap::from([("icon", "⚙".to_string())])),
        "⚙ 配置"
    );
    assert_eq!(
        translator.text_args("hb-label-jobs", HashMap::from([("count", "5".to_string())])),
        "任务: 5"
    );
    assert_eq!(
        translator.text_args("hb-ctx-runs", HashMap::from([("icon", "📋".to_string())])),
        "📋 运行记录"
    );
    assert_eq!(
        translator.text_args("hb-ctx-run-now", HashMap::from([("icon", "▶".to_string())])),
        "▶ 立即运行"
    );
    assert_eq!(
        translator.text_args("hb-ctx-edit", HashMap::from([("icon", "✏".to_string())])),
        "✏ 编辑"
    );
    assert_eq!(
        translator.text_args(
            "hb-ctx-disable",
            HashMap::from([("icon", "⚡".to_string())])
        ),
        "⚡ 禁用"
    );
    assert_eq!(
        translator.text_args("hb-ctx-enable", HashMap::from([("icon", "⚡".to_string())])),
        "⚡ 启用"
    );
    assert_eq!(
        translator.text_args("hb-ctx-delete", HashMap::from([("icon", "🗑".to_string())])),
        "🗑 删除"
    );
    assert_eq!(
        translator.text_args(
            "hb-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 复制 ID"
    );
    assert_eq!(
        translator.text_args(
            "hb-delete-prompt",
            HashMap::from([("id", "abc-123".to_string())])
        ),
        "确定删除心跳任务 'abc-123' 吗？"
    );
    assert_eq!(
        translator.text_args(
            "hb-runs-title",
            HashMap::from([("id", "job-1".to_string())])
        ),
        "心跳运行记录: job-1"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-sessions-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "加载会话列表失败: timeout"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-jobs-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "加载心跳任务列表失败: io"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-runs-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "加载心跳运行记录失败: timeout"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-update-failed",
            HashMap::from([("error", "reject".to_string())])
        ),
        "更新心跳任务失败: reject"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-create-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "创建心跳任务失败: io"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-set-enabled-failed",
            HashMap::from([("error", "reject".to_string())])
        ),
        "设置启用状态失败: reject"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-delete-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "删除心跳任务失败: io"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-running-bg",
            HashMap::from([("id", "job-42".to_string())])
        ),
        "正在后台运行心跳 'job-42'..."
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-executed",
            HashMap::from([("id", "msg-99".to_string())])
        ),
        "心跳已执行: msg-99"
    );
    assert_eq!(
        translator.text_args(
            "hb-notify-run-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "立即运行心跳失败: timeout"
    );
}

#[test]
fn gui_session_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("sess-subtitle"),
        "Browse, filter, and inspect chat session history and usage metrics."
    );
    assert_eq!(translator.text("sess-filter-start-date"), "Start Date");
    assert_eq!(translator.text("sess-filter-end-date"), "End Date");
    assert_eq!(translator.text("sess-filter-channel"), "Channel");
    assert_eq!(translator.text("sess-filter-channel-all"), "All");
    assert_eq!(translator.text("sess-filter-page"), "Page");
    assert_eq!(translator.text("sess-filter-size"), "Size");
    assert_eq!(translator.text("sess-col-session-key"), "Session Key");
    assert_eq!(translator.text("sess-col-chat-id"), "Chat ID");
    assert_eq!(translator.text("sess-col-channel"), "Channel");
    assert_eq!(translator.text("sess-col-active-session"), "Active Session");
    assert_eq!(translator.text("sess-col-provider"), "Provider");
    assert_eq!(translator.text("sess-col-model"), "Model");
    assert_eq!(translator.text("sess-col-turns"), "Turns");
    assert_eq!(translator.text("sess-col-input"), "Input");
    assert_eq!(translator.text("sess-col-output"), "Output");
    assert_eq!(translator.text("sess-col-total"), "Total");
    assert_eq!(translator.text("sess-col-jsonl-path"), "JSONL Path");
    assert_eq!(translator.text("sess-sort-updated-asc"), "Updated At ↑");
    assert_eq!(translator.text("sess-sort-updated-desc"), "Updated At ↓");
    assert_eq!(translator.text("sess-sort-created-desc"), "Created At ↓");
    assert_eq!(translator.text("sess-no-rows"), "No sessions found.");
    assert_eq!(translator.text("sess-clean-title"), "Clean Sessions");
    assert_eq!(
        translator.text("sess-clean-desc"),
        "Delete cron/webhook sessions updated before the selected date."
    );
    assert_eq!(
        translator.text("sess-clean-updated-before"),
        "Updated At before"
    );
    assert_eq!(translator.text("sess-clean-session-types"), "Session types");
    assert_eq!(translator.text("sess-clean-type-cron"), "cron");
    assert_eq!(translator.text("sess-clean-type-webhook"), "webhook");
    assert_eq!(
        translator.text("sess-clean-hint"),
        "Select a date and at least one session type to continue."
    );
    assert_eq!(translator.text("sess-clean-btn"), "Clean");
    assert_eq!(translator.text("sess-clean-cancel"), "Cancel");
    assert_eq!(
        translator.text("sess-clean-progress-title"),
        "Cleaning Sessions"
    );
    assert_eq!(
        translator.text("sess-clean-progress-label"),
        "Cleaning expired cron/webhook sessions..."
    );
    assert_eq!(
        translator.text("sess-clean-progress-footer"),
        "This dialog will close automatically when cleanup finishes."
    );
    assert_eq!(
        translator.text("sess-clean-already-running"),
        "Session cleanup is already in progress."
    );
    assert_eq!(
        translator.text("sess-clean-validation-error"),
        "Select an Updated At date and at least one session type."
    );
    assert_eq!(
        translator.text("sess-notify-clean-disconnected"),
        "Failed to clean sessions: cleanup task stopped unexpectedly"
    );
}

#[test]
fn gui_session_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("sess-subtitle"),
        "浏览、筛选和查看聊天会话历史及用量指标。"
    );
    assert_eq!(translator.text("sess-filter-start-date"), "开始日期");
    assert_eq!(translator.text("sess-filter-end-date"), "结束日期");
    assert_eq!(translator.text("sess-filter-channel"), "通道");
    assert_eq!(translator.text("sess-filter-channel-all"), "全部");
    assert_eq!(translator.text("sess-filter-page"), "页码");
    assert_eq!(translator.text("sess-filter-size"), "每页数量");
    assert_eq!(translator.text("sess-col-session-key"), "会话密钥");
    assert_eq!(translator.text("sess-col-chat-id"), "聊天 ID");
    assert_eq!(translator.text("sess-col-channel"), "通道");
    assert_eq!(translator.text("sess-col-active-session"), "活动会话");
    assert_eq!(translator.text("sess-col-provider"), "提供商");
    assert_eq!(translator.text("sess-col-model"), "模型");
    assert_eq!(translator.text("sess-col-turns"), "对话轮次");
    assert_eq!(translator.text("sess-col-input"), "输入");
    assert_eq!(translator.text("sess-col-output"), "输出");
    assert_eq!(translator.text("sess-col-total"), "总计");
    assert_eq!(translator.text("sess-col-jsonl-path"), "JSONL 路径");
    assert_eq!(translator.text("sess-sort-updated-asc"), "更新时间 ↑");
    assert_eq!(translator.text("sess-sort-updated-desc"), "更新时间 ↓");
    assert_eq!(translator.text("sess-sort-created-desc"), "创建时间 ↓");
    assert_eq!(translator.text("sess-no-rows"), "未找到会话。");
    assert_eq!(translator.text("sess-clean-title"), "清理会话");
    assert_eq!(
        translator.text("sess-clean-desc"),
        "删除指定日期之前更新的 cron/webhook 会话。"
    );
    assert_eq!(translator.text("sess-clean-updated-before"), "更新时间早于");
    assert_eq!(translator.text("sess-clean-session-types"), "会话类型");
    assert_eq!(translator.text("sess-clean-type-cron"), "cron");
    assert_eq!(translator.text("sess-clean-type-webhook"), "webhook");
    assert_eq!(
        translator.text("sess-clean-hint"),
        "请选择日期和至少一种会话类型以继续。"
    );
    assert_eq!(translator.text("sess-clean-btn"), "清理");
    assert_eq!(translator.text("sess-clean-cancel"), "取消");
    assert_eq!(translator.text("sess-clean-progress-title"), "正在清理会话");
    assert_eq!(
        translator.text("sess-clean-progress-label"),
        "正在清理过期的 cron/webhook 会话..."
    );
    assert_eq!(
        translator.text("sess-clean-progress-footer"),
        "清理完成后此对话框将自动关闭。"
    );
    assert_eq!(
        translator.text("sess-clean-already-running"),
        "会话清理已在进行中。"
    );
    assert_eq!(
        translator.text("sess-clean-validation-error"),
        "请选择更新时间日期和至少一种会话类型。"
    );
    assert_eq!(
        translator.text("sess-notify-clean-disconnected"),
        "清理会话失败：清理任务意外停止"
    );
}

#[test]
fn gui_session_panel_translates_notifications_with_args_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "sess-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ Refresh"
    );
    assert_eq!(
        translator.text_args(
            "sess-btn-clean",
            HashMap::from([("icon", "🧹".to_string())])
        ),
        "🧹 Clean"
    );
    assert_eq!(
        translator.text_args(
            "sess-label-count",
            HashMap::from([("count", "5".to_string())])
        ),
        "Sessions: 5"
    );
    assert_eq!(
        translator.text_args(
            "sess-ctx-view-chat",
            HashMap::from([("icon", "💬".to_string())])
        ),
        "💬 View Chat"
    );
    assert_eq!(
        translator.text_args(
            "sess-ctx-copy-key",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 Copy Session Key"
    );
    assert_eq!(
        translator.text_args(
            "sess-chat-title",
            HashMap::from([("key", "sess-1".to_string())])
        ),
        "Chat: sess-1"
    );
    assert_eq!(
        translator.text_args(
            "sess-clean-progress-total",
            HashMap::from([("count", "10".to_string())])
        ),
        "Total: 10"
    );
    assert_eq!(
        translator.text_args(
            "sess-clean-progress-deleted",
            HashMap::from([("count", "3".to_string())])
        ),
        "Deleted: 3"
    );
    assert_eq!(
        translator.text_args(
            "sess-clean-progress-bar",
            HashMap::from([("deleted", "3".to_string()), ("total", "10".to_string())])
        ),
        "3 / 10"
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-list-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Failed to load sessions: timeout"
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-chat-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to load chat records: io"
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-clean-success",
            HashMap::from([
                ("sessions", "5".to_string()),
                ("files", "3".to_string()),
                ("missing", "2".to_string())
            ])
        ),
        "Cleaned 5 sessions and deleted 3 JSONL files (2 already missing)."
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-clean-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to clean sessions: io"
    );
}

#[test]
fn gui_session_panel_translates_notifications_with_args_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "sess-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ 刷新"
    );
    assert_eq!(
        translator.text_args(
            "sess-btn-clean",
            HashMap::from([("icon", "🧹".to_string())])
        ),
        "🧹 清理"
    );
    assert_eq!(
        translator.text_args(
            "sess-label-count",
            HashMap::from([("count", "5".to_string())])
        ),
        "会话: 5"
    );
    assert_eq!(
        translator.text_args(
            "sess-ctx-view-chat",
            HashMap::from([("icon", "💬".to_string())])
        ),
        "💬 查看聊天"
    );
    assert_eq!(
        translator.text_args(
            "sess-ctx-copy-key",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 复制会话密钥"
    );
    assert_eq!(
        translator.text_args(
            "sess-chat-title",
            HashMap::from([("key", "sess-1".to_string())])
        ),
        "聊天: sess-1"
    );
    assert_eq!(
        translator.text_args(
            "sess-clean-progress-total",
            HashMap::from([("count", "10".to_string())])
        ),
        "总计: 10"
    );
    assert_eq!(
        translator.text_args(
            "sess-clean-progress-deleted",
            HashMap::from([("count", "3".to_string())])
        ),
        "已删除: 3"
    );
    assert_eq!(
        translator.text_args(
            "sess-clean-progress-bar",
            HashMap::from([("deleted", "3".to_string()), ("total", "10".to_string())])
        ),
        "3 / 10"
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-list-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "加载会话列表失败: timeout"
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-chat-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "加载聊天记录失败: io"
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-clean-success",
            HashMap::from([
                ("sessions", "5".to_string()),
                ("files", "3".to_string()),
                ("missing", "2".to_string())
            ])
        ),
        "已清理 5 个会话，删除 3 个 JSONL 文件（2 个已不存在）。"
    );
    assert_eq!(
        translator.text_args(
            "sess-notify-clean-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "清理会话失败: io"
    );
}

#[test]
fn gui_archive_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("archive-subtitle"),
        "Browse, filter, and preview archived files and attachments stored in the workspace."
    );
    assert_eq!(translator.text("archive-filter-session-key"), "Session Key");
    assert_eq!(translator.text("archive-filter-chat-id"), "Chat ID");
    assert_eq!(translator.text("archive-filter-source-kind"), "Source Kind");
    assert_eq!(translator.text("archive-filter-media-kind"), "Media Kind");
    assert_eq!(translator.text("archive-filter-filename"), "Filename");
    assert_eq!(translator.text("archive-filter-page"), "Page");
    assert_eq!(translator.text("archive-filter-size"), "Size");
    assert_eq!(translator.text("archive-filter-all"), "All");
    assert_eq!(translator.text("archive-col-id"), "ID");
    assert_eq!(translator.text("archive-col-source"), "Source");
    assert_eq!(translator.text("archive-col-media"), "Media");
    assert_eq!(translator.text("archive-col-filename"), "Filename");
    assert_eq!(translator.text("archive-col-mime"), "MIME");
    assert_eq!(translator.text("archive-col-size"), "Size");
    assert_eq!(translator.text("archive-col-created-at"), "Created At");
    assert_eq!(
        translator.text("archive-no-records"),
        "No archive records found."
    );
    assert_eq!(translator.text("archive-detail-title"), "Archive Details");
    assert_eq!(translator.text("archive-detail-close"), "Close");
}

#[test]
fn gui_archive_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("archive-subtitle"),
        "浏览、筛选和预览工作区中存储的归档文件和附件。"
    );
    assert_eq!(translator.text("archive-filter-session-key"), "会话标识");
    assert_eq!(translator.text("archive-filter-chat-id"), "聊天 ID");
    assert_eq!(translator.text("archive-filter-source-kind"), "来源类型");
    assert_eq!(translator.text("archive-filter-media-kind"), "媒体类型");
    assert_eq!(translator.text("archive-filter-filename"), "文件名");
    assert_eq!(translator.text("archive-filter-page"), "页码");
    assert_eq!(translator.text("archive-filter-size"), "每页数量");
    assert_eq!(translator.text("archive-filter-all"), "全部");
    assert_eq!(translator.text("archive-col-id"), "ID");
    assert_eq!(translator.text("archive-col-source"), "来源");
    assert_eq!(translator.text("archive-col-media"), "媒体");
    assert_eq!(translator.text("archive-col-filename"), "文件名");
    assert_eq!(translator.text("archive-col-mime"), "MIME");
    assert_eq!(translator.text("archive-col-size"), "大小");
    assert_eq!(translator.text("archive-col-created-at"), "创建时间");
    assert_eq!(translator.text("archive-no-records"), "未找到归档记录。");
    assert_eq!(translator.text("archive-detail-title"), "归档详情");
    assert_eq!(translator.text("archive-detail-close"), "关闭");
}

#[test]
fn gui_archive_panel_translates_parameterized_keys_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "archive-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ Refresh"
    );
    assert_eq!(
        translator.text_args(
            "archive-label-total",
            HashMap::from([("count", "42".to_string())])
        ),
        "Total: 42"
    );
    assert_eq!(
        translator.text_args(
            "archive-ctx-preview",
            HashMap::from([("icon", "👁".to_string())])
        ),
        "👁 Preview"
    );
    assert_eq!(
        translator.text_args(
            "archive-ctx-details",
            HashMap::from([("icon", "📄".to_string())])
        ),
        "📄 Details"
    );
    assert_eq!(
        translator.text_args(
            "archive-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 Copy ID"
    );
    assert_eq!(
        translator.text_args(
            "archive-detail-id",
            HashMap::from([("value", "abc".to_string())])
        ),
        "ID: abc"
    );
    assert_eq!(
        translator.text_args(
            "archive-detail-source-kind",
            HashMap::from([("value", "user_upload".to_string())])
        ),
        "Source Kind: user_upload"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-title",
            HashMap::from([("title", "test.png".to_string())])
        ),
        "Preview: test.png"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-id",
            HashMap::from([("value", "abc".to_string())])
        ),
        "ID: abc"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-mime",
            HashMap::from([("mime", "image/png".to_string())])
        ),
        "MIME: image/png"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-path",
            HashMap::from([("value", "/data/test.png".to_string())])
        ),
        "Path: /data/test.png"
    );
    assert_eq!(
        translator.text_args(
            "archive-notify-load-filters-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to load filters: io"
    );
    assert_eq!(
        translator.text_args(
            "archive-notify-query-failed",
            HashMap::from([("error", "db".to_string())])
        ),
        "Failed to query archives: db"
    );
}

#[test]
fn gui_archive_panel_translates_parameterized_keys_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "archive-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ 刷新"
    );
    assert_eq!(
        translator.text_args(
            "archive-label-total",
            HashMap::from([("count", "42".to_string())])
        ),
        "总数: 42"
    );
    assert_eq!(
        translator.text_args(
            "archive-ctx-preview",
            HashMap::from([("icon", "👁".to_string())])
        ),
        "👁 预览"
    );
    assert_eq!(
        translator.text_args(
            "archive-ctx-details",
            HashMap::from([("icon", "📄".to_string())])
        ),
        "📄 详情"
    );
    assert_eq!(
        translator.text_args(
            "archive-ctx-copy-id",
            HashMap::from([("icon", "📋".to_string())])
        ),
        "📋 复制 ID"
    );
    assert_eq!(
        translator.text_args(
            "archive-detail-id",
            HashMap::from([("value", "abc".to_string())])
        ),
        "ID: abc"
    );
    assert_eq!(
        translator.text_args(
            "archive-detail-source-kind",
            HashMap::from([("value", "user_upload".to_string())])
        ),
        "来源类型: user_upload"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-title",
            HashMap::from([("title", "test.png".to_string())])
        ),
        "预览: test.png"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-id",
            HashMap::from([("value", "abc".to_string())])
        ),
        "ID: abc"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-mime",
            HashMap::from([("mime", "image/png".to_string())])
        ),
        "MIME: image/png"
    );
    assert_eq!(
        translator.text_args(
            "archive-preview-path",
            HashMap::from([("value", "/data/test.png".to_string())])
        ),
        "路径: /data/test.png"
    );
    assert_eq!(
        translator.text_args(
            "archive-notify-load-filters-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "加载筛选条件失败: io"
    );
    assert_eq!(
        translator.text_args(
            "archive-notify-query-failed",
            HashMap::from([("error", "db".to_string())])
        ),
        "查询归档记录失败: db"
    );
    assert_eq!(
        translator.text_args(
            "archive-notify-open-preview-failed",
            HashMap::from([("error", "db".to_string())])
        ),
        "打开归档预览失败: db"
    );
}

#[test]
fn gui_knowledge_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("kn-subtitle"),
        "Search, index, and retrieve knowledge from your connected vaults and document sources."
    );
    assert_eq!(translator.text("kn-form-title"), "Knowledge Config");
    assert_eq!(translator.text("kn-form-enabled"), "Enabled");
    assert_eq!(
        translator.text("kn-form-enabled-hint"),
        "Enable or disable the knowledge retrieval subsystem."
    );
    assert_eq!(translator.text("kn-form-provider"), "Provider");
    assert_eq!(
        translator.text("kn-form-provider-hint"),
        "The knowledge source provider (e.g. Obsidian vault)."
    );
    assert_eq!(translator.text("kn-form-vault-path"), "Vault path");
    assert_eq!(
        translator.text("kn-form-vault-path-hint"),
        "Absolute path to the local vault directory on your device."
    );
    assert_eq!(
        translator.text("kn-form-auto-index"),
        "Auto-index vault changes"
    );
    assert_eq!(
        translator.text("kn-form-auto-index-hint"),
        "Automatically re-index when vault files change. Run Sync once for the initial index."
    );
    assert_eq!(translator.text("kn-form-save"), "Save");
    assert_eq!(translator.text("kn-form-cancel"), "Cancel");
    assert_eq!(translator.text("kn-form-refresh-models"), "Refresh models");
    assert_eq!(
        translator.text("kn-form-models-loading"),
        "Loading installed models..."
    );
    assert_eq!(
        translator.text("kn-form-models-empty"),
        "No installed local models were found."
    );
    assert_eq!(translator.text("kn-model-not-configured"), "Not configured");
    assert_eq!(translator.text("kn-status-runtime"), "Runtime");
    assert_eq!(translator.text("kn-status-state"), "State");
    assert_eq!(translator.text("kn-status-provider"), "Provider");
    assert_eq!(translator.text("kn-status-entries"), "Entries");
    assert_eq!(translator.text("kn-status-chunks"), "Chunks");
    assert_eq!(translator.text("kn-status-vectors"), "Vectors");
    assert_eq!(translator.text("kn-state-disabled"), "disabled");
    assert_eq!(translator.text("kn-state-unconfigured"), "unconfigured");
    assert_eq!(translator.text("kn-state-loading"), "loading");
    assert_eq!(translator.text("kn-state-ready"), "ready");
    assert_eq!(translator.text("kn-state-syncing"), "syncing");
    assert_eq!(translator.text("kn-state-error"), "error");
    assert_eq!(translator.text("kn-state-enabled"), "enabled");
    assert_eq!(translator.text("kn-state-disabled-label"), "disabled");
    assert_eq!(translator.text("kn-state-unknown"), "unknown");
    assert_eq!(translator.text("kn-capability-embedding"), "embedding");
    assert_eq!(translator.text("kn-capability-rerank"), "rerank");
    assert_eq!(translator.text("kn-capability-chat"), "chat");
    assert_eq!(
        translator.text("kn-capability-orchestrator"),
        "orchestrator"
    );
    assert_eq!(translator.text("kn-search-query"), "Query");
    assert_eq!(translator.text("kn-search-hint"), "Search notes");
    assert_eq!(translator.text("kn-search-limit"), "Limit");
    assert_eq!(translator.text("kn-btn-search"), "Search");
    assert_eq!(
        translator.text("kn-search-not-ready"),
        "Knowledge runtime is not ready yet."
    );
    assert_eq!(translator.text("kn-col-title"), "Title");
    assert_eq!(translator.text("kn-col-score"), "Score");
    assert_eq!(translator.text("kn-preview-heading"), "Preview");
    assert_eq!(
        translator.text("kn-preview-empty"),
        "Select a result to inspect it."
    );
    assert_eq!(translator.text("kn-sync-stage-indexing"), "Indexing notes");
    assert_eq!(
        translator.text("kn-sync-stage-embedding"),
        "Embedding chunks"
    );
    assert_eq!(translator.text("kn-sync-title"), "Syncing Knowledge Index");
    assert_eq!(
        translator.text("kn-sync-preparing"),
        "Preparing knowledge sync..."
    );
    assert_eq!(
        translator.text("kn-notify-store-unavailable"),
        "Configuration store is not available"
    );
    assert_eq!(
        translator.text("kn-notify-config-saved"),
        "Knowledge config saved"
    );
    assert_eq!(
        translator.text("kn-notify-syncing"),
        "Syncing knowledge index and vectors..."
    );
    assert_eq!(
        translator.text("kn-notify-search-query-required"),
        "Knowledge search requires a query"
    );
    assert_eq!(
        translator.text("kn-notify-models-disconnected"),
        "Model list worker closed unexpectedly"
    );
    assert_eq!(
        translator.text("kn-validation-provider-obsidian"),
        "knowledge.provider must be obsidian"
    );
    assert_eq!(
        translator.text("kn-validation-vault-required"),
        "knowledge.obsidian.vault_path is required when enabled"
    );
    assert_eq!(
        translator.text("kn-validation-graph-hops"),
        "graph_hops must be a non-negative integer"
    );
    assert_eq!(
        translator.text("kn-validation-temporal-decay"),
        "temporal_decay must be a number"
    );
}

#[test]
fn gui_knowledge_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("kn-subtitle"),
        "搜索、索引和检索来自已连接 vault 及文档源的知识内容。"
    );
    assert_eq!(translator.text("kn-form-title"), "知识配置");
    assert_eq!(translator.text("kn-form-enabled"), "启用");
    assert_eq!(
        translator.text("kn-form-enabled-hint"),
        "启用或禁用知识检索子系统。"
    );
    assert_eq!(translator.text("kn-form-provider"), "提供者");
    assert_eq!(
        translator.text("kn-form-provider-hint"),
        "知识源提供者（例如 Obsidian vault）。"
    );
    assert_eq!(translator.text("kn-form-vault-path"), "Vault 路径");
    assert_eq!(
        translator.text("kn-form-vault-path-hint"),
        "本地 vault 目录在设备上的绝对路径。"
    );
    assert_eq!(translator.text("kn-form-auto-index"), "自动索引 vault 变更");
    assert_eq!(
        translator.text("kn-form-auto-index-hint"),
        "vault 文件变更时自动重新索引。首次使用请先执行一次同步。"
    );
    assert_eq!(translator.text("kn-form-save"), "保存");
    assert_eq!(translator.text("kn-form-cancel"), "取消");
    assert_eq!(translator.text("kn-form-refresh-models"), "刷新模型列表");
    assert_eq!(
        translator.text("kn-form-models-loading"),
        "正在加载已安装模型..."
    );
    assert_eq!(
        translator.text("kn-form-models-empty"),
        "未找到已安装的本地模型。"
    );
    assert_eq!(translator.text("kn-model-not-configured"), "未配置");
    assert_eq!(translator.text("kn-status-runtime"), "运行时");
    assert_eq!(translator.text("kn-status-state"), "状态");
    assert_eq!(translator.text("kn-status-provider"), "提供者");
    assert_eq!(translator.text("kn-status-entries"), "条目数");
    assert_eq!(translator.text("kn-status-chunks"), "块数");
    assert_eq!(translator.text("kn-status-vectors"), "向量数");
    assert_eq!(translator.text("kn-state-disabled"), "已禁用");
    assert_eq!(translator.text("kn-state-unconfigured"), "未配置");
    assert_eq!(translator.text("kn-state-loading"), "加载中");
    assert_eq!(translator.text("kn-state-ready"), "就绪");
    assert_eq!(translator.text("kn-state-syncing"), "同步中");
    assert_eq!(translator.text("kn-state-error"), "错误");
    assert_eq!(translator.text("kn-state-enabled"), "已启用");
    assert_eq!(translator.text("kn-state-disabled-label"), "已禁用");
    assert_eq!(translator.text("kn-state-unknown"), "未知");
    assert_eq!(translator.text("kn-capability-embedding"), "嵌入");
    assert_eq!(translator.text("kn-capability-rerank"), "重排");
    assert_eq!(translator.text("kn-capability-chat"), "聊天");
    assert_eq!(translator.text("kn-capability-orchestrator"), "编排");
    assert_eq!(translator.text("kn-search-query"), "查询");
    assert_eq!(translator.text("kn-search-hint"), "搜索笔记");
    assert_eq!(translator.text("kn-search-limit"), "限制");
    assert_eq!(translator.text("kn-btn-search"), "搜索");
    assert_eq!(
        translator.text("kn-search-not-ready"),
        "知识运行时尚未就绪。"
    );
    assert_eq!(translator.text("kn-col-title"), "标题");
    assert_eq!(translator.text("kn-col-score"), "分数");
    assert_eq!(translator.text("kn-preview-heading"), "预览");
    assert_eq!(
        translator.text("kn-preview-empty"),
        "选择一个结果以查看详情。"
    );
    assert_eq!(translator.text("kn-sync-stage-indexing"), "正在索引笔记");
    assert_eq!(translator.text("kn-sync-stage-embedding"), "正在嵌入块");
    assert_eq!(translator.text("kn-sync-title"), "同步知识索引");
    assert_eq!(translator.text("kn-sync-preparing"), "正在准备知识同步...");
    assert_eq!(
        translator.text("kn-notify-store-unavailable"),
        "配置存储不可用"
    );
    assert_eq!(translator.text("kn-notify-config-saved"), "知识配置已保存");
    assert_eq!(
        translator.text("kn-notify-syncing"),
        "正在同步知识索引与向量..."
    );
    assert_eq!(
        translator.text("kn-notify-search-query-required"),
        "知识搜索需要输入查询"
    );
    assert_eq!(
        translator.text("kn-notify-models-disconnected"),
        "模型列表加载器意外关闭"
    );
    assert_eq!(
        translator.text("kn-validation-provider-obsidian"),
        "knowledge.provider 必须为 obsidian"
    );
    assert_eq!(
        translator.text("kn-validation-vault-required"),
        "启用时 knowledge.obsidian.vault_path 为必填"
    );
    assert_eq!(
        translator.text("kn-validation-graph-hops"),
        "graph_hops 必须为非负整数"
    );
    assert_eq!(
        translator.text("kn-validation-temporal-decay"),
        "temporal_decay 必须为数字"
    );
}

#[test]
fn gui_knowledge_panel_translates_notifications_with_args_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args("kn-btn-refresh", HashMap::from([("icon", "⟳".to_string())])),
        "⟳ Refresh"
    );
    assert_eq!(
        translator.text_args("kn-btn-sync", HashMap::from([("icon", "⟲".to_string())])),
        "⟲ Sync Index & Vectors"
    );
    assert_eq!(
        translator.text_args("kn-btn-config", HashMap::from([("icon", "⚙".to_string())])),
        "⚙ Config"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-save-failed",
            HashMap::from([("error", "disk error".to_string())])
        ),
        "Save failed: disk error"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-sync-complete",
            HashMap::from([("notes", "42".to_string()), ("chunks", "128".to_string()),])
        ),
        "Knowledge sync complete: 42 notes indexed, 128 chunks embedded"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-sync-failed",
            HashMap::from([("error", "io error".to_string())])
        ),
        "Knowledge sync failed: io error"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-status-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Knowledge status failed: timeout"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-search-failed",
            HashMap::from([("error", "network".to_string())])
        ),
        "Knowledge search failed: network"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-entry-failed",
            HashMap::from([("error", "missing".to_string())])
        ),
        "Knowledge entry failed: missing"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-models-failed",
            HashMap::from([("error", "disk".to_string())])
        ),
        "Model list failed: disk"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-not-installed",
            HashMap::from([("name", "my-model".to_string())])
        ),
        "my-model (not installed)"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-with-capability",
            HashMap::from([
                ("name", "embed-v1".to_string()),
                ("capability", "embedding".to_string()),
            ])
        ),
        "embed-v1 (embedding)"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-capability-unknown",
            HashMap::from([("name", "unknown-m".to_string())])
        ),
        "unknown-m (capability unknown)"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-with-capabilities",
            HashMap::from([
                ("name", "multi-m".to_string()),
                ("capabilities", "embedding, rerank".to_string()),
            ])
        ),
        "multi-m (embedding, rerank)"
    );
    assert_eq!(
        translator.text_args(
            "kn-provider-unsupported",
            HashMap::from([("name", "notion".to_string())])
        ),
        "notion (unsupported)"
    );
    assert_eq!(
        translator.text_args(
            "kn-vault-label",
            HashMap::from([("path", "/tmp/vault".to_string())])
        ),
        "Vault: /tmp/vault"
    );
    assert_eq!(
        translator.text_args(
            "kn-path-label",
            HashMap::from([("path", "/home/config".to_string())])
        ),
        "Path: /home/config"
    );
    assert_eq!(
        translator.text_args(
            "kn-results-heading",
            HashMap::from([("count", "5".to_string())])
        ),
        "Results (5)"
    );
    assert_eq!(
        translator.text_args(
            "kn-preview-not-loaded",
            HashMap::from([("id", "note-1".to_string())])
        ),
        "No entry loaded for note-1."
    );
    assert_eq!(
        translator.text_args(
            "kn-preview-tags",
            HashMap::from([("tags", "rust, cli".to_string())])
        ),
        "tags: rust, cli"
    );
    assert_eq!(
        translator.text_args(
            "kn-preview-uri",
            HashMap::from([("uri", "file:///vault/note.md".to_string())])
        ),
        "URI: file:///vault/note.md"
    );
    assert_eq!(
        translator.text_args(
            "kn-sync-current",
            HashMap::from([("item", "readme.md".to_string())])
        ),
        "Current: readme.md"
    );
    assert_eq!(
        translator.text_args(
            "kn-sync-progress",
            HashMap::from([("completed", "10".to_string()), ("total", "50".to_string()),])
        ),
        "10 / 50"
    );
    assert_eq!(
        translator.text_args(
            "kn-sync-processed",
            HashMap::from([("count", "12".to_string())])
        ),
        "12 processed"
    );
    assert_eq!(
        translator.text_args(
            "kn-validation-positive-integer",
            HashMap::from([("field", "top_k".to_string())])
        ),
        "top_k must be a positive integer"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-config-load-failed",
            HashMap::from([("error", "disk".to_string())])
        ),
        "Failed to load config: disk"
    );
}

#[test]
fn gui_knowledge_panel_translates_notifications_with_args_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args("kn-btn-refresh", HashMap::from([("icon", "⟳".to_string())])),
        "⟳ 刷新"
    );
    assert_eq!(
        translator.text_args("kn-btn-sync", HashMap::from([("icon", "⟲".to_string())])),
        "⟲ 同步索引与向量"
    );
    assert_eq!(
        translator.text_args("kn-btn-config", HashMap::from([("icon", "⚙".to_string())])),
        "⚙ 配置"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-save-failed",
            HashMap::from([("error", "磁盘错误".to_string())])
        ),
        "保存失败: 磁盘错误"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-sync-complete",
            HashMap::from([("notes", "42".to_string()), ("chunks", "128".to_string()),])
        ),
        "知识同步完成: 已索引 42 条笔记, 已嵌入 128 个块"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-sync-failed",
            HashMap::from([("error", "io 错误".to_string())])
        ),
        "知识同步失败: io 错误"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-status-failed",
            HashMap::from([("error", "超时".to_string())])
        ),
        "知识状态加载失败: 超时"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-search-failed",
            HashMap::from([("error", "网络".to_string())])
        ),
        "知识搜索失败: 网络"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-entry-failed",
            HashMap::from([("error", "缺失".to_string())])
        ),
        "知识条目加载失败: 缺失"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-models-failed",
            HashMap::from([("error", "磁盘".to_string())])
        ),
        "模型列表加载失败: 磁盘"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-not-installed",
            HashMap::from([("name", "my-model".to_string())])
        ),
        "my-model（未安装）"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-with-capability",
            HashMap::from([
                ("name", "embed-v1".to_string()),
                ("capability", "嵌入".to_string()),
            ])
        ),
        "embed-v1（嵌入）"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-capability-unknown",
            HashMap::from([("name", "unknown-m".to_string())])
        ),
        "unknown-m（能力未知）"
    );
    assert_eq!(
        translator.text_args(
            "kn-model-with-capabilities",
            HashMap::from([
                ("name", "multi-m".to_string()),
                ("capabilities", "嵌入, 重排".to_string()),
            ])
        ),
        "multi-m（嵌入, 重排）"
    );
    assert_eq!(
        translator.text_args(
            "kn-provider-unsupported",
            HashMap::from([("name", "notion".to_string())])
        ),
        "notion（不支持）"
    );
    assert_eq!(
        translator.text_args(
            "kn-vault-label",
            HashMap::from([("path", "/tmp/vault".to_string())])
        ),
        "Vault: /tmp/vault"
    );
    assert_eq!(
        translator.text_args(
            "kn-path-label",
            HashMap::from([("path", "/home/config".to_string())])
        ),
        "路径: /home/config"
    );
    assert_eq!(
        translator.text_args(
            "kn-results-heading",
            HashMap::from([("count", "5".to_string())])
        ),
        "结果（5）"
    );
    assert_eq!(
        translator.text_args(
            "kn-preview-not-loaded",
            HashMap::from([("id", "note-1".to_string())])
        ),
        "无法加载条目 note-1。"
    );
    assert_eq!(
        translator.text_args(
            "kn-preview-tags",
            HashMap::from([("tags", "rust, cli".to_string())])
        ),
        "标签: rust, cli"
    );
    assert_eq!(
        translator.text_args(
            "kn-preview-uri",
            HashMap::from([("uri", "file:///vault/note.md".to_string())])
        ),
        "URI: file:///vault/note.md"
    );
    assert_eq!(
        translator.text_args(
            "kn-sync-current",
            HashMap::from([("item", "readme.md".to_string())])
        ),
        "当前: readme.md"
    );
    assert_eq!(
        translator.text_args(
            "kn-sync-progress",
            HashMap::from([("completed", "10".to_string()), ("total", "50".to_string()),])
        ),
        "10 / 50"
    );
    assert_eq!(
        translator.text_args(
            "kn-sync-processed",
            HashMap::from([("count", "12".to_string())])
        ),
        "已处理 12"
    );
    assert_eq!(
        translator.text_args(
            "kn-validation-positive-integer",
            HashMap::from([("field", "top_k".to_string())])
        ),
        "top_k 必须为正整数"
    );
    assert_eq!(
        translator.text_args(
            "kn-notify-config-load-failed",
            HashMap::from([("error", "磁盘".to_string())])
        ),
        "加载配置失败: 磁盘"
    );
}

#[test]
fn gui_memory_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("mem-subtitle"),
        "Store, search, and manage persistent memory records across sessions."
    );
    assert_eq!(translator.text("mem-btn-archive"), "Archive Now");
    assert_eq!(translator.text("mem-status-archiving"), "Archiving...");
    assert_eq!(translator.text("mem-status-deleting"), "Deleting...");
    assert_eq!(translator.text("mem-status-loading"), "Loading...");
    assert_eq!(
        translator.text("mem-status-no-data"),
        "No memory data available yet."
    );
    assert_eq!(translator.text("mem-tab-long-term"), "Long-term");
    assert_eq!(translator.text("mem-tab-session-search"), "Session Search");
    assert_eq!(translator.text("mem-tab-diagnostics"), "Diagnostics");
    assert_eq!(translator.text("mem-filter-status-active"), "Active");
    assert_eq!(
        translator.text("mem-filter-status-superseded"),
        "Superseded"
    );
    assert_eq!(translator.text("mem-filter-status-archived"), "Archived");
    assert_eq!(translator.text("mem-filter-status-rejected"), "Rejected");
    assert_eq!(translator.text("mem-filter-status-all"), "All");
    assert_eq!(translator.text("mem-filter-kind-all"), "All kinds");
    assert_eq!(translator.text("mem-filter-kind-identity"), "identity");
    assert_eq!(translator.text("mem-filter-kind-preference"), "preference");
    assert_eq!(
        translator.text("mem-filter-kind-project-rule"),
        "project_rule"
    );
    assert_eq!(translator.text("mem-filter-kind-workflow"), "workflow");
    assert_eq!(translator.text("mem-filter-kind-fact"), "fact");
    assert_eq!(translator.text("mem-filter-kind-constraint"), "constraint");
    assert_eq!(translator.text("mem-topic-label"), "Topic");
    assert_eq!(translator.text("mem-col-id"), "ID");
    assert_eq!(translator.text("mem-col-kind"), "Kind");
    assert_eq!(translator.text("mem-col-status"), "Status");
    assert_eq!(translator.text("mem-col-priority"), "Priority");
    assert_eq!(translator.text("mem-col-topic"), "Topic");
    assert_eq!(translator.text("mem-col-pin"), "Pin");
    assert_eq!(translator.text("mem-col-summary"), "Summary");
    assert_eq!(translator.text("mem-col-content"), "Content");
    assert_eq!(translator.text("mem-col-updated"), "Updated");
    assert_eq!(translator.text("mem-pin-yes"), "yes");
    assert_eq!(translator.text("mem-pin-no"), "no");
    assert_eq!(translator.text("mem-summary-type"), "summary");
    assert_eq!(translator.text("mem-summary-source"), "source");
    assert_eq!(translator.text("mem-summary-none"), "-");
    assert_eq!(translator.text("mem-priority-none"), "-");
    assert_eq!(
        translator.text("mem-config-title"),
        "Long-term Memory Embedding Config"
    );
    assert_eq!(translator.text("mem-config-enabled"), "Embedding enabled");
    assert_eq!(translator.text("mem-config-provider"), "Provider");
    assert_eq!(translator.text("mem-config-model"), "Model");
    assert_eq!(translator.text("mem-config-save"), "Save");
    assert_eq!(translator.text("mem-config-cancel"), "Cancel");
    assert_eq!(translator.text("mem-stats-title"), "Memory Info");
    assert_eq!(translator.text("mem-stats-total-records"), "Total Records");
    assert_eq!(
        translator.text("mem-stats-pinned-records"),
        "Pinned Records"
    );
    assert_eq!(
        translator.text("mem-stats-embedded-records"),
        "Embedded Records"
    );
    assert_eq!(
        translator.text("mem-stats-distinct-scopes"),
        "Distinct Scopes"
    );
    assert_eq!(translator.text("mem-stats-updated-24h"), "Updated Last 24h");
    assert_eq!(translator.text("mem-stats-updated-7d"), "Updated Last 7d");
    assert_eq!(translator.text("mem-stats-fts-enabled"), "FTS Enabled");
    assert_eq!(
        translator.text("mem-stats-vector-enabled"),
        "Vector Index Enabled"
    );
    assert_eq!(
        translator.text("mem-stats-avg-content"),
        "Avg Content Length"
    );
    assert_eq!(translator.text("mem-stats-created-min"), "Created Min");
    assert_eq!(translator.text("mem-stats-created-max"), "Created Max");
    assert_eq!(translator.text("mem-stats-updated-max"), "Updated Max");
}

#[test]
fn gui_memory_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("mem-subtitle"),
        "存储、搜索和管理跨会话的持久记忆记录。"
    );
    assert_eq!(translator.text("mem-btn-archive"), "立即归档");
    assert_eq!(translator.text("mem-status-archiving"), "正在归档...");
    assert_eq!(translator.text("mem-status-deleting"), "正在删除...");
    assert_eq!(translator.text("mem-status-loading"), "加载中...");
    assert_eq!(
        translator.text("mem-status-no-data"),
        "尚无可用的记忆数据。"
    );
    assert_eq!(translator.text("mem-tab-long-term"), "长期记忆");
    assert_eq!(translator.text("mem-tab-session-search"), "会话搜索");
    assert_eq!(translator.text("mem-tab-diagnostics"), "诊断");
    assert_eq!(translator.text("mem-filter-status-active"), "活动中");
    assert_eq!(translator.text("mem-filter-status-superseded"), "已取代");
    assert_eq!(translator.text("mem-filter-status-archived"), "已归档");
    assert_eq!(translator.text("mem-filter-status-rejected"), "已拒绝");
    assert_eq!(translator.text("mem-filter-status-all"), "全部");
    assert_eq!(translator.text("mem-filter-kind-all"), "全部类型");
    assert_eq!(translator.text("mem-filter-kind-identity"), "身份");
    assert_eq!(translator.text("mem-filter-kind-preference"), "偏好");
    assert_eq!(translator.text("mem-filter-kind-project-rule"), "项目规则");
    assert_eq!(translator.text("mem-filter-kind-workflow"), "工作流");
    assert_eq!(translator.text("mem-filter-kind-fact"), "事实");
    assert_eq!(translator.text("mem-filter-kind-constraint"), "约束");
    assert_eq!(translator.text("mem-topic-label"), "主题");
    assert_eq!(translator.text("mem-col-id"), "ID");
    assert_eq!(translator.text("mem-col-kind"), "类型");
    assert_eq!(translator.text("mem-col-status"), "状态");
    assert_eq!(translator.text("mem-col-priority"), "优先级");
    assert_eq!(translator.text("mem-col-topic"), "主题");
    assert_eq!(translator.text("mem-col-pin"), "固定");
    assert_eq!(translator.text("mem-col-summary"), "概要");
    assert_eq!(translator.text("mem-col-content"), "内容");
    assert_eq!(translator.text("mem-col-updated"), "更新时间");
    assert_eq!(translator.text("mem-pin-yes"), "是");
    assert_eq!(translator.text("mem-pin-no"), "否");
    assert_eq!(translator.text("mem-summary-type"), "概要");
    assert_eq!(translator.text("mem-summary-source"), "来源");
    assert_eq!(translator.text("mem-summary-none"), "-");
    assert_eq!(translator.text("mem-priority-none"), "-");
    assert_eq!(translator.text("mem-config-title"), "长期记忆嵌入配置");
    assert_eq!(translator.text("mem-config-enabled"), "嵌入已启用");
    assert_eq!(translator.text("mem-config-provider"), "提供者");
    assert_eq!(translator.text("mem-config-model"), "模型");
    assert_eq!(translator.text("mem-config-save"), "保存");
    assert_eq!(translator.text("mem-config-cancel"), "取消");
    assert_eq!(translator.text("mem-stats-title"), "记忆信息");
    assert_eq!(translator.text("mem-stats-total-records"), "总记录数");
    assert_eq!(translator.text("mem-stats-pinned-records"), "固定记录数");
    assert_eq!(
        translator.text("mem-stats-embedded-records"),
        "已嵌入记录数"
    );
    assert_eq!(translator.text("mem-stats-distinct-scopes"), "作用域数");
    assert_eq!(translator.text("mem-stats-updated-24h"), "最近 24 小时更新");
    assert_eq!(translator.text("mem-stats-updated-7d"), "最近 7 天更新");
    assert_eq!(translator.text("mem-stats-fts-enabled"), "FTS 已启用");
    assert_eq!(
        translator.text("mem-stats-vector-enabled"),
        "向量索引已启用"
    );
    assert_eq!(translator.text("mem-stats-avg-content"), "平均内容长度");
    assert_eq!(translator.text("mem-stats-created-min"), "最早创建时间");
    assert_eq!(translator.text("mem-stats-created-max"), "最晚创建时间");
    assert_eq!(translator.text("mem-stats-updated-max"), "最晚更新时间");
}

#[test]
fn gui_memory_panel_translates_notifications_with_args_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "mem-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ Refresh"
    );
    assert_eq!(
        translator.text_args("mem-btn-config", HashMap::from([("icon", "⚙".to_string())])),
        "⚙ Config"
    );
    assert_eq!(
        translator.text_args("mem-btn-info", HashMap::from([("icon", "ℹ".to_string())])),
        "ℹ Info"
    );
    assert_eq!(
        translator.text_args(
            "mem-records-count",
            HashMap::from([("count", "42".to_string())])
        ),
        "Records: 42"
    );
    assert_eq!(
        translator.text_args(
            "mem-detail-title",
            HashMap::from([("id", "abc123".to_string())])
        ),
        "Memory Detail — abc123"
    );
    assert_eq!(
        translator.text_args(
            "mem-ctx-detail",
            HashMap::from([("icon", "📄".to_string())])
        ),
        "📄 Detail"
    );
    assert_eq!(
        translator.text_args("mem-ctx-delete", HashMap::from([("icon", "🗑".to_string())])),
        "🗑 Delete"
    );
    assert_eq!(
        translator.text_args(
            "mem-delete-prompt",
            HashMap::from([("id", "abc123".to_string())])
        ),
        "Are you sure you want to delete memory record 'abc123'?"
    );
    assert_eq!(
        translator.text_args("mem-delete-btn", HashMap::from([("icon", "🗑".to_string())])),
        "🗑 Delete"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-load-failed",
            HashMap::from([("error", "disk".to_string())])
        ),
        "Failed to load memory panel: disk"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-save-failed",
            HashMap::from([("error", "write".to_string())])
        ),
        "Save failed: write"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-provider-unavailable",
            HashMap::from([("provider", "missing".to_string())])
        ),
        "Provider 'missing' is not available"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-session-search-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Session search failed: timeout"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-delete-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "Failed to delete record: io"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-archive-failed",
            HashMap::from([("error", "net".to_string())])
        ),
        "Archive run failed: net"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-supersedes",
            HashMap::from([("ids", "old-1, old-2".to_string())])
        ),
        "supersedes: old-1, old-2"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-superseded-by",
            HashMap::from([("id", "new-2".to_string())])
        ),
        "superseded_by: new-2"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-summary-sources",
            HashMap::from([("ids", "old-1, old-2".to_string())])
        ),
        "summary sources: old-1, old-2"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-archived-by",
            HashMap::from([("id", "summary-1".to_string())])
        ),
        "archived_by_summary: summary-1"
    );
    assert_eq!(
        translator.text_args(
            "mem-session-input",
            HashMap::from([("key", "sess-1".to_string())])
        ),
        "Input session: sess-1"
    );
    assert_eq!(
        translator.text_args(
            "mem-session-base",
            HashMap::from([("key", "base-1".to_string())])
        ),
        "Resolved base session: base-1"
    );
    assert_eq!(
        translator.text_args(
            "mem-session-window",
            HashMap::from([("days", "3".to_string()), ("limit", "8".to_string())])
        ),
        "Window: 3 day(s), limit 8"
    );
}

#[test]
fn gui_memory_panel_translates_notifications_with_args_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "mem-btn-refresh",
            HashMap::from([("icon", "⟳".to_string())])
        ),
        "⟳ 刷新"
    );
    assert_eq!(
        translator.text_args("mem-btn-config", HashMap::from([("icon", "⚙".to_string())])),
        "⚙ 配置"
    );
    assert_eq!(
        translator.text_args("mem-btn-info", HashMap::from([("icon", "ℹ".to_string())])),
        "ℹ 信息"
    );
    assert_eq!(
        translator.text_args(
            "mem-records-count",
            HashMap::from([("count", "42".to_string())])
        ),
        "记录: 42"
    );
    assert_eq!(
        translator.text_args(
            "mem-detail-title",
            HashMap::from([("id", "abc123".to_string())])
        ),
        "记忆详情 — abc123"
    );
    assert_eq!(
        translator.text_args(
            "mem-ctx-detail",
            HashMap::from([("icon", "📄".to_string())])
        ),
        "📄 详情"
    );
    assert_eq!(
        translator.text_args("mem-ctx-delete", HashMap::from([("icon", "🗑".to_string())])),
        "🗑 删除"
    );
    assert_eq!(
        translator.text_args(
            "mem-delete-prompt",
            HashMap::from([("id", "abc123".to_string())])
        ),
        "确定要删除记忆记录 'abc123' 吗？"
    );
    assert_eq!(
        translator.text_args("mem-delete-btn", HashMap::from([("icon", "🗑".to_string())])),
        "🗑 删除"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-load-failed",
            HashMap::from([("error", "磁盘".to_string())])
        ),
        "加载记忆面板失败: 磁盘"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-save-failed",
            HashMap::from([("error", "写入".to_string())])
        ),
        "保存失败: 写入"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-provider-unavailable",
            HashMap::from([("provider", "missing".to_string())])
        ),
        "提供者 'missing' 不可用"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-session-search-failed",
            HashMap::from([("error", "超时".to_string())])
        ),
        "会话搜索失败: 超时"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-delete-failed",
            HashMap::from([("error", "io".to_string())])
        ),
        "删除记录失败: io"
    );
    assert_eq!(
        translator.text_args(
            "mem-notify-archive-failed",
            HashMap::from([("error", "net".to_string())])
        ),
        "归档运行失败: net"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-supersedes",
            HashMap::from([("ids", "old-1, old-2".to_string())])
        ),
        "取代: old-1, old-2"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-superseded-by",
            HashMap::from([("id", "new-2".to_string())])
        ),
        "被取代: new-2"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-summary-sources",
            HashMap::from([("ids", "old-1, old-2".to_string())])
        ),
        "概要来源: old-1, old-2"
    );
    assert_eq!(
        translator.text_args(
            "mem-governance-archived-by",
            HashMap::from([("id", "summary-1".to_string())])
        ),
        "被概要归档: summary-1"
    );
    assert_eq!(
        translator.text_args(
            "mem-session-input",
            HashMap::from([("key", "sess-1".to_string())])
        ),
        "输入会话: sess-1"
    );
    assert_eq!(
        translator.text_args(
            "mem-session-base",
            HashMap::from([("key", "base-1".to_string())])
        ),
        "解析基础会话: base-1"
    );
    assert_eq!(
        translator.text_args(
            "mem-session-window",
            HashMap::from([("days", "3".to_string()), ("limit", "8".to_string())])
        ),
        "窗口: 3 天, 限制 8"
    );
}

#[test]
fn gui_observability_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(translator.text("obs-section-general"), "General");
    assert_eq!(translator.text("obs-section-metrics"), "Metrics");
    assert_eq!(translator.text("obs-section-traces"), "Traces");
    assert_eq!(translator.text("obs-section-otlp"), "OTLP Exporter");
    assert_eq!(
        translator.text("obs-section-prometheus"),
        "Prometheus Exporter"
    );
    assert_eq!(translator.text("obs-section-audit"), "Audit");
    assert_eq!(
        translator.text("obs-section-local-store"),
        "Local Analysis Store"
    );
    assert_eq!(translator.text("obs-section-pricing"), "Model Pricing");
    assert_eq!(translator.text("obs-field-enabled"), "Enabled");
    assert_eq!(translator.text("obs-field-service-name"), "Service Name");
    assert_eq!(
        translator.text("obs-field-service-version"),
        "Service Version"
    );
    assert_eq!(
        translator.text("obs-field-export-interval"),
        "Export Interval (seconds)"
    );
    assert_eq!(
        translator.text("obs-field-sample-rate"),
        "Sample Rate (0.0-1.0)"
    );
    assert_eq!(translator.text("obs-field-endpoint"), "Endpoint");
    assert_eq!(translator.text("obs-field-listen-port"), "Listen Port");
    assert_eq!(translator.text("obs-field-path"), "Path");
    assert_eq!(
        translator.text("obs-field-output-path"),
        "Output Path (optional)"
    );
    assert_eq!(
        translator.text("obs-field-retention-days"),
        "Retention Days"
    );
    assert_eq!(
        translator.text("obs-field-flush-interval"),
        "Flush Interval (seconds)"
    );
    assert_eq!(translator.text("obs-status-label"), "Status:");
    assert_eq!(translator.text("obs-status-enabled"), "Enabled");
    assert_eq!(translator.text("obs-status-disabled"), "Disabled");
    assert_eq!(translator.text("obs-status-unsaved"), "(unsaved changes)");
    assert_eq!(
        translator.text("obs-note-restart"),
        "Note: Changes require restart to take effect."
    );
}

#[test]
fn gui_observability_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(translator.text("obs-section-general"), "通用");
    assert_eq!(translator.text("obs-section-metrics"), "指标");
    assert_eq!(translator.text("obs-section-traces"), "链路追踪");
    assert_eq!(translator.text("obs-section-otlp"), "OTLP 导出器");
    assert_eq!(
        translator.text("obs-section-prometheus"),
        "Prometheus 导出器"
    );
    assert_eq!(translator.text("obs-section-audit"), "审计");
    assert_eq!(translator.text("obs-section-local-store"), "本地分析存储");
    assert_eq!(translator.text("obs-section-pricing"), "模型定价");
    assert_eq!(translator.text("obs-field-enabled"), "启用");
    assert_eq!(translator.text("obs-field-service-name"), "服务名称");
    assert_eq!(translator.text("obs-field-service-version"), "服务版本");
    assert_eq!(
        translator.text("obs-field-export-interval"),
        "导出间隔（秒）"
    );
    assert_eq!(translator.text("obs-field-sample-rate"), "采样率 (0.0-1.0)");
    assert_eq!(translator.text("obs-field-endpoint"), "端点");
    assert_eq!(translator.text("obs-field-listen-port"), "监听端口");
    assert_eq!(translator.text("obs-field-path"), "路径");
    assert_eq!(translator.text("obs-field-output-path"), "输出路径（可选）");
    assert_eq!(translator.text("obs-field-retention-days"), "保留天数");
    assert_eq!(
        translator.text("obs-field-flush-interval"),
        "刷新间隔（秒）"
    );
    assert_eq!(translator.text("obs-status-label"), "状态:");
    assert_eq!(translator.text("obs-status-enabled"), "已启用");
    assert_eq!(translator.text("obs-status-disabled"), "已禁用");
    assert_eq!(translator.text("obs-status-unsaved"), "(未保存的更改)");
    assert_eq!(
        translator.text("obs-note-restart"),
        "注意：更改需要重启才能生效。"
    );
}

#[test]
fn gui_observability_panel_translates_notifications_with_args_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "obs-notify-config-load-failed",
            HashMap::from([("error", "disk error".to_string())])
        ),
        "Failed to load config: disk error"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-save-parse-failed",
            HashMap::from([("error", "parse error".to_string())])
        ),
        "Save failed: parse error"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-save-write-failed",
            HashMap::from([("error", "write error".to_string())])
        ),
        "Save failed: write error"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-reload-failed",
            HashMap::from([("error", "io error".to_string())])
        ),
        "Reload failed: io error"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-price-duplicate",
            HashMap::from([
                ("provider", "openai".to_string()),
                ("model", "gpt-4".to_string())
            ])
        ),
        "Price entry for openai/gpt-4 already exists"
    );
    assert_eq!(
        translator.text_args(
            "obs-price-delete-prompt",
            HashMap::from([
                ("provider", "openai".to_string()),
                ("model", "gpt-4".to_string())
            ])
        ),
        "Delete price entry for openai/gpt-4?"
    );
}

#[test]
fn gui_observability_panel_translates_notifications_with_args_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "obs-notify-config-load-failed",
            HashMap::from([("error", "磁盘错误".to_string())])
        ),
        "加载配置失败: 磁盘错误"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-save-parse-failed",
            HashMap::from([("error", "解析错误".to_string())])
        ),
        "保存失败: 解析错误"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-save-write-failed",
            HashMap::from([("error", "写入错误".to_string())])
        ),
        "保存失败: 写入错误"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-reload-failed",
            HashMap::from([("error", "IO错误".to_string())])
        ),
        "重载失败: IO错误"
    );
    assert_eq!(
        translator.text_args(
            "obs-notify-price-duplicate",
            HashMap::from([
                ("provider", "openai".to_string()),
                ("model", "gpt-4".to_string())
            ])
        ),
        "提供者 openai/模型 gpt-4 的定价条目已存在"
    );
    assert_eq!(
        translator.text_args(
            "obs-price-delete-prompt",
            HashMap::from([
                ("provider", "openai".to_string()),
                ("model", "gpt-4".to_string())
            ])
        ),
        "确定删除定价条目 openai/gpt-4 吗？"
    );
}

#[test]
fn webui_thinking_placeholder_translates_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);
    assert_eq!(translator.text("assistant-label"), "Klaw");
    assert_eq!(translator.text("thinking"), "Thinking…");
}

#[test]
fn webui_thinking_placeholder_translates_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);
    assert_eq!(translator.text("assistant-label"), "Klaw");
    assert_eq!(translator.text("thinking"), "思考中…");
}

#[test]
fn webui_history_loading_states_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);
    assert_eq!(
        translator.text("history-loading-title"),
        "Loading conversation history…"
    );
    assert_eq!(
        translator.text("history-loading-body"),
        "Fetching messages from Klaw gateway."
    );
    assert_eq!(
        translator.text("history-page-loading"),
        "Loading older messages…"
    );
}

#[test]
fn webui_history_loading_states_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("history-loading-title"),
        "正在加载对话历史…"
    );
    assert_eq!(
        translator.text("history-loading-body"),
        "正在从 Klaw 网关获取消息。"
    );
    assert_eq!(
        translator.text("history-page-loading"),
        "正在加载更早的消息…"
    );
}

#[test]
fn webui_role_labels_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("role-you"), "You");
    assert_eq!(translator.text("role-system"), "System");
}

#[test]
fn webui_role_labels_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("role-you"), "你");
    assert_eq!(translator.text("role-system"), "系统");
}

#[test]
fn webui_card_completion_labels_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("card-approved"), "Approved");
    assert_eq!(translator.text("card-rejected"), "Rejected");
}

#[test]
fn webui_card_completion_labels_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("card-approved"), "已审批");
    assert_eq!(translator.text("card-rejected"), "已拒绝");
}

#[test]
fn webui_archive_preview_labels_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(
        translator.text("archive-preview-loading"),
        "Loading preview..."
    );
    assert_eq!(
        translator.text("archive-preview-unavailable"),
        "Preview is not available for this file type."
    );
    assert_eq!(
        translator.text("archive-hover-preview"),
        "Preview archive resource"
    );
    assert_eq!(
        translator.text("archive-hover-download"),
        "Download archive resource"
    );
}

#[test]
fn webui_archive_preview_labels_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(
        translator.text("archive-preview-loading"),
        "正在加载预览..."
    );
    assert_eq!(
        translator.text("archive-preview-unavailable"),
        "此文件类型不支持预览。"
    );
    assert_eq!(translator.text("archive-hover-preview"), "预览存档资源");
    assert_eq!(translator.text("archive-hover-download"), "下载存档资源");
}

#[test]
fn webui_route_labels_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("route-default"), "Route: default");
    assert_eq!(
        translator.text_args(
            "route-provider",
            HashMap::from([("provider", "openai".to_string())])
        ),
        "Route: openai"
    );
    assert_eq!(
        translator.text_args(
            "route-model",
            HashMap::from([("model", "gpt-4".to_string())])
        ),
        "Route: gpt-4"
    );
    assert_eq!(
        translator.text_args(
            "route-provider-model",
            HashMap::from([
                ("provider", "openai".to_string()),
                ("model", "gpt-4".to_string())
            ])
        ),
        "Route: openai/gpt-4"
    );
}

#[test]
fn webui_route_labels_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("route-default"), "路由：默认");
    assert_eq!(
        translator.text_args(
            "route-provider",
            HashMap::from([("provider", "openai".to_string())])
        ),
        "路由：openai"
    );
    assert_eq!(
        translator.text_args(
            "route-model",
            HashMap::from([("model", "gpt-4".to_string())])
        ),
        "路由：gpt-4"
    );
    assert_eq!(
        translator.text_args(
            "route-provider-model",
            HashMap::from([
                ("provider", "openai".to_string()),
                ("model", "gpt-4".to_string())
            ])
        ),
        "路由：openai/gpt-4"
    );
}

#[test]
fn webui_activity_labels_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(translator.text("activity-history"), "History");
    assert_eq!(translator.text("activity-uploading"), "Uploading");
    assert_eq!(translator.text("activity-picking-file"), "Picking File");
    assert_eq!(translator.text("activity-streaming"), "Streaming");
    assert_eq!(translator.text("activity-files-ready"), "Files Ready");
}

#[test]
fn webui_activity_labels_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(translator.text("activity-history"), "历史");
    assert_eq!(translator.text("activity-uploading"), "上传中");
    assert_eq!(translator.text("activity-picking-file"), "选择文件");
    assert_eq!(translator.text("activity-streaming"), "流式传输中");
    assert_eq!(translator.text("activity-files-ready"), "文件就绪");
}

#[test]
fn webui_statusbar_messages_translate_in_english() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::English);

    assert_eq!(
        translator.text_args(
            "statusbar-messages",
            HashMap::from([("count", "5".to_string())])
        ),
        "5 msgs"
    );
}

#[test]
fn webui_statusbar_messages_translate_in_chinese() {
    let translator = Translator::new(LocaleDomain::WebUi, UiLanguage::SimplifiedChinese);

    assert_eq!(
        translator.text_args(
            "statusbar-messages",
            HashMap::from([("count", "5".to_string())])
        ),
        "5 条消息"
    );
}

#[test]
fn gui_logs_panel_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text("logs-subtitle"),
        "Live process logs from tracing output"
    );
    assert_eq!(translator.text("logs-level-trace"), "trace");
    assert_eq!(translator.text("logs-level-debug"), "debug");
    assert_eq!(translator.text("logs-level-info"), "info");
    assert_eq!(translator.text("logs-level-warn"), "warn");
    assert_eq!(translator.text("logs-level-error"), "error");
    assert_eq!(translator.text("logs-level-unknown"), "unknown");
    assert_eq!(translator.text("logs-search"), "Search");
    assert_eq!(translator.text("logs-pause-stream"), "Pause stream");
    assert_eq!(translator.text("logs-auto-scroll"), "Auto-scroll");
    assert_eq!(translator.text("logs-btn-clear"), "Clear");
    assert_eq!(translator.text("logs-btn-apply"), "Apply");
    assert_eq!(translator.text("logs-btn-export"), "Export");
    assert_eq!(translator.text("logs-max-lines"), "Max lines");
    assert_eq!(translator.text("logs-export-path"), "Export path");
    assert_eq!(
        translator.text("logs-notify-buffer-cleared"),
        "Log buffer cleared"
    );
    assert_eq!(
        translator.text("logs-notify-capacity-updated"),
        "Log capacity updated"
    );
}

#[test]
fn gui_logs_panel_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text("logs-subtitle"),
        "来自追踪输出的实时进程日志"
    );
    assert_eq!(translator.text("logs-level-trace"), "跟踪");
    assert_eq!(translator.text("logs-level-debug"), "调试");
    assert_eq!(translator.text("logs-level-info"), "信息");
    assert_eq!(translator.text("logs-level-warn"), "警告");
    assert_eq!(translator.text("logs-level-error"), "错误");
    assert_eq!(translator.text("logs-level-unknown"), "未知");
    assert_eq!(translator.text("logs-search"), "搜索");
    assert_eq!(translator.text("logs-pause-stream"), "暂停流");
    assert_eq!(translator.text("logs-auto-scroll"), "自动滚动");
    assert_eq!(translator.text("logs-btn-clear"), "清除");
    assert_eq!(translator.text("logs-btn-apply"), "应用");
    assert_eq!(translator.text("logs-btn-export"), "导出");
    assert_eq!(translator.text("logs-max-lines"), "最大行数");
    assert_eq!(translator.text("logs-export-path"), "导出路径");
    assert_eq!(
        translator.text("logs-notify-buffer-cleared"),
        "日志缓冲已清除"
    );
    assert_eq!(
        translator.text("logs-notify-capacity-updated"),
        "日志容量已更新"
    );
}

#[test]
fn gui_logs_panel_translates_parameterized_keys_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "logs-notify-exported",
            HashMap::from([("path", "/tmp/gui-live.log".to_string())])
        ),
        "Logs exported to /tmp/gui-live.log"
    );
    assert_eq!(
        translator.text_args(
            "logs-stats-line",
            HashMap::from([
                ("buffered", "100".to_string()),
                ("visible", "80".to_string()),
                ("panel_dropped", "5".to_string()),
                ("transport_dropped", "3".to_string()),
                ("bridge_dropped", "2".to_string()),
            ])
        ),
        "Buffered: 100 | Visible: 80 | Panel dropped: 5 | Transport dropped: 3 | Bridge dropped: 2"
    );
    assert_eq!(
        translator.text_args(
            "logs-transport-warning",
            HashMap::from([("chunks", "10".to_string()), ("bytes", "4096".to_string()),])
        ),
        "GUI transport has dropped 10 chunks (4096 bytes). Runtime logging continued, but the GUI sink fell behind."
    );
}

#[test]
fn gui_logs_panel_translates_parameterized_keys_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "logs-notify-exported",
            HashMap::from([("path", "/tmp/gui-live.log".to_string())])
        ),
        "日志已导出到 /tmp/gui-live.log"
    );
    assert_eq!(
        translator.text_args(
            "logs-stats-line",
            HashMap::from([
                ("buffered", "100".to_string()),
                ("visible", "80".to_string()),
                ("panel_dropped", "5".to_string()),
                ("transport_dropped", "3".to_string()),
                ("bridge_dropped", "2".to_string()),
            ])
        ),
        "缓冲: 100 | 可见: 80 | 面板丢弃: 5 | 传输丢弃: 3 | 桥接丢弃: 2"
    );
    assert_eq!(
        translator.text_args(
            "logs-transport-warning",
            HashMap::from([("chunks", "10".to_string()), ("bytes", "4096".to_string()),])
        ),
        "GUI 传输已丢弃 10 个数据块 (4096 字节)。运行时日志仍在继续，但 GUI 接收端落后了。"
    );
}

#[test]
fn gui_analyze_dashboard_translates_labels_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    // Panel header
    assert_eq!(
        translator.text("ad-subtitle"),
        "Tool and model analysis from the local observability store"
    );
    assert_eq!(
        translator.text("ad-obs-disabled"),
        "Observability is disabled. Enable it in the Observability panel first."
    );
    assert_eq!(
        translator.text("ad-local-store-disabled"),
        "Local analysis store is disabled. Enable it in the Observability panel."
    );
    assert_eq!(
        translator.text("ad-notify-worker-disconnected"),
        "Analyze Dashboard worker disconnected"
    );
    assert_eq!(
        translator.text("ad-resolve-data-dir-failed"),
        "Unable to resolve local data directory"
    );
    // Controls
    assert_eq!(translator.text("ad-view"), "View:");
    assert_eq!(translator.text("ad-view-tools"), "Tools");
    assert_eq!(translator.text("ad-view-models"), "Models");
    assert_eq!(translator.text("ad-time-range"), "Time Range:");
    assert_eq!(translator.text("ad-granularity"), "Granularity:");
    assert_eq!(translator.text("ad-all-providers"), "All Providers");
    assert_eq!(translator.text("ad-all-models"), "All Models");
    assert_eq!(translator.text("ad-refresh"), "Refresh");
    assert_eq!(translator.text("ad-loading"), "Loading...");
    // Summary cards
    assert_eq!(translator.text("ad-total-calls"), "Total Calls");
    assert_eq!(translator.text("ad-success-rate"), "Success Rate");
    assert_eq!(translator.text("ad-failures"), "Failures");
    assert_eq!(translator.text("ad-avg-duration"), "Avg Duration");
    assert_eq!(translator.text("ad-total-requests"), "Total Requests");
    assert_eq!(translator.text("ad-p95-duration"), "P95 Duration");
    assert_eq!(translator.text("ad-total-tokens"), "Total Tokens");
    assert_eq!(translator.text("ad-estimated-cost"), "Estimated Cost");
    assert_eq!(translator.text("ad-tool-call-rate"), "Tool Call Rate");
    assert_eq!(translator.text("ad-turn-completion"), "Turn Completion");
    // Table titles
    assert_eq!(
        translator.text("ad-top-tools-by-calls"),
        "Top Tools by Calls"
    );
    assert_eq!(
        translator.text("ad-top-tools-by-failure-load"),
        "Top Tools by Failure Load"
    );
    assert_eq!(
        translator.text("ad-top-models-by-requests"),
        "Top Models by Requests"
    );
    assert_eq!(
        translator.text("ad-top-models-by-tokens"),
        "Top Models by Token Usage"
    );
    assert_eq!(
        translator.text("ad-worst-models-by-failures"),
        "Worst Models by Failure Load"
    );
    assert_eq!(
        translator.text("ad-highest-p95-models"),
        "Highest P95 Latency Models"
    );
    assert_eq!(
        translator.text("ad-highest-cost-models"),
        "Highest Cost Models"
    );
    // Column headers
    assert_eq!(translator.text("ad-col-tool"), "Tool");
    assert_eq!(translator.text("ad-col-calls"), "Calls");
    assert_eq!(translator.text("ad-col-success"), "Success");
    assert_eq!(translator.text("ad-col-failures"), "Failures");
    assert_eq!(translator.text("ad-col-model"), "Model");
    assert_eq!(translator.text("ad-col-requests"), "Requests");
    assert_eq!(translator.text("ad-col-tokens"), "Tokens");
    assert_eq!(translator.text("ad-col-avg"), "Avg");
    assert_eq!(translator.text("ad-col-timeout"), "Timeout");
    assert_eq!(translator.text("ad-col-p95"), "P95");
    assert_eq!(translator.text("ad-col-cost"), "Cost");
    assert_eq!(translator.text("ad-col-cost-per-success"), "Cost/Success");
    assert_eq!(translator.text("ad-col-approval"), "Approval");
    // Error breakdown
    assert_eq!(translator.text("ad-error-breakdown"), "Error Breakdown");
    assert_eq!(
        translator.text("ad-error-breakdown-provider-model"),
        "Error Breakdown by Provider/Model"
    );
    assert_eq!(
        translator.text("ad-no-tool-failures"),
        "No failures in the selected time range."
    );
    assert_eq!(
        translator.text("ad-no-model-failures"),
        "No model request failures in the selected time range."
    );
    // Timeseries
    assert_eq!(
        translator.text("ad-no-samples"),
        "No samples in the selected time range."
    );
    assert_eq!(
        translator.text("ad-no-model-samples"),
        "No model samples in the selected time range."
    );
    // Token composition
    assert_eq!(translator.text("ad-token-composition"), "Token Composition");
    assert_eq!(translator.text("ad-input-tokens"), "Input Tokens");
    assert_eq!(translator.text("ad-output-tokens"), "Output Tokens");
    assert_eq!(
        translator.text("ad-cached-input-tokens"),
        "Cached Input Tokens"
    );
    assert_eq!(translator.text("ad-reasoning-tokens"), "Reasoning Tokens");
    // Model tool breakdown
    assert_eq!(
        translator.text("ad-model-tool-breakdown"),
        "Selected Model Tool Success Breakdown"
    );
    assert_eq!(
        translator.text("ad-no-model-tool-data"),
        "No model-attributed tool data in the selected time range."
    );
    // Empty states
    assert_eq!(
        translator.text("ad-no-tool-metrics"),
        "No local tool metrics yet."
    );
    assert_eq!(
        translator.text("ad-no-model-metrics"),
        "No local model metrics yet."
    );
    assert_eq!(
        translator.text("ad-no-model-level-metrics"),
        "No model-level metrics yet. New charts populate from new telemetry."
    );
    assert_eq!(translator.text("ad-no-tool-data"), "No tool data.");
    assert_eq!(translator.text("ad-no-model-data"), "No model data.");
    assert_eq!(translator.text("ad-na"), "N/A");
    // Legends
    assert_eq!(translator.text("ad-legend-success-rate"), "Success Rate");
    assert_eq!(translator.text("ad-legend-calls"), "Calls");
    assert_eq!(
        translator.text("ad-legend-tool-call-rate"),
        "Tool Call Rate"
    );
    assert_eq!(
        translator.text("ad-legend-tool-success-rate"),
        "Tool Success Rate"
    );
    assert_eq!(translator.text("ad-legend-avg-duration"), "Avg Duration");
    assert_eq!(translator.text("ad-legend-p95-duration"), "P95 Duration");
    assert_eq!(translator.text("ad-legend-token-usage"), "Token Usage");
    assert_eq!(
        translator.text("ad-legend-requests-per-turn"),
        "Requests/Turn"
    );
    assert_eq!(
        translator.text("ad-legend-tool-iterations-per-turn"),
        "Tool Iterations/Turn"
    );
}

#[test]
fn gui_analyze_dashboard_translates_labels_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    // Panel header
    assert_eq!(
        translator.text("ad-subtitle"),
        "从本地可观测性存储进行工具与模型分析"
    );
    assert_eq!(
        translator.text("ad-obs-disabled"),
        "可观测性已禁用。请先在可观测性面板中启用。"
    );
    assert_eq!(
        translator.text("ad-local-store-disabled"),
        "本地分析存储已禁用。请先在可观测性面板中启用。"
    );
    assert_eq!(
        translator.text("ad-notify-worker-disconnected"),
        "分析仪表盘工作线程已断开"
    );
    assert_eq!(
        translator.text("ad-resolve-data-dir-failed"),
        "无法解析本地数据目录"
    );
    // Controls
    assert_eq!(translator.text("ad-view"), "视图:");
    assert_eq!(translator.text("ad-view-tools"), "工具");
    assert_eq!(translator.text("ad-view-models"), "模型");
    assert_eq!(translator.text("ad-time-range"), "时间范围:");
    assert_eq!(translator.text("ad-granularity"), "粒度:");
    assert_eq!(translator.text("ad-all-providers"), "全部提供商");
    assert_eq!(translator.text("ad-all-models"), "全部模型");
    assert_eq!(translator.text("ad-refresh"), "刷新");
    assert_eq!(translator.text("ad-loading"), "加载中...");
    // Summary cards
    assert_eq!(translator.text("ad-total-calls"), "总调用次数");
    assert_eq!(translator.text("ad-success-rate"), "成功率");
    assert_eq!(translator.text("ad-failures"), "失败次数");
    assert_eq!(translator.text("ad-avg-duration"), "平均耗时");
    assert_eq!(translator.text("ad-total-requests"), "总请求数");
    assert_eq!(translator.text("ad-p95-duration"), "P95 耗时");
    assert_eq!(translator.text("ad-total-tokens"), "总 Token 数");
    assert_eq!(translator.text("ad-estimated-cost"), "预估费用");
    assert_eq!(translator.text("ad-tool-call-rate"), "工具调用率");
    assert_eq!(translator.text("ad-turn-completion"), "回合完成率");
    // Table titles
    assert_eq!(
        translator.text("ad-top-tools-by-calls"),
        "按调用次数排行工具"
    );
    assert_eq!(
        translator.text("ad-top-tools-by-failure-load"),
        "按失败负载排行工具"
    );
    assert_eq!(
        translator.text("ad-top-models-by-requests"),
        "按请求数排行模型"
    );
    assert_eq!(
        translator.text("ad-top-models-by-tokens"),
        "按 Token 用量排行模型"
    );
    assert_eq!(
        translator.text("ad-worst-models-by-failures"),
        "按失败负载排行模型"
    );
    assert_eq!(
        translator.text("ad-highest-p95-models"),
        "最高 P95 延迟模型"
    );
    assert_eq!(translator.text("ad-highest-cost-models"), "最高费用模型");
    // Column headers
    assert_eq!(translator.text("ad-col-tool"), "工具");
    assert_eq!(translator.text("ad-col-calls"), "调用");
    assert_eq!(translator.text("ad-col-success"), "成功");
    assert_eq!(translator.text("ad-col-failures"), "失败");
    assert_eq!(translator.text("ad-col-model"), "模型");
    assert_eq!(translator.text("ad-col-requests"), "请求数");
    assert_eq!(translator.text("ad-col-tokens"), "Token 数");
    assert_eq!(translator.text("ad-col-avg"), "平均");
    assert_eq!(translator.text("ad-col-timeout"), "超时");
    assert_eq!(translator.text("ad-col-p95"), "P95");
    assert_eq!(translator.text("ad-col-cost"), "费用");
    assert_eq!(translator.text("ad-col-cost-per-success"), "费用/成功");
    assert_eq!(translator.text("ad-col-approval"), "审批");
    // Error breakdown
    assert_eq!(translator.text("ad-error-breakdown"), "错误分布");
    assert_eq!(
        translator.text("ad-error-breakdown-provider-model"),
        "按提供商/模型的错误分布"
    );
    assert_eq!(
        translator.text("ad-no-tool-failures"),
        "所选时间范围内无失败记录。"
    );
    assert_eq!(
        translator.text("ad-no-model-failures"),
        "所选时间范围内无模型请求失败。"
    );
    // Timeseries
    assert_eq!(
        translator.text("ad-no-samples"),
        "所选时间范围内无样本数据。"
    );
    assert_eq!(
        translator.text("ad-no-model-samples"),
        "所选时间范围内无模型样本数据。"
    );
    // Token composition
    assert_eq!(translator.text("ad-token-composition"), "Token 构成");
    assert_eq!(translator.text("ad-input-tokens"), "输入 Token");
    assert_eq!(translator.text("ad-output-tokens"), "输出 Token");
    assert_eq!(translator.text("ad-cached-input-tokens"), "缓存输入 Token");
    assert_eq!(translator.text("ad-reasoning-tokens"), "推理 Token");
    // Model tool breakdown
    assert_eq!(
        translator.text("ad-model-tool-breakdown"),
        "所选模型的工具成功分布"
    );
    assert_eq!(
        translator.text("ad-no-model-tool-data"),
        "所选时间范围内无模型关联的工具数据。"
    );
    // Empty states
    assert_eq!(
        translator.text("ad-no-tool-metrics"),
        "暂无本地工具指标数据。"
    );
    assert_eq!(
        translator.text("ad-no-model-metrics"),
        "暂无本地模型指标数据。"
    );
    assert_eq!(
        translator.text("ad-no-model-level-metrics"),
        "暂无模型级指标数据。新图表将从新遥测数据中填充。"
    );
    assert_eq!(translator.text("ad-no-tool-data"), "无工具数据。");
    assert_eq!(translator.text("ad-no-model-data"), "无模型数据。");
    assert_eq!(translator.text("ad-na"), "无数据");
    // Legends
    assert_eq!(translator.text("ad-legend-success-rate"), "成功率");
    assert_eq!(translator.text("ad-legend-calls"), "调用");
    assert_eq!(translator.text("ad-legend-tool-call-rate"), "工具调用率");
    assert_eq!(translator.text("ad-legend-tool-success-rate"), "工具成功率");
    assert_eq!(translator.text("ad-legend-avg-duration"), "平均耗时");
    assert_eq!(translator.text("ad-legend-p95-duration"), "P95 耗时");
    assert_eq!(translator.text("ad-legend-token-usage"), "Token 用量");
    assert_eq!(
        translator.text("ad-legend-requests-per-turn"),
        "请求数/回合"
    );
    assert_eq!(
        translator.text("ad-legend-tool-iterations-per-turn"),
        "工具迭代/回合"
    );
}

#[test]
fn gui_analyze_dashboard_translates_parameterized_keys_in_english() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::English);
    assert_eq!(
        translator.text_args(
            "ad-notify-load-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "Analyze Dashboard load failed: timeout"
    );
    assert_eq!(
        translator.text_args(
            "ad-error-breakdown-tool",
            HashMap::from([("tool", "file_read".to_string())])
        ),
        "Error Breakdown: file_read"
    );
    assert_eq!(
        translator.text_args(
            "ad-success-rate-trend",
            HashMap::from([("bucket", "1h".to_string())])
        ),
        "Success Rate Trend (1h)"
    );
    assert_eq!(
        translator.text_args(
            "ad-model-trends",
            HashMap::from([("bucket", "1h".to_string())])
        ),
        "Model Trends (1h)"
    );
    assert_eq!(
        translator.text_args(
            "ad-updated-ago",
            HashMap::from([("seconds", "5".to_string())])
        ),
        "Updated 5s ago"
    );
}

#[test]
fn gui_analyze_dashboard_translates_parameterized_keys_in_chinese() {
    let translator = Translator::new(LocaleDomain::Gui, UiLanguage::SimplifiedChinese);
    assert_eq!(
        translator.text_args(
            "ad-notify-load-failed",
            HashMap::from([("error", "timeout".to_string())])
        ),
        "分析仪表盘加载失败: timeout"
    );
    assert_eq!(
        translator.text_args(
            "ad-error-breakdown-tool",
            HashMap::from([("tool", "file_read".to_string())])
        ),
        "错误分布: file_read"
    );
    assert_eq!(
        translator.text_args(
            "ad-success-rate-trend",
            HashMap::from([("bucket", "1h".to_string())])
        ),
        "成功率趋势 (1h)"
    );
    assert_eq!(
        translator.text_args(
            "ad-model-trends",
            HashMap::from([("bucket", "1h".to_string())])
        ),
        "模型趋势 (1h)"
    );
    assert_eq!(
        translator.text_args(
            "ad-updated-ago",
            HashMap::from([("seconds", "5".to_string())])
        ),
        "更新于 5秒前"
    );
}
