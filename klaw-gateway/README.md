# klaw-gateway

`klaw-gateway` 提供基于 `axum` 的 WebSocket 网关服务，负责：

- 绑定配置中的监听地址和端口
- 支持 `listen_port = 0` 时由系统分配随机可用端口
- 暴露 `/ws/chat` WebSocket 入口
- 可选暴露受 `Authorization: Bearer <token>` 保护的 `events` / `agents` 双 webhook 入口
- 按 `session_key` 维护房间广播通道
- 在启动成功后打印实际可连接的 WebSocket 地址
- 提供可管理的 `GatewayHandle` / `GatewayRuntimeInfo`，以及可注入业务逻辑的 `GatewayWebhookHandler`

## Module Layout

- `lib.rs`: 仅保留模块声明与公开 API re-export
- `runtime.rs`: gateway 启动、监听、路由装配与生命周期入口
- `state.rs`: 运行态共享状态、`GatewayHandle` 与 `GatewayRuntimeInfo`
- `websocket.rs`: `/ws/chat` WebSocket 连接与房间广播逻辑
- `webhook.rs`: webhook 鉴权、`events` / `agents` payload 归一化与 handler 集成
- `handlers.rs`: health / metrics HTTP handlers
- `error.rs`: `GatewayError`

## Runtime Notes

- 当前仅支持非 TLS 监听
- 启动成功后会输出实际监听地址对应的 `http://<listen_addr>/ws/chat`
- webhook 路由是否注册由 `gateway.webhook.enabled` 决定；`events` / `agents` 可分别启停并配置独立 path 与 body limit
- `TailscaleManager::inspect_host()` 可独立读取本机 Tailscale 状态，供 GUI 在 gateway 未运行时展示主机连接信息
- Tailscale Serve/Funnel 会在 gateway 绑定完成后使用实际监听端口做反向代理，并在 setup 后回读 `tailscale serve status --json` / `tailscale funnel status --json` 确认配置是否生效

## Examples

- `examples/webhook_request.rs`: 使用 Rust 和 `reqwest` 向 `events` webhook 端点发送一条测试事件
- `examples/webhook_agents_request.rs`: 使用 Rust 和 `reqwest` 向 `agents` webhook 端点发送 query + raw JSON body 请求

```bash
cargo run -p klaw-gateway --example webhook_request
WEBHOOK_TOKEN=replace-me BASE_URL=http://127.0.0.1:18080 cargo run -p klaw-gateway --example webhook_request

cargo run -p klaw-gateway --example webhook_agents_request
WEBHOOK_TOKEN=replace-me BASE_URL=http://127.0.0.1:18080 cargo run -p klaw-gateway --example webhook_agents_request
```
