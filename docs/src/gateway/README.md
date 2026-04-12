# 网关模块

`klaw-gateway` 提供基于 `axum` 的独立 HTTP 服务，暴露 WebSocket 聊天端点和可选的 webhook 事件输入端点。

## 功能特性

- 基于 `axum` 的轻量 HTTP 服务
- `GET /ws/chat` WebSocket 端点
- `POST /webhook/events` 风格的 webhook 事件输入端点
- 基于 `session_key` 的会话订阅
- `method/result/error/event` 结构化帧协议
- 连接生命周期管理
- Tailscale Serve/Funnel 支持

## 配置

```toml
[gateway]
enabled = false
listen_ip = "127.0.0.1"
listen_port = 0

[gateway.auth]
enabled = false
token = "your-secret-token"
env_key = "KLAW_GATEWAY_TOKEN"

[gateway.tailscale]
mode = "off"              # off | serve | funnel
reset_on_exit = true

[gateway.webhook]
enabled = false
path = "/webhook/events"
max_body_bytes = 262144

[gateway.tls]
enabled = false
cert_path = "/path/to/fullchain.pem"
key_path = "/path/to/privkey.pem"
```

### 配置说明

- `enabled = true` 时，`klaw gui` 启动会自动拉起内置 gateway
- `listen_port = 0` 时由系统分配随机可用端口，实际端口会输出到日志并展示在 GUI Gateway 面板
- `gateway.auth.enabled = true` 时，`/ws/chat` 需要 `Authorization: Bearer <token>`，浏览器 WebSocket 也可用 `?token=` 传递
- `gateway.tailscale.mode` 可将 gateway 暴露到 Tailscale 私有网络或公网
- `gateway.webhook.enabled = true` 时会注册 webhook HTTP 路由

### 配置校验

- `listen_ip` 必须能解析为合法 IP
- `listen_port` 允许为 `0`（随机端口）或任意合法 `u16` 端口
- `gateway.webhook.path` 必须以 `/` 开头
- `gateway.webhook.max_body_bytes` 必须大于 `0`
- `tls.enabled=true` 时，`cert_path` 和 `key_path` 不能为空
- `gateway.tailscale.mode = "funnel"` 时，必须配置 `gateway.auth`

## 启动

```bash
klaw gateway
```

连接示例：

如果配置了随机端口，请使用启动日志或 GUI `Gateway` 面板显示的实际地址进行连接，例如：

```text
ws://127.0.0.1:18080/ws/chat
```

Webhook 示例：

```bash
curl -X POST http://127.0.0.1:18080/webhook/events \
  -H 'Authorization: Bearer your-secret-token' \
  -H 'Content-Type: application/json' \
  -d '{
    "source": "github",
    "event_type": "issue_comment.created",
    "content": "PR #42 收到新的 review comment",
    "payload": {"number": 42}
  }'
```

> **注意**: Webhook 复用 `gateway.auth` 的 token 配置。若 `gateway.auth.enabled = false`，则 webhook 无需鉴权。

Webhook 请求在鉴权和参数校验通过后会立即返回 `202 Accepted`，随后由 runtime 异步处理，并把请求、状态和结果摘要落库供 GUI `Webhook` 面板查看。

## WebSocket 协议

- 连接建立后服务端先下发 `event`：`session.connected`
- 客户端通过 `method` 调用：
  - `session.subscribe`
  - `session.unsubscribe`
  - `session.ping`
  - `session.submit`
- 服务端返回：
  - `result`：method 成功结果
  - `error`：结构化错误
  - `event`：连接态、消息快照、流式事件

示例：

```json
{"type":"method","id":"sub-1","method":"session.subscribe","params":{"session_key":"websocket:demo"}}
{"type":"method","id":"req-1","method":"session.submit","params":{"input":"hello","stream":true}}
```

## 连接生命周期

- 每条连接维护独立的连接上下文与当前订阅的 `session_key`
- `session.subscribe` 会更新当前连接的会话路由
- `session.submit` 会把输入映射为 runtime `ChannelRequest`
- 连接断开后，进程内连接注册表会立即清理

## 当前限制

- TLS 仅有配置模型，尚未实现 HTTPS/WSS 监听
- 连接状态为进程内内存结构，重启后不保留
- 当前 streaming 以 runtime snapshot 事件为单位，不是 token 级别推送

## 后续演进

- 接入 `rustls` 实现 WSS
- 增加 observability 指标
- 增加消息大小、发送频率限制
- 支持跨实例广播后端（如 Redis pub/sub）
- 为 webhook 增加 replay / 重跑等运维能力

详细文档：
- [WebSocket 协议](./websocket.md)
- [Tailscale 集成](./tailscale.md)
