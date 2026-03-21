# klaw-gateway

`klaw-gateway` 提供基于 `axum` 的 WebSocket 网关服务，负责：

- 绑定配置中的监听地址和端口
- 支持 `listen_port = 0` 时由系统分配随机可用端口
- 暴露 `/ws/chat` WebSocket 入口
- 可选暴露受 `Authorization: Bearer <token>` 保护的 webhook 事件入口
- 按 `session_key` 维护房间广播通道
- 在启动成功后打印实际可连接的 WebSocket 地址
- 提供可管理的 `GatewayHandle` / `GatewayRuntimeInfo`，以及可注入业务逻辑的 `GatewayWebhookHandler`

## Runtime Notes

- 当前仅支持非 TLS 监听
- 启动成功后会输出实际监听地址对应的 `http://<listen_addr>/ws/chat`
- webhook 路由是否注册由 `gateway.webhook.enabled` 决定，Bearer token 来自 `gateway.webhook.token` 或 `gateway.webhook.env_key`

## Examples

- `examples/webhook_request.rs`: 使用 Rust 和 `reqwest` 向 gateway 的 webhook 端点发送一条测试事件

```bash
cargo run -p klaw-gateway --example webhook_request
WEBHOOK_TOKEN=replace-me BASE_URL=http://127.0.0.1:18080 cargo run -p klaw-gateway --example webhook_request
```
