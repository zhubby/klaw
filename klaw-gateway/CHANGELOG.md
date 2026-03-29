# CHANGELOG

## 2026-03-29

### Added

- gateway 根路径 `/` 现在会返回卡通风格的默认单页首页，并内置 `/assets/logo.webp` logo 静态资源
- webhook 请求校验现抽象为可扩展 validator 链，支持 `Authorization: Bearer`、GitHub `X-Hub-Signature-256` / `X-Hub-Signature`、以及 GitLab `X-Gitlab-Token` / `X-Gitlab-Signature`
- `/webhook/events` 与 `/webhook/agents` 现在会输出入站 debug 日志，记录 endpoint、request id、session、body 大小与命中的鉴权模式，不暴露明文 secret

### Changed

- 首页内置 logo 资源现改为 `512x512` 的 `/assets/logo.webp`，显著减小静态资源和内嵌二进制体积
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
