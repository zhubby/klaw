# Gateway WebSocket v1 协议

本文档是 `klaw-gateway` 当前 `/ws/chat` WebSocket 协议的完整说明。Gateway WebSocket v1 是面向 WebUI、桌面端、移动端和第三方客户端的长期 agent 交互协议底座，使用 JSON-RPC 语义的轻量 envelope，覆盖初始化、会话、Provider、历史、turn/item 生命周期、结构化内容、反向请求、错误、背压、安全和 schema 管理。

`klaw-webui` 已直接切换到 v1，不再发送旧版 `type: "method"` 帧，也不把旧版 `type: "event" | "result" | "error"` 服务端帧作为正常输入路径。

## 代码位置

- Gateway WebSocket 处理器：`klaw-gateway/src/websocket.rs`
- v1 协议类型与 schema：`klaw-gateway/src/protocol.rs`
- Gateway 状态与广播：`klaw-gateway/src/state.rs`
- Gateway 路由与启动：`klaw-gateway/src/runtime.rs`
- Gateway 认证：`klaw-gateway/src/auth.rs`
- Gateway 配置结构：`klaw-config/src/lib.rs`
- Runtime WebSocket 桥接：`klaw-runtime/src/gateway_websocket.rs`
- WebUI v1 客户端：`klaw-webui/src/web_chat/protocol.rs`、`klaw-webui/src/web_chat/transport.rs`

## 配置与启动

Gateway 配置位于根节点 `gateway`：

```toml
[gateway]
enabled = false
listen_ip = "127.0.0.1"
listen_port = 0

[gateway.auth]
enabled = false
token = "replace-me"
env_key = "KLAW_GATEWAY_AUTH_TOKEN"

[gateway.tailscale]
mode = "off"
reset_on_exit = false

[gateway.tls]
enabled = false
cert_path = "/path/to/fullchain.pem"
key_path = "/path/to/privkey.pem"

[gateway.webhook]
enabled = false

[gateway.webhook.events]
enabled = true
max_body_bytes = 262144

[gateway.webhook.agents]
enabled = false
max_body_bytes = 262144
```

配置说明：

- `gateway.auth`：控制所有 gateway 路径的 Bearer 鉴权。`token` 直接指定密钥，`env_key` 指定环境变量名；优先使用 `token`，其次从环境变量读取。
- `gateway.tailscale`：通过 Tailscale Serve/Funnel 暴露 gateway。`mode` 取值 `off`、`serve` 或 `funnel`。`reset_on_exit` 控制退出时是否回滚 Tailscale 配置。
- `gateway.webhook.events`：webhook 事件接收端点 `/webhook/events`，默认启用。
- `gateway.webhook.agents`：webhook agents 接收端点 `/webhook/agents`，默认禁用。
- `gateway.tls`：TLS 配置模型已就绪，但当前仅做校验，暂未接入证书加载与 HTTPS/WSS 监听。

启动链路：

- `klaw gateway` 或 GUI 内嵌 gateway 加载并校验配置。
- Gateway 绑定 `listen_ip:listen_port`；当 `listen_port = 0` 时由系统分配端口。
- 服务注册 `/ws/chat`、可选 webhook `/webhook/events` 与 `/webhook/agents`、archive 和 provider HTTP 路由。
- 若 `gateway.tailscale.mode` 不为 `off`，启动时自动配置 Tailscale Serve/Funnel 暴露。
- 远程暴露时应启用 `gateway.auth`；`/ws/chat` 支持 Bearer 鉴权，也保留 query token 兼容浏览器限制。

## 端点与握手

- 端点：`GET /ws/chat`
- 推荐鉴权：`Authorization: Bearer <token>`
- 兼容鉴权：`?token=<token>` 或 `?access_token=<token>`（仅 `/ws/chat` 路径接受 query token）
- 客户端应通过 `session/subscribe` 显式订阅目标会话；`/ws/chat?session_key=` 旧默认会话入口不再生效。
- Gateway 不接受 WebSocket binary 帧；收到 binary 帧时返回 `invalid_request` 错误。
- Ping/Pong 帧按 WebSocket 协议正常处理。

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
- 旧版 `type` 字段帧（`type: "method" | "event" | "result" | "error"`）不再被接受；服务端返回 `invalid_request` 错误。

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
      "experimental": false,
      "turns": true,
      "items": true,
      "tools": true,
      "approvals": true,
      "server_requests": true,
      "cancellation": true,
      "steering": true,
      "schema": true,
      "notification_opt_out": []
    }
  }
}
```

服务端响应包含：

- `protocol_version`：固定为 `"v1"`。
- `protocol_name`：`"gateway.websocket.v1"`。
- `connection_id`：服务端生成的连接级 UUID。
- 协商后的 `capabilities`：服务端保留客户端的 `experimental` 和 `notification_opt_out`，其余字段以服务端默认值为准（`turns: true`、`items: true`、`cancellation: true`、`steering: true`、`schema: true`、`tools: false`、`approvals: false`、`server_requests: false`）。
- `server_info`：`{ "name": "klaw-gateway", "version": "<crate_version>" }`。

客户端随后可发送 `initialized` 通知：

```json
{ "method": "initialized", "params": {} }
```

服务端对 `initialized` 不做额外处理，不返回任何帧。

实验字段必须通过 `capabilities.experimental = true` 显式启用。未协商的能力不能作为稳定协议依赖。

客户端可通过 `capabilities.notification_opt_out` 指定不想接收的通知方法名列表。服务端在协商后保留此列表，运行时事件推送应参考此列表跳过已 opt-out 的通知。

## 身份模型

- `connection_id`：连接级诊断和路由 ID，不作为权限边界。
- `session_id`：Klaw 工作区会话。在 `turn/start` 中等同于 `session_key`。
- `thread_id`：agent 对话上下文；当前 WebUI 通常与 `session_id` 相同，但协议不要求永久绑定。
- `turn_id`：一次用户请求及其后续 agent 工作。
- `item_id`：turn 内的一个工作单元，例如 assistant message、reasoning、tool call 或 file change。
- `request_id`：RPC 请求响应匹配 ID，不能替代 `turn_id` 或 `item_id`。
- `channel_id`：来源渠道标识，默认 `"default"`。

## 方法总览

### 客户端发起的请求方法

| 方法 | 方向 | 描述 |
|------|------|------|
| `initialize` | Client → Server | 初始化协议与能力协商 |
| `initialized` | Client → Server | 客户端初始化完成通知 |
| `session/list` | Client → Server | 获取工作区会话列表 |
| `session/create` | Client → Server | 创建新会话 |
| `session/update` | Client → Server | 更新会话标题等信息 |
| `session/delete` | Client → Server | 删除会话 |
| `session/subscribe` | Client → Server | 订阅会话实时事件 |
| `session/unsubscribe` | Client → Server | 取消当前连接的会话订阅 |
| `provider/list` | Client → Server | 获取模型提供商列表 |
| `thread/history` | Client → Server | 按游标分页读取会话历史 |
| `thread/read` | Client → Server | `thread/history` 的别名，参数和响应完全相同 |
| `turn/start` | Client → Server | 创建一次用户 turn |
| `turn/cancel` | Client → Server | 中断一个 turn |
| `approval/respond` | Client → Server | 响应审批反向请求（⚠️ 当前未实现，返回 `method_not_found`） |
| `tool/respond` | Client → Server | 响应客户端工具反向请求（⚠️ 当前未实现，返回 `method_not_found`） |
| `user_input/respond` | Client → Server | 响应补充用户输入反向请求（⚠️ 当前未实现，返回 `method_not_found`） |

### 服务端发起的通知方法

| 方法 | 方向 | 描述 |
|------|------|------|
| `session/subscribed` | Server → Client | 会话订阅成功通知 |
| `session/unsubscribed` | Server → Client | 会话取消订阅通知 |
| `turn/started` | Server → Client | turn 开始执行通知 |
| `turn/completed` | Server → Client | turn 正常完成终态通知 |
| `turn/failed` | Server → Client | turn 失败终态通知 |
| `turn/interrupted` | Server → Client | turn 被中断终态通知 |
| `item/started` | Server → Client | item 开始通知 |
| `item/updated` | Server → Client | item 状态更新通知 |
| `item/completed` | Server → Client | item 完成通知 |
| `item/agentMessage/delta` | Server → Client | agent message 流式增量文本 |
| `item/agentMessage/clear` | Server → Client | 清除 agent message 内容 |
| `item/reasoning/delta` | Server → Client | reasoning 流式增量 |
| `item/plan/delta` | Server → Client | plan 流式增量 |
| `serverRequest/resolved` | Server → Client | 服务端反向请求闭环通知 |

### 服务端发起的反向请求方法

| 方法 | 方向 | 描述 |
|------|------|------|
| `approval/request` | Server → Client | 请求客户端审批决策 |
| `tool/requestUserInput` | Server → Client | 请求客户端补充用户输入 |

### 预留但尚未实现的方法

`thread/start`、`thread/resume`、`thread/list`、`thread/rollback`、`turn/steer`、`turn/read` 等方法已在协议类型中预留，但服务端收到后返回 `method_not_found`。

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

会话列表按 `created_at_ms` 降序排序，相同时间按 `session_key` 降序排序。

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

### 会话操作

```json
{ "id": "create_1", "method": "session/create", "params": {} }
```

`session/create` 响应：

```json
{
  "id": "create_1",
  "result": {
    "session_key": "websocket:new_session",
    "title": "Agent new_session",
    "created_at_ms": 1714200100000,
    "model_provider": null,
    "model": null
  }
}
```

创建会话后，连接自动订阅该会话（等同于隐式 `session/subscribe`）。

```json
{
  "id": "rename_1",
  "method": "session/update",
  "params": { "session_key": "websocket:abc", "title": "New title" }
}
```

`session/update` 要求 `session_key` 和 `title` 均为非空字符串。响应包含 `updated: true` 标记：

```json
{
  "id": "rename_1",
  "result": {
    "session_key": "websocket:abc",
    "title": "New title",
    "created_at_ms": 1714200000000,
    "model_provider": "anthropic",
    "model": "claude-sonnet-4-5",
    "updated": true
  }
}
```

```json
{
  "id": "delete_1",
  "method": "session/delete",
  "params": { "session_key": "websocket:abc" }
}
```

`session/delete` 响应：

```json
{
  "id": "delete_1",
  "result": { "session_key": "websocket:abc", "deleted": true }
}
```

```json
{
  "id": "subscribe_1",
  "method": "session/subscribe",
  "params": { "session_key": "websocket:abc" }
}
```

订阅成功后，服务端返回 success envelope，并发送 `session/subscribed` 通知：

```json
{
  "id": "subscribe_1",
  "result": { "session_key": "websocket:abc" }
}
{ "method": "session/subscribed", "params": { "session_key": "websocket:abc" } }
```

订阅后连接会自动关联该会话，后续 `turn/start` 可省略 `session_id`（将使用当前订阅的会话）。

```json
{
  "id": "unsubscribe_1",
  "method": "session/unsubscribe",
  "params": {}
}
```

取消订阅同理发送 `session/unsubscribed` 通知：

```json
{
  "id": "unsubscribe_1",
  "result": { "session_key": "websocket:abc" }
}
{ "method": "session/unsubscribed", "params": { "session_key": "websocket:abc" } }
```

取消订阅会清除连接上所有会话关联和订阅记录。

### 历史分页

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

`thread/read` 为 `thread/history` 的别名，使用完全相同的参数和响应格式。

参数说明：

- `session_key`：必填，非空字符串。
- `before_message_id`：可选游标，用于加载更早的消息。
- `limit`：可选，默认 `10`，最小 `1`。

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

### turn/start

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
    "channel_id": "default",
    "stream": true,
    "model_provider": "anthropic",
    "model": "claude-sonnet-4-5",
    "metadata": {}
  }
}
```

参数说明：

- `session_id`：可选；若省略则使用当前订阅的会话。两者均无时返回 `invalid_params`。
- `thread_id`：可选；若省略则默认等同于 `session_id`。也接受 `chat_id` 作为别名（向后兼容）。
- `turn_id`：可选；若省略则自动生成 `turn_{request_id}` 格式。同一连接不允许重复的活跃 `turn_id`。
- `input`：结构化 content blocks 数组（见「内容块」），必须至少包含一个文本块或附件块。
- `channel_id`：可选来源渠道标识，默认 `"default"`。
- `stream`：可选，是否流式输出。
- `model_provider`：可选，指定模型提供商。
- `model`：可选，指定具体模型。
- `metadata`：可选扩展元数据，不可存放密钥或长期凭据。

服务端先返回初始 turn success 响应，并向订阅该 session 的连接广播已完成的 `userMessage` item 与 `turn/started` 通知：

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
  "method": "item/completed",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "item": {
      "item_id": "item_user_turn_1",
      "turn_id": "turn_1",
      "type": "userMessage",
      "status": "completed",
      "payload": {
        "message": {
          "content": "Hello",
          "metadata": {},
          "attachments": []
        }
      }
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

### 活跃 turn 限制

单个 WebSocket 连接最多 `4` 个并发活跃 turn。超出时返回：

```json
{
  "id": "turn_req_2",
  "error": {
    "code": "too_many_active_turns",
    "message": "too many active websocket v1 turns for this connection",
    "data": { "max_active_turns": 4, "retryable": true }
  }
}
```

### 流式输出

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

### 终态

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

### turn 失败

当 handler submit 失败时，服务端发送 `turn/failed` 通知：

```json
{
  "method": "turn/failed",
  "params": {
    "session_id": "websocket:abc",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "request_id": "turn_req_1",
    "status": "failed",
    "error": {
      "code": "model_error",
      "message": "provider call failed"
    }
  }
}
```

非流式 v1 turn 也必须以 `item/completed` 和 `turn/completed` 闭环。客户端应以 `turn/completed`、`turn/failed` 或 `turn/interrupted` 作为 turn 终态。

## 内容块

`turn/start.params.input` 使用结构化 content blocks 数组：

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
    "archive_id": "archive_img_1",
    "mime_type": "image/png"
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

- `text`：纯文本，`text` 字段必填。
- `image`：图片。`mime_type` 必填；`uri`（内联 URI）可选；`archive_id`（归档引用）可选。二者至少提供一个有意义的引用。
- `attachment`：归档附件引用。`archive_id` 必填；`filename`、`mime_type` 可选；`size_bytes` 默认 `0`。
- `uiPayload`：命名空间化 UI payload，`namespace` 和 `payload` 必填，不承载核心协议语义。

服务端处理 `input` 时：从 `text` 块提取文本拼接为输入字符串（以 `\n` 连接），从 `attachment` 块和带 `archive_id` 的 `image` 块提取附件引用传给 handler。`image` 块中的 `uri` 不提取为附件引用。

`metadata` 仅用于扩展命名空间，不承载核心协议语义，也不能存放密钥或长期凭据。

## Item 类型

v1 稳定 item 类型（`GatewayThreadItemType::stable_v1()`）包括：

| 类型 | 描述 |
|------|------|
| `userMessage` | 用户文本、图片和附件引用 |
| `agentMessage` | assistant 正文和 content blocks |
| `reasoning` | 推理摘要或可选原始 reasoning，受 capability 与配置控制 |
| `plan` | 计划文本与条目状态 |
| `toolCall` | 通用工具调用，包含 `tool_call_id`、`name`、`kind`、`status`、`arguments`、`result`、`error` |
| `commandExecution` | 命令、cwd、stdout/stderr delta、exit code、sandbox/network 信息 |
| `fileChange` | path、diff、status、approval state、grant root |
| `mcpToolCall` | server、tool、arguments、result/error |
| `approvalRequest` | 审批目标、可选决策和权限范围 |
| `dynamicToolCall` | 动态工具调用 |

Item 状态（`GatewayThreadItemStatus`）包括 `pending`、`inProgress`、`completed`、`failed`、`declined`、`interrupted`。

客户端应按 `item_id` 合并同一 item 的 started、delta/update 和 completed 状态。

## Tool Call 结构

`GatewayToolCall` 包含：

```json
{
  "tool_call_id": "tool_1",
  "name": "shell",
  "kind": "command",
  "status": "in_progress",
  "arguments": { "command": "cargo test" },
  "result": null,
  "error": null,
  "duration_ms": null
}
```

Tool call 状态（`GatewayToolCallStatus`）包括 `pending`、`in_progress`、`completed`、`failed`、`declined`、`cancelled`。

## 反向请求

### approval/request

当服务端需要审批决策时，发送带 `id` 的反向请求：

```json
{
  "id": "srv_req_1",
  "method": "approval/request",
  "params": {
    "request_id": "srv_req_1",
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "item_id": "item_approval_1",
    "scope": "turn",
    "message": "Allow command execution?",
    "options": ["accept", "decline"],
    "payload": { "tool_call_id": "tool_1" }
  }
}
```

`GatewayApprovalRequest` 字段：

- `request_id`：反向请求 ID。
- `thread_id`、`turn_id`：上下文定位。
- `item_id`：关联的审批 item。
- `scope`：审批范围，取值为 `turn`、`session` 或 `thread`。默认应使用 `turn`，避免一次授权扩大到长期会话。
- `message`：可选审批提示文案。
- `options`：可选决策列表。
- `payload`：可选附加上下文数据。

### approval/respond

客户端响应审批（⚠️ 当前未实现，返回 `method_not_found`）：

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

审批决策（`GatewayApprovalDecision`）包括：

- `accept`：仅本次 turn 授权。
- `accept_for_session`：本次会话后续相同操作均授权。
- `decline`：拒绝。
- `cancel`：取消整个 turn。

### serverRequest/resolved

服务端完成反向请求处理后发送：

```json
{
  "method": "serverRequest/resolved",
  "params": {
    "thread_id": "websocket:abc",
    "turn_id": "turn_1",
    "request_id": "srv_req_1"
  }
}
```

### tool/requestUserInput

服务端请求客户端补充用户输入（反向请求）。

### tool/respond 与 user_input/respond

⚠️ 当前未实现，返回 `method_not_found` 错误。

## 控制面

### turn/cancel

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

参数说明：

- `thread_id`：必填，必须与活跃 turn 的 `thread_id` 匹配，否则返回 `thread_not_found`。
- `turn_id`：必填，必须在当前连接的活跃 turn 中存在，否则返回 `turn_not_found`。
- `session_id`：可选，用于响应 payload。

服务端 abort 对应的 handler 任务，响应 success envelope，并发送 `turn/interrupted` 终态通知：

```json
{
  "id": "cancel_1",
  "result": {
    "status": "interrupted",
    "turn": {
      "session_id": "websocket:abc",
      "thread_id": "websocket:abc",
      "turn_id": "turn_1",
      "request_id": "cancel_1",
      "status": "interrupted"
    }
  }
}
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

连接断开时，服务端自动 abort 所有活跃 turn 并清理订阅状态。

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

稳定错误码（`GatewayProtocolErrorCode::stable_v1()`）：

| 类别 | code |
|------|------|
| 协议错误 | `invalid_json`, `invalid_request`, `method_not_found`, `invalid_params`, `not_initialized`, `unsupported_capability` |
| 资源错误 | `overloaded`, `payload_too_large`, `rate_limited`, `too_many_active_turns` |
| 业务错误 | `session_not_found`, `thread_not_found`, `turn_not_found`, `permission_denied` |
| Runtime 错误 | `model_error`, `tool_error`, `cancelled`, `timeout`, `internal_error` |

Handler 错误映射：handler 返回的 `GatewayWebsocketHandlerError` 会自动映射为协议错误码——`invalid_request` → `invalid_params`、`session_not_found`/`missing_session` → `session_not_found`、`thread_not_found` → `thread_not_found`、`turn_not_found` → `turn_not_found`、`permission_denied` → `permission_denied`、`timeout` → `timeout`、其余 → `internal_error`。

## 背压与资源限制

当前基础限制：

- 单个 WebSocket 文本帧最大 `1048576` 字节（`GATEWAY_WEBSOCKET_MAX_TEXT_FRAME_BYTES = 1 MiB`）。超出时返回 `payload_too_large` 并附带 `data.max_bytes`、`data.actual_bytes`、`data.retryable`。
- 出站队列容量 `256`（`GATEWAY_WEBSOCKET_OUTBOUND_QUEUE_CAPACITY`）。
- 单连接并发活跃 turn 上限 `4`（`GATEWAY_WEBSOCKET_MAX_ACTIVE_TURNS_PER_CONNECTION`）。

当服务端检测到队列或调度过载时，应返回 `overloaded`，并在 `data.retry_after_ms` 中给出带 jitter 的重试建议。客户端收到 `payload_too_large`、`too_many_active_turns`、`rate_limited` 或 `overloaded` 时不应立即无限重试。

## 安全边界

- 远程暴露 Gateway 时必须启用 `gateway.auth`。
- 推荐使用 `Authorization: Bearer <token>` 握手认证。
- `token` / `access_token` query 参数仅为 `/ws/chat` 路径保留浏览器兼容，不建议用于新客户端。
- `connection_id` 不能作为授权凭据。
- `metadata` 和 `uiPayload` 不得存放密钥或长期凭据。
- 权限审批必须有 `scope`，避免将一次 turn 授权扩大为长期权限。

## 会话订阅与广播

Gateway 维护进程内的 `GatewayWebsocketBroadcaster`，跟踪每个连接的订阅会话集合：

- `session/subscribe` 将 `session_key` 添加到连接的订阅集合，并设置为当前活跃会话。
- `session/unsubscribe` 清除连接的所有订阅和活跃会话。
- `session/create` 自动订阅新会话。
- `turn/start` 自动关联并订阅 `session_id`。
- 所有 item/turn 生命周期通知通过 broadcaster 路由到订阅了对应 `session_key` 的连接。

当出站队列满或连接已断开时，broadcaster 自动清理 stale 连接。

## Schema 与版本

`klaw-gateway` 暴露 `GatewayProtocolSchemaBundle::v1()`，包含核心 Rust 类型生成的 JSON Schema 定义和稳定错误码列表。Schema 定义包括：

- `GatewayRpcMessage`
- `GatewayWebsocketProtocolInitializeParams` / `GatewayWebsocketProtocolInitializeResult`
- `GatewayWebsocketTurnStarted`
- `GatewayThreadItem`
- `GatewayContentBlock`
- `GatewayToolCall`
- `GatewayApprovalRequest`
- `GatewayServerRequestResolved`

新增字段应默认可选；删除、改名或改变语义属于 breaking change，需要提升协议版本。

协议版本和 crate 版本绑定发布。客户端应优先基于 schema 生成类型，并对未知通知或未知可选字段保持前向兼容。

## v1-only 边界

旧版 `type: "method" | "result" | "event" | "error"` 帧已移除。服务端不会发送 legacy startup frame，也不会接受旧客户端 method frame。具体映射：

- 旧 `workspace.bootstrap` → v1 `session/list`
- 旧 `provider.list` → v1 `provider/list`
- 旧 `session.history.load` → v1 `thread/history`
- 旧 `session.submit` → v1 `turn/start`
- 旧 `session.message` / `session.stream.*` → v1 `item/*` 与 `turn/*` 生命周期通知

Gateway `GatewayWebsocketServerFrame` 的 `Deserialize` 实现会拒绝包含 `type` 字段的帧，返回错误消息："legacy websocket frames are not accepted by the v1 gateway protocol"。

## 当前限制

- TLS 仅有配置模型和校验，暂未接入证书加载与 HTTPS/WSS 监听（返回 `TlsNotImplemented` 错误）。
- `approval/respond`、`tool/respond`、`user_input/respond` 已有协议类型定义，但服务端处理逻辑未实现，当前返回 `method_not_found`。
- 连接和订阅状态为进程内内存结构（`GatewayWebsocketBroadcaster`），重启后不保留。
- 当前适用于单实例；尚未提供跨实例共享订阅或广播后端。
- `turn/steer`、`thread/resume`、`thread/rollback` 等控制面能力仍为预留协议面，未实现。
- 工具调用和审批反向请求已有协议类型，运行时事件覆盖会继续扩展。

## 验证入口

维护本协议时至少运行：

```bash
cargo test -p klaw-gateway --lib
cargo test -p klaw-gateway --test protocol_v1
cargo test -p klaw-runtime stream_ --lib
cargo check -p klaw-webui --target wasm32-unknown-unknown
mdbook build docs
```