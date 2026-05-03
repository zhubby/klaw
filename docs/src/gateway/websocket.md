# Gateway WebSocket v1 协议

本文档是 `klaw-gateway` 当前 `/ws/chat` WebSocket 协议的完整说明。Gateway WebSocket v1 是面向 WebUI、桌面端、移动端和第三方客户端的长期 agent 交互协议底座，使用 JSON-RPC 语义的轻量 envelope，覆盖初始化、会话、历史、turn/item 生命周期、结构化内容、反向请求、错误、背压、安全和 schema 管理。

`klaw-webui` 已直接切换到 v1，不再发送旧版 `type: "method"` 帧，也不把旧版 `type: "event" | "result" | "error"` 服务端帧作为正常输入路径。

## 代码位置

- Gateway WebSocket 实现：`klaw-gateway/src/websocket.rs`
- v1 协议类型与 schema：`klaw-gateway/src/protocol.rs`
- Gateway 状态与路由：`klaw-gateway/src/lib.rs`
- Runtime WebSocket 桥接：`klaw-runtime/src/gateway_websocket.rs`
- WebUI v1 客户端：`klaw-webui/src/web_chat/protocol.rs`、`klaw-webui/src/web_chat/transport.rs`
- Gateway 配置结构：`klaw-config/src/lib.rs`

## 配置与启动

Gateway 配置位于根节点 `gateway`：

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

启动链路：

- `klaw gateway` 或 GUI 内嵌 gateway 加载并校验配置。
- Gateway 绑定 `listen_ip:listen_port`；当 `listen_port = 0` 时由系统分配端口。
- 服务注册 `/ws/chat`、可选 webhook、archive 和 provider HTTP 路由。
- 远程暴露时应启用 gateway auth；`/ws/chat` 支持 Bearer 鉴权，也保留 query token 兼容浏览器限制。

## 端点与握手

- 端点：`GET /ws/chat`
- 推荐鉴权：`Authorization: Bearer <token>`
- 兼容鉴权：`?token=<token>` 或 `?access_token=<token>`
- 兼容 query：`session_key`，仅用于旧连接方式；v1 客户端应通过 `session/subscribe` 显式订阅。

示例：

```text
ws://127.0.0.1:18080/ws/chat?token=secret
```

连接建立后，v1 客户端发送 `initialize` 完成协议初始化和能力协商。

## Envelope

v1 使用 JSON-RPC 2.0 语义，但线上帧省略 `jsonrpc` 字段。每个 WebSocket 文本帧承载一个 JSON 消息：

```json
{ "id": "req_1", "method": "turn/start", "params": {} }
{ "id": "req_1", "result": {} }
{ "method": "item/started", "params": {} }
{ "id": "srv_req_1", "method": "approval/request", "params": {} }
{ "id": "req_2", "error": { "code": "invalid_params", "message": "..." } }
```

规则：

- 客户端请求必须包含 `id`、`method` 和可选 `params`。
- 成功响应必须 echo 同一个 `id`，并包含 `result`。
- 错误响应必须 echo 同一个 `id`（无法解析请求时可为 `null`），并包含 `error`。
- 服务端通知没有 `id`，只包含 `method` 和 `params`。
- 服务端反向请求包含 `id`，客户端通过 `approval/respond`、`tool/respond` 或 `user_input/respond` 闭环。

## 初始化

客户端连接后发送：

```json
{
  "id": "init_1",
  "method": "initialize",
  "params": {
    "client_info": {
      "name": "klaw-webui",
      "title": "Klaw WebUI",
      "version": "0.15.6"
    },
    "capabilities": {
      "protocol_version": "v1",
      "turns": true,
      "items": true,
      "tools": true,
      "approvals": true,
      "server_requests": true,
      "cancellation": true,
      "schema": true
    }
  }
}
```

服务端响应包含：

- `protocol_version`
- `protocol_name`
- `connection_id`
- 协商后的 `capabilities`
- `server_info`

客户端随后可发送 `initialized` 通知：

```json
{ "method": "initialized", "params": {} }
```

实验字段必须通过 `capabilities.experimental = true` 显式启用。未协商的能力不能作为稳定协议依赖。

## 身份模型

- `connection_id`：连接级诊断和路由 ID，不作为权限边界。
- `session_id`：Klaw 工作区会话；当前兼容旧 `session_key`。
- `thread_id`：agent 对话上下文；当前 WebUI 通常与 `session_id` 相同，但协议不要求永久绑定。
- `turn_id`：一次用户请求及其后续 agent 工作。
- `item_id`：turn 内的一个工作单元，例如 assistant message、reasoning、tool call 或 file change。
- `request_id`：RPC 请求响应匹配 ID，不能替代 `turn_id` 或 `item_id`。

## 方法总览

当前 v1 稳定方法：

| 方法 | 方向 | 描述 |
|------|------|------|
| `initialize` | Client -> Server | 初始化协议与能力协商 |
| `initialized` | Client -> Server | 客户端初始化完成通知 |
| `session/list` | Client -> Server | 获取工作区会话列表 |
| `session/create` | Client -> Server | 创建新会话 |
| `session/update` | Client -> Server | 更新会话标题等信息 |
| `session/delete` | Client -> Server | 删除会话 |
| `session/subscribe` | Client -> Server | 订阅会话实时事件 |
| `session/unsubscribe` | Client -> Server | 取消当前连接的会话订阅 |
| `provider/list` | Client -> Server | 获取模型提供商列表 |
| `thread/history` | Client -> Server | 按游标分页读取会话历史 |
| `turn/start` | Client -> Server | 创建一次用户 turn |
| `turn/cancel` | Client -> Server | 中断一个 turn |
| `approval/respond` | Client -> Server | 响应审批反向请求 |
| `tool/respond` | Client -> Server | 响应客户端工具反向请求 |
| `user_input/respond` | Client -> Server | 响应补充用户输入反向请求 |

预留但尚未完整实现的方法包括 `thread/start`、`thread/resume`、`thread/read`、`thread/list`、`thread/rollback`、`turn/steer` 和 `turn/read`。

## 会话、Provider 与历史

WebUI 启动后通常按以下顺序加载工作区：

```json
{ "id": "sessions_1", "method": "session/list", "params": {} }
{ "id": "providers_1", "method": "provider/list", "params": {} }
```

`session/list` 响应：

```json
{
  "id": "sessions_1",
  "result": {
    "sessions": [
      {
        "session_key": "websocket:abc",
        "title": "Agent abc",
        "created_at_ms": 1714200000000,
        "model_provider": "anthropic",
        "model": "claude-sonnet-4-5"
      }
    ],
    "active_session_key": "websocket:abc"
  }
}
```

`provider/list` 响应：

```json
{
  "id": "providers_1",
  "result": {
    "default_provider": "anthropic",
    "providers": [
      { "id": "anthropic", "default_model": "claude-sonnet-4-5" }
    ]
  }
}
```

会话操作：

```json
{ "id": "create_1", "method": "session/create", "params": {} }
{
  "id": "rename_1",
  "method": "session/update",
  "params": { "session_key": "websocket:abc", "title": "New title" }
}
{
  "id": "delete_1",
  "method": "session/delete",
  "params": { "session_key": "websocket:abc" }
}
{
  "id": "subscribe_1",
  "method": "session/subscribe",
  "params": { "session_key": "websocket:abc" }
}
```

订阅成功后，服务端返回 success envelope，并发送 `session/subscribed` 通知。取消订阅同理发送 `session/unsubscribed`。

历史分页：

```json
{
  "id": "history_1",
  "method": "thread/history",
  "params": {
    "session_key": "websocket:abc",
    "before_message_id": null,
    "limit": 30
  }
}
```

响应：

```json
{
  "id": "history_1",
  "result": {
    "session_key": "websocket:abc",
    "thread_id": "websocket:abc",
    "messages": [
      {
        "role": "assistant",
        "content": "previous answer",
        "timestamp_ms": 1714200000000,
        "metadata": {},
        "message_id": "msg_1"
      }
    ],
    "has_more": false,
    "oldest_loaded_message_id": "msg_1"
  }
}
```

## Turn 与 Item 生命周期

`turn/start` 创建一次 agent 交互：

```json
{
  "id": "turn_req_1",
  "method": "turn/start",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "input": [{ "type": "text", "text": "hello" }],
    "stream": true,
    "model_provider": "anthropic",
    "model": "claude-sonnet-4-5",
    "metadata": {}
  }
}
```

服务端先返回初始 turn，再发送生命周期通知：

```json
{
  "id": "turn_req_1",
  "result": {
    "turn": {
      "session_id": "websocket:abc",
      "thread_id": "websocket:abc",
      "turn_id": "turn_1",
      "request_id": "turn_req_1",
      "status": "in_progress"
    }
  }
}
{
  "method": "turn/started",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "request_id": "turn_req_1",
    "status": "in_progress"
  }
}
```

流式输出：

```json
{
  "method": "item/started",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "item": {
      "item_id": "item_agent_turn_1",
      "turn_id": "turn_1",
      "type": "agentMessage",
      "status": "inProgress",
      "payload": {
        "response": {
          "content": "Hel",
          "metadata": {},
          "attachments": []
        }
      }
    }
  }
}
{
  "method": "item/agentMessage/delta",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "item_id": "item_agent_turn_1",
    "delta": "lo"
  }
}
```

终态：

```json
{
  "method": "item/completed",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "item": {
      "item_id": "item_agent_turn_1",
      "turn_id": "turn_1",
      "type": "agentMessage",
      "status": "completed",
      "payload": {
        "response": {
          "content": "Hello",
          "metadata": {},
          "attachments": []
        }
      }
    }
  }
}
{
  "method": "turn/completed",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "request_id": "turn_req_1",
    "status": "completed",
    "response": {
      "content": "Hello",
      "metadata": {},
      "attachments": []
    }
  }
}
```

非流式 v1 turn 也必须以 `item/completed` 和 `turn/completed` 闭环。客户端应以 `turn/completed`、`turn/failed` 或 `turn/interrupted` 作为 turn 终态。

## 内容块

`turn/start.params.input` 使用结构化 content blocks：

```json
[
  { "type": "text", "text": "请总结附件" },
  {
    "type": "attachment",
    "archive_id": "archive_1",
    "filename": "report.pdf",
    "mime_type": "application/pdf",
    "size_bytes": 1024
  },
  {
    "type": "image",
    "uri": "data:image/png;base64,...",
    "mime_type": "image/png"
  },
  {
    "type": "uiPayload",
    "namespace": "webui.card",
    "payload": {}
  }
]
```

稳定内容块：

- `text`：纯文本。
- `image`：图片 URI 或 archive 引用。
- `attachment`：归档附件引用。
- `uiPayload`：命名空间化 UI payload，不承载核心协议语义。

`metadata` 仅用于扩展命名空间，不承载核心协议语义，也不能存放密钥或长期凭据。

## Item 类型

v1 稳定 item 类型包括：

- `userMessage`：用户文本、图片和附件引用。
- `agentMessage`：assistant 正文和 content blocks。
- `reasoning`：推理摘要或可选原始 reasoning，受 capability 与配置控制。
- `plan`：计划文本与条目状态。
- `toolCall`：通用工具调用，包含 `tool_call_id`、`name`、`kind`、`status`、`arguments`、`result`、`error`。
- `commandExecution`：命令、cwd、stdout/stderr delta、exit code、sandbox/network 信息。
- `fileChange`：path、diff、status、approval state、grant root。
- `mcpToolCall`：server、tool、arguments、result/error。
- `approvalRequest`：审批目标、可选决策和权限范围。
- `dynamicToolCall`：动态工具调用。

客户端应按 `item_id` 合并同一 item 的 started、delta/update 和 completed 状态。

## 反向请求

当服务端需要审批、客户端工具执行或补充用户输入时，会发送带 `id` 的反向请求：

```json
{
  "id": "srv_req_1",
  "method": "approval/request",
  "params": {
    "request_id": "srv_req_1",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "scope": "turn",
    "prompt": "Allow command execution?",
    "metadata": {}
  }
}
```

客户端响应：

```json
{
  "id": "approval_response_1",
  "method": "approval/respond",
  "params": {
    "request_id": "srv_req_1",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "decision": "accept"
  }
}
```

服务端完成处理后发送：

```json
{
  "method": "serverRequest/resolved",
  "params": {
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "request_id": "srv_req_1",
    "item_id": "item_approval_1"
  }
}
```

审批必须包含 `scope`，取值为 `turn`、`session` 或 `thread`。默认应使用 `turn`，避免一次授权扩大到长期会话。

## 控制面

`turn/cancel` 请求中断一个 turn：

```json
{
  "id": "cancel_1",
  "method": "turn/cancel",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1"
  }
}
```

服务端响应 success envelope，并发送 `turn/interrupted` 终态通知：

```json
{
  "method": "turn/interrupted",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "request_id": "cancel_1",
    "status": "interrupted"
  }
}
```

## 错误模型

错误帧：

```json
{
  "id": "req_1",
  "error": {
    "code": "payload_too_large",
    "message": "websocket text frame exceeds the configured payload limit",
    "data": {
      "max_bytes": 1048576,
      "actual_bytes": 1048577,
      "retryable": false
    }
  }
}
```

稳定错误码：

| 类别 | code |
|------|------|
| 协议错误 | `invalid_json`, `invalid_request`, `method_not_found`, `invalid_params`, `not_initialized`, `unsupported_capability` |
| 资源错误 | `overloaded`, `payload_too_large`, `rate_limited`, `too_many_active_turns` |
| 业务错误 | `session_not_found`, `thread_not_found`, `turn_not_found`, `permission_denied` |
| Runtime 错误 | `model_error`, `tool_error`, `cancelled`, `timeout`, `internal_error` |

## 背压与资源限制

当前基础限制：

- 单个 WebSocket 文本帧最大 `1048576` 字节。
- 出站队列容量目标 `256`。
- 单连接 active turn 目标上限 `4`。

当服务端检测到队列或调度过载时，应返回 `overloaded`，并在 `data.retry_after_ms` 中给出带 jitter 的重试建议。客户端收到 `payload_too_large`、`too_many_active_turns`、`rate_limited` 或 `overloaded` 时不应立即无限重试。

## 安全边界

- 远程暴露 Gateway 时必须启用 gateway auth。
- 推荐使用 `Authorization: Bearer <token>` 握手认证。
- `token` / `access_token` query 参数仅为浏览器兼容保留，不建议用于新客户端。
- `connection_id` 不能作为授权凭据。
- `metadata` 和 `uiPayload` 不得存放密钥或长期凭据。
- 权限审批必须有 `scope`，避免将一次 turn 授权扩大为长期权限。

## Schema 与版本

`klaw-gateway` 暴露 `GatewayProtocolSchemaBundle::v1()`，包含核心 Rust 类型生成的 JSON Schema 定义。新增字段应默认可选；删除、改名或改变语义属于 breaking change，需要提升协议版本。

协议版本和 crate 版本绑定发布。客户端应优先基于 schema 生成类型，并对未知通知或未知可选字段保持前向兼容。

## 旧协议边界

旧版 `type: "method" | "result" | "event" | "error"` 帧属于兼容层，不是当前 WebUI 的正常协议路径。新客户端应使用 v1 JSON-RPC envelope：

- 旧 `workspace.bootstrap` 对应 v1 `session/list`。
- 旧 `provider.list` 对应 v1 `provider/list`。
- 旧 `session.history.load` 对应 v1 `thread/history`。
- 旧 `session.submit` 对应 v1 `turn/start`。
- 旧 `session.message` / `session.stream.*` 对应 v1 `item/*` 与 `turn/*` 生命周期通知。

## 当前限制

- TLS 仅有配置模型和校验，暂未接入证书加载与 HTTPS/WSS 监听。
- 连接和订阅状态为进程内内存结构，重启后不保留。
- 当前适用于单实例；尚未提供跨实例共享订阅或广播后端。
- `turn/steer`、`thread/resume`、`thread/rollback` 等控制面能力仍为预留协议面。
- 工具调用、审批和用户输入反向请求已有协议类型，运行时事件覆盖会继续扩展。

## 验证入口

维护本协议时至少运行：

```bash
cargo test -p klaw-gateway websocket_v1 --lib
cargo test -p klaw-gateway --test protocol_v1
cargo test -p klaw-runtime stream_ --lib
cargo check -p klaw-webui --target wasm32-unknown-unknown
mdbook build docs
```
