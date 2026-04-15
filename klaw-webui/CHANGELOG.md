# CHANGELOG

## 2026-04-15

### Fixed

- webui 现在会忽略带 heartbeat metadata 且内容等于 `silent_ack_token` 的历史/实时 assistant 消息，避免 `HEARTBEAT_OK` 之类的静默确认文本出现在会话窗口
- websocket 新建会话在 runtime 侧同步 heartbeat 后，webui 创建的新 session 也会立即具备默认 heartbeat 绑定

## 2026-04-14

### Fixed

- `approval` 与 `question_single_select` 交互卡片现在会按亮色/暗色主题分别使用对应调色板，暗色模式下不再沿用浅色背景；命令预览区也改为跟随卡片主题的高对比代码块样式

### Added

- webui 对话消息现在支持解析并渲染 `im.card` 交互卡片，首批覆盖 `approval` 与 `question_single_select`（`ask_question`）两类 websocket channel 卡片

### Changed

- agent 输入区上传控件现已改为 `Upload` 图标+文字按钮，并新增 `File` 按钮与已上传文件弹窗；弹窗使用表格展示文件名、archive id 和大小，支持右键预览或仅从当前页面移除
- webui 会话附件状态已从单个 `archive_id` 扩展为多文件列表，支持连续上传多个不同文件；原右侧 `File attached` 状态文案已移除
- `session.submit` 现会把当前待发送文件作为结构化 `attachments` 数组随 websocket 请求一起发出，同时保留首个 `archive_id` 兼容字段
- 卡片动作现在通过 `session.submit` 回传结构化 `webui.card.*` metadata，并在浏览器端就地展示 pending / completed / failed 交互状态
- 历史消息恢复时会优先读取持久化的消息 metadata 还原卡片；对 `/approve`、`/reject`、`/card_answer` 这类内部卡片命令，UI 会隐藏原始 slash 文本并回填卡片完成态
- websocket 历史与实时消息的前端模型已从纯文本扩展为“文本 + metadata + 派生卡片”，为后续更多 IM 卡片类型留出协议位
- 多个已打开 agent 窗口现在可以通过同一条 `/ws/chat` 连接持续接收各自会话的实时消息；后台推送不再被最后一次 `session.subscribe` 或最后激活窗口错误覆盖
- agent 窗口历史消息现在只首屏加载最新 10 条，并在滚动到顶部时通过 `session.history.load` 自动按 `message_id` 游标继续向前分页

## 2026-04-13

### Added

- 顶部菜单栏新增 `Help` 菜单，并提供 `About` 弹窗；弹窗复用连接页同一张 `crab.png` 网络图片，显示当前版本和仓库链接
- agent 对话输入框新增 slash command 自动补全面板；输入 `/` 后会根据当前 token 过滤 `/new`、`/help`、`/model_provider`、`/model`、`/approve` 等 runtime 命令，并支持点击或 `Tab` / `Enter` 插入

### Fixed

- 修正 webui 底部状态栏右侧 `FPS` 显示，改为基于当前 egui 帧间隔计算实时帧率，而不再错误读取可用矩形尺寸
- 调整 agent 对话输入框的 slash command 补全面板：恢复更接近原布局的输入区高度，面板改为锚定到输入光标附近，并在接受命令后按当前 slash token 立即关闭而不是停留到下一次输入

### Changed

- 底部状态栏的主题模式入口已改为复用 `klaw-ui-kit::ThemeSwitch` 三态滑块，浏览器端不再使用普通下拉框切换 system/light/dark
- 底部状态栏新增 `Open` 计数，以及当前 agent 的路由、消息数和即时活动状态，便于排查 websocket 会话和 streaming/upload 运行情况

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
- webui 的 websocket 会话键前缀已从 `web:` 统一切换为 `websocket:`，并与 channel 类型命名保持一致

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

- 初始版本：WASM 聊天 UI（egui）、WebSocket 客户端、`websocket:` 会话前缀与可选 gateway token query

### Changed

- 收平 crate 内部结构：移除独立 `presentation` 层，改为由 crate 根部复用纯逻辑并由 `web_chat` 直接消费单一连接状态
# 2026-04-14

## Fixed

- webui 文件上传现在优先监听浏览器 `input[type=file]` 的 `change` 事件，并把取消选择的回焦 grace time 放宽到 1 秒，减少“已选中文件但没有触发上传”的竞态
- webui agent 历史分页现在要求 `oldest_loaded_message_id` 真正推进才继续向前拉取；当服务端返回缺失或未推进的游标时，前端会停止重复请求，避免顶部滚动时同一条历史消息反复刷新
