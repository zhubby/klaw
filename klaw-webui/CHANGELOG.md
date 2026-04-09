# CHANGELOG

## 2026-04-09

### Added

- `klaw-webui` 现在依赖新的共享基础 crate `klaw-ui-kit`，复用 `ThemeMode`、`theme_preference()` 与 `NotificationCenter`

### Changed

- 将原来的单文件 `src/web_chat.rs` 拆分为 `app`、`session`、`protocol`、`storage`、`transport` 和 `ui` 模块，明确浏览器聊天 UI 的职责边界

## 2026-04-08

### Added

- 初始版本：WASM 聊天 UI（egui）、WebSocket 客户端、`web:` 会话前缀与可选 gateway token query

### Changed

- 收平 crate 内部结构：移除独立 `presentation` 层，改为由 crate 根部复用纯逻辑并由 `web_chat` 直接消费单一连接状态
