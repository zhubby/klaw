# 网关模块

`klaw-gateway` 提供基于 `axum` 的独立 HTTP 服务，暴露 WebSocket 聊天端点。

## 功能特性

- 基于 `axum` 的轻量 HTTP 服务
- `GET /ws/chat` WebSocket 端点
- 基于 `session_key` 的房间隔离
- 同房间广播、跨房间隔离
- 连接生命周期管理

## 配置

```toml
[gateway]
enabled = false
listen_ip = "127.0.0.1"
listen_port = 0

[gateway.tls]
enabled = false
cert_path = "/path/to/fullchain.pem"
key_path = "/path/to/privkey.pem"
```

### 配置说明

- `enabled = true` 时，`klaw gui` 启动会自动拉起内置 gateway
- `listen_port = 0` 时由系统分配随机可用端口，实际端口会输出到日志并展示在 GUI Gateway 面板

### 配置校验

- `listen_ip` 必须能解析为合法 IP
- `listen_port` 允许为 `0`（随机端口）或任意合法 `u16` 端口
- `tls.enabled=true` 时，`cert_path` 和 `key_path` 不能为空

## 启动

```bash
klaw gateway
```

连接示例：

如果配置了随机端口，请使用启动日志或 GUI `Gateway` 面板显示的实际地址进行连接，例如：

```text
ws://127.0.0.1:18080/ws/chat?session_key=demo-room
```

## 房间模型

- 服务维护 `HashMap<session_key, broadcast::Sender<String>>`
- 每个 `session_key` 对应一个 `tokio::broadcast` 总线
- 新连接订阅对应总线，收到上行消息后向该总线广播

## 消息处理

| 帧类型 | 处理方式 |
|--------|----------|
| `Text` | 原样转为字符串并广播 |
| `Binary` | 按 UTF-8 lossy 转字符串后广播 |
| `Ping/Pong` | 忽略业务处理 |
| `Close` | 结束连接并触发房间清理 |

## 连接生命周期

- 每条连接拆分为读写两路任务
- 写任务持续消费广播总线并下发到 WebSocket
- 读循环持续读取客户端消息并向房间广播
- 连接断开后，若房间订阅数为 0 则移除

## 当前限制

- TLS 仅有配置模型，尚未实现 HTTPS/WSS 监听
- 房间状态为进程内内存结构，重启后不保留
- 无鉴权、限流、消息大小限制
- 无跨实例共享房间（单实例适用）

## 后续演进

- 接入 `rustls` 实现 WSS
- 增加连接鉴权（token / session 绑定）
- 增加 observability 指标
- 增加消息大小、发送频率限制
- 支持跨实例广播后端（如 Redis pub/sub）

详细文档：
- [WebSocket 协议](./websocket.md)
