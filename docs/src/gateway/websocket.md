# Gateway WebSocket 设计与实现

本文档记录 `klaw-gateway` 模块的 WebSocket HTTP 服务设计，覆盖配置模型、服务启动链路、`/ws/chat` 协议行为、错误处理和后续演进方向。

## 目标

- 提供基于 `axum` 的独立 HTTP 服务。
- 暴露 `GET /ws/chat` 端点，承载 WebSocket 聊天。
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
listen_ip = "127.0.0.1"
listen_port = 8080

[gateway.tls]
enabled = false
cert_path = "/path/to/fullchain.pem"
key_path = "/path/to/privkey.pem"
```

字段说明：

- `listen_ip`：监听 IP，默认 `127.0.0.1`。
- `listen_port`：监听端口，默认 `8080`。
- `tls.enabled`：是否启用 TLS（当前版本仅保留配置结构，尚未启用 TLS 监听实现）。
- `tls.cert_path`：TLS 证书路径（当 `tls.enabled=true` 时必填）。
- `tls.key_path`：TLS 私钥路径（当 `tls.enabled=true` 时必填）。

## 配置校验规则

在 `klaw-config` 校验阶段执行：

- `gateway.listen_ip` 必须能解析为合法 IP。
- `gateway.listen_port` 必须大于 0。
- `gateway.tls.enabled=true` 时：
  - `gateway.tls.cert_path` 不能为空字符串。
  - `gateway.tls.key_path` 不能为空字符串。

这保证了 `klaw gateway` 启动前即可发现配置错误。

## 启动链路

- 用户执行 `klaw gateway`。
- `klaw-cli` 先完成通用配置加载与校验（`load_or_init`）。
- `GatewayCommand::run()` 调用 `klaw_gateway::run_gateway(&config.gateway)`。
- 网关创建 `axum::Router` 并监听 `listen_ip:listen_port`。

## `/ws/chat` 协议行为

### 握手

- 端点：`GET /ws/chat`
- 必填 query：`session_key`
- 缺少或空 `session_key` 返回 `400 Bad Request`。

示例：

```text
ws://127.0.0.1:8080/ws/chat?session_key=demo-room
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

## 当前限制

- TLS 仅有配置模型和校验，暂未接入证书加载与 HTTPS/WSS 监听。
- 房间状态为进程内内存结构，重启后不保留。
- 不包含鉴权、限流、消息大小限制、房间成员上限等策略。
- 不包含跨实例共享房间（当前适用于单实例）。

## 后续演进建议

- 接入 `rustls`，实现 `tls.enabled=true` 的 HTTPS/WSS 监听。
- 增加连接鉴权（例如 token / session 绑定校验）。
- 增加 observability：连接数、房间数、广播失败数指标。
- 对消息大小、发送频率、房间成员数量增加防护阈值。
- 支持跨实例广播后端（如 Redis pub/sub）以支持水平扩展。
