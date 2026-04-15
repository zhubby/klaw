# CHANGELOG

## 2026-04-15

### Added

- 新增共享主题模块，统一提供 `LightThemePreset`、`DarkThemePreset`、preset 标签文案，以及 light/dark `egui::Visuals` 构建逻辑

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
