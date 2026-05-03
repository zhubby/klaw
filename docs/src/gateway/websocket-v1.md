# Gateway WebSocket v1 基线协议

Gateway WebSocket v1 是 `klaw-gateway` 面向 WebUI、桌面端、移动端和第三方客户端的长期 agent 交互协议底座。`klaw-webui` 已直接切换到 v1 JSON-RPC envelope，不再把旧版 `type: "method"` 帧作为正常收发路径。

## Envelope

v1 使用 JSON-RPC 2.0 语义，但线上帧省略 `jsonrpc` 字段。每个 WebSocket 文本帧承载一个 JSON 消息：

```json
{ "id": "req_1", "method": "turn/start", "params": {} }
{ "id": "req_1", "result": {} }
{ "method": "item/started", "params": {} }
{ "id": "srv_req_1", "method": "approval/request", "params": {} }
```

请求必须包含 `id`，响应必须 echo 同一个 `id`。服务端通知没有 `id`。服务端反向请求包含 `id`，客户端用 `approval/respond`、`tool/respond` 或 `user_input/respond` 闭环。

## 初始化

v1 客户端连接建立后发送 `initialize`：

```json
{
  "id": "init_1",
  "method": "initialize",
  "params": {
    "client_info": {
      "name": "my-client",
      "title": "My Client",
      "version": "0.1.0"
    },
    "capabilities": {
      "protocol_version": "v1",
      "turns": true,
      "items": true,
      "tools": true,
      "approvals": true,
      "schema": true
    }
  }
}
```

响应包含协议名、连接 ID、服务端信息和协商后的 capabilities。实验字段必须通过 `capabilities.experimental = true` 显式启用。

## 工作区与历史

WebUI 启动后使用 v1 方法加载工作区、provider 和历史：

```json
{ "id": "sessions_1", "method": "session/list", "params": {} }
{ "id": "providers_1", "method": "provider/list", "params": {} }
{
  "id": "history_1",
  "method": "thread/history",
  "params": {
    "session_key": "websocket:session",
    "before_message_id": null,
    "limit": 30
  }
}
```

会话操作使用 `session/create`、`session/update`、`session/delete`、`session/subscribe` 和 `session/unsubscribe`。所有响应均使用 v1 success/error envelope，不返回旧版 `type: "result"`。

## 身份模型

- `connection_id`：连接级诊断和路由 ID，不作为权限边界。
- `session_id`：Klaw 工作区会话，兼容旧 `session_key`。
- `thread_id`：agent 对话上下文。当前实现可与 `session_id` 一一对应，但协议不要求永久绑定。
- `turn_id`：一次用户请求及其后续 agent 工作。
- `item_id`：turn 内的一个工作单元，例如 assistant message、reasoning、tool call、file change。
- `request_id`：RPC 请求响应匹配 ID，不能替代 `turn_id` 或 `item_id`。

## Turn 与 Item 生命周期

`turn/start` 创建一次 agent 交互：

```json
{
  "id": "turn_req_1",
  "method": "turn/start",
  "params": {
    "session_id": "websocket:session",
    "thread_id": "thr_session",
    "turn_id": "turn_1",
    "input": [{ "type": "text", "text": "hello" }],
    "model_provider": "anthropic",
    "model": "claude-opus-4-1"
  }
}
```

服务端先返回初始 turn，再发送生命周期通知：

```json
{ "id": "turn_req_1", "result": { "turn": { "turn_id": "turn_1", "status": "in_progress" } } }
{ "method": "turn/started", "params": { "turn_id": "turn_1", "status": "in_progress" } }
{ "method": "item/started", "params": { "item": { "type": "agentMessage", "status": "inProgress" } } }
{ "method": "item/agentMessage/delta", "params": { "delta": "Hello" } }
{ "method": "item/completed", "params": { "item": { "type": "agentMessage", "status": "completed" } } }
{ "method": "turn/completed", "params": { "turn_id": "turn_1", "status": "completed" } }
```

WebUI 以 `item/*` 与 `turn/*` 为实时渲染主路径，不依赖旧版 `session.message` 或 `session.stream.*` 帧。

## 内容与工具

`input` 使用结构化 content blocks：

- `text`：纯文本。
- `image`：图片 URI 或 archive 引用。
- `attachment`：归档附件引用。
- `uiPayload`：命名空间化 UI payload，不承载核心协议语义。

v1 稳定 item 类型包括 `userMessage`、`agentMessage`、`reasoning`、`plan`、`toolCall`、`commandExecution`、`fileChange`、`mcpToolCall`、`approvalRequest` 和 `dynamicToolCall`。工具调用必须包含稳定 `tool_call_id`、`name`、`kind`、`status`、`arguments`、`result` 或 `error`。

## 反向请求

需要审批、客户端工具执行或用户补充输入时，服务端发送带 `id` 的请求。客户端响应后，服务端会发送 `serverRequest/resolved`：

```json
{
  "id": "approval_response_1",
  "method": "approval/respond",
  "params": {
    "request_id": "srv_req_1",
    "thread_id": "thr_1",
    "turn_id": "turn_1",
    "decision": "accept"
  }
}
```

权限审批必须有 `scope`，取值为 `turn`、`session` 或 `thread`。默认应使用 `turn`，避免一次授权扩大到长期会话。

## 控制面

`turn/cancel` 请求中断一个 turn，并以 `turn/interrupted` 作为终态事件：

```json
{
  "id": "cancel_1",
  "method": "turn/cancel",
  "params": {
    "session_id": "websocket:session",
    "thread_id": "thr_1",
    "turn_id": "turn_1"
  }
}
```

协议预留 `turn/steer`、`turn/read`、`thread/resume` 和 `thread/rollback`，用于后续恢复、追加输入和回滚能力；`thread/history` 当前用于分页读取会话历史。

## 错误与背压

错误帧使用稳定 code：

```json
{
  "id": "req_1",
  "error": {
    "code": "payload_too_large",
    "message": "websocket text frame exceeds the configured payload limit",
    "data": { "max_bytes": 1048576, "actual_bytes": 1048577, "retryable": false }
  }
}
```

当前基础限制：

- 单个文本帧最大 `1048576` 字节。
- 出站队列容量目标 `256`。
- 单连接 active turn 目标上限 `4`。

当服务端未来检测到队列或调度过载时，应返回 `overloaded`，并在 `data.retry_after_ms` 中给出带 jitter 的重试建议。

## 安全

- 远程暴露 Gateway 时必须启用 gateway auth。
- 推荐使用 `Authorization: Bearer <token>` 握手认证。
- `token` / `access_token` query 参数仅为浏览器兼容保留，不建议用于新客户端。
- `connection_id` 不能作为授权凭据。
- 核心协议语义必须使用结构化字段；`metadata` 仅用于扩展命名空间，不能存放密钥或长期凭据。

## Schema

`klaw-gateway` 暴露 `GatewayProtocolSchemaBundle::v1()`，包含核心 Rust 类型生成的 JSON Schema 定义。新增字段应默认可选；删除、改名或改变语义属于 breaking change，需要提升协议版本。
