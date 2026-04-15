# CHANGELOG

## 2026-04-15

### Fixed

- GUI gateway 面板中的 `tailscale Host Status` 现在会把 `tailscale status --json` 的 daemon/CLI 执行失败显示为明确的 `Error`，不再把 `failed to connect to local Tailscaled process` 一类错误误报成 `Disconnected`

### Added

- `tailscale` 主机探测现在会输出 debug 日志，记录 `tailscale version` / `tailscale status --json` 的执行结果、退出码与 stdout/stderr 摘要，便于排查 GUI 中的主机状态误判

### Changed

- `tailscale version` / `tailscale status --json` 的 GUI 主机探测超时从 `400ms` 放宽到 `30s`，避免本机 Tailscale CLI 偶发稍慢时被过早判成超时

## 2026-04-14

### Fixed

- `TailscaleHostInfo` 主机探测现在对 `tailscale version` / `tailscale status --json` 使用短超时；当本机 Tailscale daemon 未启动或无响应时，GUI 会在 `tailscale Host Status` 中显示不可用，而不会把整个 gateway 状态刷新拖成超时错误
- gateway 在启用 `tailscale.mode = "serve" | "funnel"` 时，即使 Tailscale 配置失败也会继续启动本地 HTTP/WebSocket 服务；失败信息仅记录在 Tailscale 运行态摘要里，不再阻断 gateway server 本身

### Changed

- `/ws/chat` 的 `session.submit` 现在支持结构化 `attachments` 数组；gateway 会兼容旧的单 `archive_id` 请求，并把附件信息原样转交 runtime handler
- websocket `session.subscribe` 历史消息现在会把持久化的 `response.metadata` 一并回传，不再只包含纯文本 `content`
- `/ws/chat` 协议的实时与历史消息现在统一支持结构化 `response.metadata`，便于 webui 恢复并渲染 IM 卡片
- `/ws/chat` 连接现在可同时保留多个会话订阅；重复调用 `session.subscribe` 不再覆盖旧订阅，实时消息会按所属 `session_key` 广播到同一浏览器连接中的对应 agent 窗口
- `/ws/chat` 已将实时订阅与历史加载拆分：`session.subscribe` 仅负责订阅实时消息，新增 `session.history.load` 以 `message_id` 游标按页返回历史记录

## 2026-04-13

### Added

- 新增共享 websocket broadcaster；gateway 现在会把每条 `/ws/chat` 连接的 sender 和订阅 session 一起登记，供 runtime 后台任务按 session 回推消息

### Changed

- `GatewayOptions` / `GatewayState` 现在支持注入并复用共享 websocket broadcaster，而不再只在连接处理协程里临时持有 frame sender

## 2026-04-12

### Added

- 新增 archive 文件上传下载 HTTP 接口，支持 Bearer 鉴权：
  - `POST /archive/upload`: multipart 文件上传
  - `GET /archive/download/:id`: 文件下载
  - `GET /archive/list`: 查询文件列表（支持 session_key、chat_id、source_kind、media_kind、filename 过滤）
  - `GET /archive/:id`: 获取文件元数据
- 新增 model providers 列表 HTTP 接口，支持 Bearer 鉴权：
  - `GET /providers/list`: 获取所有配置的 model providers 信息
- `GatewayOptions` 新增 `archive_service` 和 `app_config` 字段
- `GatewayState` 新增 `archive` 和 `providers` 字段
- archive 路由在提供 `archive_service` 时自动注册，默认 body limit 为 100MB
- providers 路由在提供 `app_config` 时自动注册

### Changed

- gateway Bearer 鉴权中间件现在保护 `/ws/chat`、所有 `/archive/*` 和 `/providers/*` 路由
- `should_require_gateway_auth` 函数现在检查 archive 和 providers 相关路径
- `Route` 枚举新增 `ArchiveUpload`、`ArchiveDownload`、`ArchiveList`、`ArchiveGet` 和 `ProvidersList` 变体
- `session.subscribe` 现在会在历史消息流发送完后额外发出 `session.history.done` 事件，便于浏览器 chat UI 在异步加载历史时准确结束 loading 状态
- websocket chat 会话键前缀已统一改为 `websocket:`，与 channel 类型 `websocket` 保持一致

## 2026-04-10

### Fixed

- `/ws/chat` 的 `session.submit` 现在改为后台逐帧推送 handler 产出的 websocket frame，不再等整轮响应结束后一次性发送，修复浏览器 chat UI 在 provider 开启 stream 时仍只看到整段结果的问题

### Changed

- gateway 现在改用 `rust-embed` 从 `static/` 与 `assets/` 目录打包首页、`/chat` 和 logo 资源；聊天 WASM/JS 路径同步从 `/chat/pkg/*` 切换为 `/chat/dist/*`
- 首页介绍区现改为风格化的 `/chat` 主按钮，作为浏览器聊天入口而不再展示三枚装饰性标签

## 2026-04-08

### Added

- 新增 `GET /chat` 与 `/chat/pkg/*` 内嵌 `klaw-webui`（egui WASM）聊天界面；`/ws/chat` 在启用 `gateway.auth` 时除 `Authorization: Bearer` 外支持 `token` / `access_token` query，便于浏览器 WebSocket 鉴权

### Changed

- `klaw-gateway/static/chat/pkg/`（wasm-bindgen 生成的 `.js` / `.wasm`）改为 `.gitignore`，构建前需本地生成后再编译 gateway
- `/chat` 内嵌资源响应现收敛为共享 helper，保留原有路径与缓存行为但减少重复代码；README 也改为统一指向根目录 `make webui-wasm`

## 2026-03-29

### Added

- gateway 根路径 `/` 现在会返回卡通风格的默认单页首页，并内置 `logo.webp` logo 静态资源
- webhook 请求校验现抽象为可扩展 validator 链，支持 `Authorization: Bearer`、GitHub `X-Hub-Signature-256` / `X-Hub-Signature`、以及 GitLab `X-Gitlab-Token` / `X-Gitlab-Signature`
- `/webhook/events` 与 `/webhook/agents` 现在会输出入站 debug 日志，记录 endpoint、request id、session、body 大小与命中的鉴权模式，不暴露明文 secret

### Changed

- 首页内置 logo 资源现改为 `512x512` 的 `logo.webp`，显著减小静态资源和内嵌二进制体积
- `/webhook/events` 与 `/webhook/agents` 路径现固定为内置常量，不再从配置读取 path
- webhook `events` / `agents` 现在都会生成独立的 `webhook:*` 执行 session；`base_session_key` 仅用于回复投递路由，旧 `session_key` 字段暂作为兼容别名保留
- gateway Bearer 鉴权中间件现仅保护 `/ws/chat`；webhook 路由继续使用独立的 webhook 校验链；首页、health、metrics 不参与鉴权
- `gateway.auth.enabled = false` 时，webhook 入口现在直接放行，不再要求任何 header 校验
- Tailscale Funnel 不再强制要求 `gateway.auth` 已配置；若未配置认证，gateway/GUI 仅保留公网暴露警告

## 2026-03-26

### Fixed

- Tailscale runtime checks and serve/funnel CLI invocations now reuse the shared augmented PATH so GUI/macOS launches can still discover a Homebrew-installed `tailscale` binary

## 2026-03-25

### Added

- 新增 `TailscaleHostInfo` 主机状态快照，支持在不启动 gateway 的情况下读取本机 Tailscale CLI / 登录 / backend / DNS 信息

- 新增 `POST /webhook/agents` 入口，请求可携带 `hook_id`、短 `session_key`、可选 `provider` / `model` 与任意 JSON `body`
- 新增 webhook agent 请求/响应类型与归一化逻辑，并为 handler 提供可返回 HTTP 状态码的错误类型

### Changed

- `/webhook/agents` 现改为通过 URL query 接收 `hook_id` / `session_key` / `provider` / `model` 等控制参数，HTTP body 保持为原始 JSON 内容
- gateway webhook 配置与路由注册现支持 `events` / `agents` 双 endpoint，各自拥有独立 path 与 body limit

### Fixed

- Tailscale Funnel 现改为使用新版 `tailscale funnel --bg <target>` / `tailscale funnel reset` CLI 语法，并在 setup 后通过 `tailscale funnel status --json` 回读确认配置是否真正生效
- gateway 在 `listen_port = 0` 时，Tailscale Serve/Funnel 现在会绑定实际监听端口而不是配置端口 `0`

## 2026-03-21

### Added

- 新增 `GatewayHandle` 与 `GatewayRuntimeInfo`，支持在 GUI runtime 中受控管理 gateway 生命周期并暴露实际监听信息
- 新增通用 `GatewayWebhookHandler`、webhook 请求/响应类型与 Bearer 鉴权 webhook HTTP 入口
- 新增 `examples/webhook_request.rs`，用于向 webhook 端点发送测试事件请求

### Changed

- gateway 现在允许 `listen_port = 0` 由系统分配随机端口，并在日志/stdout 中输出实际监听地址
- `klaw-gateway/src/lib.rs` 已按职责拆分为 `runtime/state/websocket/webhook/handlers/error` 模块，`lib.rs` 仅保留 API re-export，便于后续扩展 TLS、更多 HTTP 入口和单元测试

## 2026-03-13

### Added

- `klaw gateway` 启动成功后会向 stdout 打印实际监听的 WebSocket 地址
