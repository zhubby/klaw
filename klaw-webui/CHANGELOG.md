# CHANGELOG

## 2026-04-10

### Added

- `klaw-webui` 的浏览器业务状态现在会持久化 gateway token，刷新页面后仍可恢复
- 工作区 bootstrap 完成后，对所有 **已打开** 的 agent 窗口调用 `session.subscribe`，各自拉取历史（不再仅订阅服务端 `active_session_key`）
- 连接前将输入框中的 token 写回 `gateway_token` 并 `localStorage` 落盘，避免只在输入框里填了 token、未点「Save & Reconnect」时刷新丢失

### Changed

- `gateway_token` 的恢复策略改为“URL query 优先，其次使用 localStorage 中的上次保存值”，便于显式覆盖浏览器内已缓存的 token
- `egui/eframe` 的内建 persistence 现已启用，主题偏好、侧栏宽度与 agent 窗口布局改由框架自身恢复，不再由 `klaw-webui` 手写持久化
- 与服务端会话列表对齐时：仅在本地已有记录时恢复 `open`，**新出现的 agent 默认不自动打开窗口**（`PersistedSession.open` 缺省改为 `false`）；本机新建的 agent 仍会在创建后自动打开
- 「Connect / Reconnect」在判断是否弹出 token 对话框时，会同时参考输入框与已保存的 token

## 2026-04-09

### Added

- `klaw-webui` 现在依赖新的共享基础 crate `klaw-ui-kit`，复用 `ThemeMode`、`theme_preference()` 与 `NotificationCenter`

### Changed

- 将原来的单文件 `src/web_chat.rs` 拆分为 `app`、`session`、`protocol`、`storage`、`transport` 和 `ui` 模块，明确浏览器聊天 UI 的职责边界
- WASM 启动入口现在会先调用 `klaw-ui-kit::install_fonts()`，与桌面端共享同一套字体装配逻辑，并加载内嵌 LXGW WenKai 字体保证中文可见

## 2026-04-08

### Added

- 初始版本：WASM 聊天 UI（egui）、WebSocket 客户端、`web:` 会话前缀与可选 gateway token query

### Changed

- 收平 crate 内部结构：移除独立 `presentation` 层，改为由 crate 根部复用纯逻辑并由 `web_chat` 直接消费单一连接状态
