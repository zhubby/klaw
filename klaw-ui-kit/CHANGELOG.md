# CHANGELOG

## 2026-04-09

### Added

- 初始共享 UI crate，提供 `ThemeMode`、`theme_preference()` 与跨前端复用的 `NotificationCenter`
- 新增共享字体安装入口 `install_fonts()`，统一封装内嵌 LXGW WenKai 字体、Phosphor 图标字体，以及桌面端系统 CJK fallback 逻辑