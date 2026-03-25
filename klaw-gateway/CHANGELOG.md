# CHANGELOG

## 2026-03-25

### Added

- 新增 `POST /webhook/agents` 入口，请求可携带 `hook_id`、短 `session_key`、可选 `provider` / `model` 与任意 JSON `body`
- 新增 webhook agent 请求/响应类型与归一化逻辑，并为 handler 提供可返回 HTTP 状态码的错误类型

### Changed

- gateway webhook 配置与路由注册现支持 `events` / `agents` 双 endpoint，各自拥有独立 path 与 body limit

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
