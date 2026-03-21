# Gateway WebSocket 设计与实现

本文档记录 `klaw-gateway` 模块的 HTTP 服务设计，覆盖配置模型、服务启动链路、`/ws/chat` 协议行为、webhook 事件输入、错误处理和后续演进方向。

## 目标

- 提供基于 `axum` 的独立 HTTP 服务。
- 暴露 `GET /ws/chat` 端点，承载 WebSocket 聊天。
- 暴露受 Bearer Token 保护的 webhook 事件输入端点。
- 在根配置 `gateway` 下统一管理监听地址和 TLS 配置。
- 以 `session_key` 作为房间隔离键，实现同房间广播、跨房间隔离。

## 代码位置

- 网关实现：`klaw-gateway/src/lib.rs`
- 配置结构：`klaw-config/src/lib.rs`
- 配置校验：`klaw-config/src/validate.rs`
- CLI 启动命令：`klaw-cli/src/commands/gateway.rs`
- CLI 子命令注册：`klaw-cli/src/main.rs`

## 配置模型

网关配置位于根节点 `gateway`：

```toml
[gateway]
enabled = false
listen_ip = "127.0.0.1"
listen_port = 0

[gateway.webhook]
enabled = false
path = "/webhook/events"
token = "replace-me"
env_key = "KLAW_GATEWAY_WEBHOOK_TOKEN"
max_body_bytes = 262144

[gateway.tls]
enabled = false
cert_path = "/path/to/fullchain.pem"
key_path = "/path/to/privkey.pem"
```

字段说明：

- `enabled`：是否启用 gateway 服务，默认 `false`。
- `listen_ip`：监听 IP，默认 `127.0.0.1`。
- `listen_port`：监听端口，默认 `0`，表示由系统分配随机可用端口。
- `webhook.enabled`：是否注册 webhook 路由，默认 `false`。
- `webhook.path`：webhook 路径，默认 `"/webhook/events"`。
- `webhook.token`：固定 Bearer Token，可选。
- `webhook.env_key`：读取 webhook token 的环境变量名，可选。
- `webhook.max_body_bytes`：请求体大小限制，默认 `262144`。
- `tls.enabled`：是否启用 TLS（当前版本仅保留配置结构，尚未启用 TLS 监听实现）。
- `tls.cert_path`：TLS 证书路径（当 `tls.enabled=true` 时必填）。
- `tls.key_path`：TLS 私钥路径（当 `tls.enabled=true` 时必填）。

## 配置校验规则

在 `klaw-config` 校验阶段执行：

- `gateway.listen_ip` 必须能解析为合法 IP。
- `gateway.listen_port` 允许为 `0` 或任意合法 `u16` 端口。
- `gateway.webhook.path` 必须以 `/` 开头且不能为空。
- `gateway.webhook.max_body_bytes` 必须大于 `0`。
- `gateway.webhook.enabled=true` 时，`gateway.webhook.token` 和 `gateway.webhook.env_key` 至少需要配置一个。
- `gateway.tls.enabled=true` 时：
  - `gateway.tls.cert_path` 不能为空字符串。
  - `gateway.tls.key_path` 不能为空字符串。

这保证了 `klaw gateway` 启动前即可发现配置错误。

## 启动链路

- 用户执行 `klaw gateway`。
- `klaw-cli` 先完成通用配置加载与校验（`load_or_init`）。
- `GatewayCommand::run()` 和 `klaw gui` 内嵌 gateway 都会构造带 runtime webhook handler 的 `GatewayOptions`。
- `klaw-gateway` 先对 `listen_ip:listen_port` 执行 `TcpListener::bind`，再从 `local_addr()` 读取实际监听端口。
- 当 `listen_port = 0` 时，日志和运行态快照中都使用实际分配端口。
- 网关创建 `axum::Router`，注册 `/ws/chat` 和可选的 webhook 路由，然后启动服务。

## `/ws/chat` 协议行为

### 握手

- 端点：`GET /ws/chat`
- 必填 query：`session_key`
- 缺少或空 `session_key` 返回 `400 Bad Request`。

示例：

```text
ws://127.0.0.1:18080/ws/chat?session_key=demo-room
```

### 房间模型

- 服务内维护 `HashMap<session_key, broadcast::Sender<String>>`。
- 每个 `session_key` 对应一个 `tokio::broadcast` 总线。
- 新连接订阅对应总线；收到上行消息后向该总线广播。

### 消息处理

- `Text` 帧：原样转为字符串并广播。
- `Binary` 帧：按 UTF-8 lossy 转字符串后广播。
- `Ping/Pong`：忽略业务处理。
- `Close`：结束连接并触发房间清理。

## 连接生命周期与清理

- 每条连接拆分为读写两路：
  - 写任务持续消费广播总线并下发到 WebSocket。
  - 读循环持续读取客户端消息并向房间广播。
- 连接断开后：
  - 终止写任务。
  - 若对应房间订阅数为 0，则从房间表移除，避免长期空房间占用。

## 错误处理语义

`GatewayError` 当前包含：

- `InvalidListenAddress`：监听地址格式非法。
- `TlsNotImplemented`：TLS 配置启用但服务端 TLS 尚未实现。
- `Bind`：端口绑定失败。
- `Serve`：服务运行阶段错误。
- `MissingWebhookToken`：启用 webhook 但没有可用 Bearer Token。
- `MissingWebhookHandler`：启用 webhook 但没有注入处理器。

## Webhook 输入

### 路由与鉴权

- 端点：`POST <gateway.webhook.path>`
- 默认路径：`POST /webhook/events`
- 认证方式：`Authorization: Bearer <token>`
- token 解析顺序：
  - 优先使用 `gateway.webhook.token`
  - 若为空，再读取 `gateway.webhook.env_key` 指向的环境变量

缺少或错误的 Bearer Token 会返回 `401 Unauthorized`。

### 请求体

```json
{
  "source": "github",
  "event_type": "issue_comment.created",
  "content": "PR #42 收到新的 review comment",
  "session_key": "webhook:github:42",
  "chat_id": "repo-42",
  "sender_id": "github:webhook",
  "payload": {"number": 42},
  "metadata": {"repo": "openclaw/klaw"}
}
```

字段规则：

- `source`：必填，外部系统来源标识。
- `event_type`：必填，事件类型。
- `content`：必填，给 agent 的文本摘要。
- `session_key`：可选，不传时自动生成 `webhook:<source>:<uuid>`。
- `chat_id`：可选，默认回退到 `session_key`。
- `sender_id`：可选，默认回退到 `<source>:webhook`。
- `payload`：可选，原始结构化事件体。
- `metadata`：可选，附加元数据。

### 规范化与处理

- 入站 channel 固定为 `webhook`。
- 服务端会自动补充 metadata：
  - `trigger.kind = "webhook"`
  - `webhook.source`
  - `webhook.event_type`
  - `webhook.event_id`
- webhook 请求通过校验后会先落库为 `accepted`。
- runtime 随后异步执行一次 webhook turn。
- 处理成功后状态更新为 `processed`；失败则更新为 `failed`。

### 响应语义

成功受理时返回 `202 Accepted`：

```json
{
  "event_id": "2f4e6f1c-8d8d-4b4f-a45e-2f9a71e84384",
  "status": "accepted",
  "session_key": "webhook:github:42"
}
```

请求体非法时返回 `400 Bad Request`，请求超过 `max_body_bytes` 时返回 `413 Payload Too Large`。

## 当前限制

- TLS 仅有配置模型和校验，暂未接入证书加载与 HTTPS/WSS 监听。
- 房间状态为进程内内存结构，重启后不保留。
- WebSocket 连接仍不包含独立鉴权、限流、房间成员上限等策略。
- webhook 仅支持单一事件入口，不支持 replay、重放映射或多端点模式。
- 不包含跨实例共享房间（当前适用于单实例）。

## 后续演进建议

- 接入 `rustls`，实现 `tls.enabled=true` 的 HTTPS/WSS 监听。
- 增加连接鉴权（例如 token / session 绑定校验）。
- 增加 observability：连接数、房间数、广播失败数指标。
- 对消息大小、发送频率、房间成员数量增加防护阈值。
- 支持跨实例广播后端（如 Redis pub/sub）以支持水平扩展。
- 在 GUI 中增加 webhook 记录实时刷新、重跑和导出能力。
