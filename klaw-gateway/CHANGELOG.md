# CHANGELOG

## 2026-03-21

### Added

- 新增 `GatewayHandle` 与 `GatewayRuntimeInfo`，支持在 GUI runtime 中受控管理 gateway 生命周期并暴露实际监听信息
- 新增通用 `GatewayWebhookHandler`、webhook 请求/响应类型与 Bearer 鉴权 webhook HTTP 入口

### Changed

- gateway 现在允许 `listen_port = 0` 由系统分配随机端口，并在日志/stdout 中输出实际监听地址

## 2026-03-13

### Added

- `klaw gateway` 启动成功后会向 stdout 打印实际监听的 WebSocket 地址
