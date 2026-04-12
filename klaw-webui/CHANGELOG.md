# CHANGELOG

## 2026-04-12

### Added

- 新增文件上传功能：在 agent 对话框中，原 "Enter to send" 提示位置改为文件选择按钮（回形针图标）
- 点击文件按钮后触发浏览器文件选择器，选择文件后自动上传到 gateway 的 `/archive/upload` 接口
- 上传完成后通过 notification 组件通知用户，并在输入框右侧显示"File attached"状态
- 发送消息时，如果有已上传的文件，会在 `session.submit` websocket 消息中包含 `archive_id` 字段
- 支持在发送前移除已附加的文件（点击 ✕ 按钮）
- 上传过程中显示 spinner 和"Uploading..."状态提示

### Changed

- 移除了输入框下方的 "Enter to send, Cmd+Enter for newline" 提示文本
- `SessionWindow` 中的 `selected_archive_id` 和 `uploading_file` 字段改为 `Rc<RefCell<>>` 以支持异步上传任务更新状态
- `session.submit` 方法现在支持可选的 `archive_id` 参数，发送后自动清除已附加的文件
- agent 窗口现在维护独立的历史初始化状态：未打开窗口不预取历史，首次打开时异步调用 `session.subscribe`，并在历史尚未返回完成前显示加载 spinner

## 2026-04-10

### Added

- `klaw-webui` 的浏览器业务状态现在会持久化 gateway token，刷新页面后仍可恢复
- 工作区 bootstrap 完成后，对所有 **已打开** 的 agent 窗口调用 `session.subscribe`，各自拉取历史（不再仅订阅服务端 `active_session_key`）
- 连接前将输入框中的 token 写回 `gateway_token` 并 `localStorage` 落盘，避免只在输入框里填了 token、未点「Save & Reconnect」时刷新丢失

### Changed

- `gateway_token` 的恢复策略改为"URL query 优先，其次使用 localStorage 中的上次保存值"，便于显式覆盖浏览器内已缓存的 token
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
