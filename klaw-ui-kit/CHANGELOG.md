# CHANGELOG

## 2026-05-15

### Added

- WebUI i18n key set for complete WebUI internationalization (settings, about, archive preview, session sidebar, dialogs, composer area, workbench, status bar, empty state, card messages, file dialog, attachment context menu, role labels, route labels, activity labels).
- Locale tests for all new WebUI keys in both English and Simplified Chinese.

## 2026-05-09

### Added

- 新增共享 `UiLanguage`、`LocaleDomain` 与 Fluent `Translator`，为桌面 GUI 和 WebUI 提供 English / 简体中文的嵌入式 i18n 基础能力
- 新增分域 Fluent 资源，GUI 与 WebUI 使用独立 `gui.ftl` / `webui.ftl` key 集，并以英文资源作为 fallback
- 新增 System 面板完整 i18n key 集（视图标签、目录标题、主机信息行标签、参数化文案、环境依赖检查、清除确认对话框、通知消息等），覆盖 `en-US/gui.ftl` 和 `zh-CN/gui.ftl`
- 新增 System 面板 i18n 单元测试（视图标签、目录标题、主机信息标签、参数化 key 在英文/简体中文下的翻译验证）
- 新增 ACP 面板完整 i18n key 集（统计区、代理表格、配置表单、全局设置、删除确认、详情窗口、测试提示窗口、内容块渲染、权限标签、通知消息等），覆盖 `en-US/gui.ftl` 和 `zh-CN/gui.ftl`
- 新增 LLM 面板完整 i18n key 集（筛选栏、表格列标题、上下文菜单、审计详情窗口、排序标签、状态显示、通知消息等），覆盖 `en-US/gui.ftl` 和 `zh-CN/gui.ftl`
- 新增 MCP 面板完整 i18n key 集（服务器表格、配置表单、全局设置、详情窗口、上下文菜单、状态标签、Markdown 详情、通知消息等），覆盖 `en-US/gui.ftl` 和 `zh-CN/gui.ftl`
- 新增 ACP/LLM/MCP 面板 i18n 单元测试（各面板标签在英文/简体中文下的翻译验证）

## 2026-04-15

### Added

- 新增共享主题模块，统一提供 `LightThemePreset`、`DarkThemePreset`、preset 标签文案，以及 light/dark `egui::Visuals` 构建逻辑
- 新增暗色主题 preset `Blackpink`，使用高对比黑底与粉色高亮风格

### Changed

- `klaw-ui-kit` 现在导出共享 `apply_theme()`，供 `klaw-gui` 与 `klaw-webui` 复用同一套主题 mode + preset 应用逻辑

## 2026-04-14

### Changed

- `install_fonts()` now selects embedded fonts through Cargo features: `fonts-lxgw` is the default, `fonts-noto-sans` is the alternate option, disabling both falls back to `egui` defaults plus existing desktop system CJK fallback loading, and enabling both now fails compilation

## 2026-04-13

### Added

- 新增共享三态 `ThemeSwitch` widget，围绕 `egui::ThemePreference` 提供 system/light/dark 主题切换，并暴露 `global_theme_switch()` 便于直接绑定全局主题

### Changed

- `foundation` 现在额外提供 `theme_mode_from_preference()` 与 `theme_preference_label()`，统一桌面端和 Web 端的主题模式转换与显示文案

## 2026-04-09

### Added

- 初始共享 UI crate，提供 `ThemeMode`、`theme_preference()` 与跨前端复用的 `NotificationCenter`
- 新增共享字体安装入口 `install_fonts()`，统一封装内嵌 LXGW WenKai 字体、Phosphor 图标字体，以及桌面端系统 CJK fallback 逻辑
