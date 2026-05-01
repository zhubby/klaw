# Gateway HTTP API 文档

本文档描述 `klaw-gateway` 模块提供的 HTTP RESTful API 接口，供 WebUI 和第三方客户端调用。这些接口与 WebSocket 接口互补，提供文件归档、模型提供商查询、健康检查等功能。

## 概述

- 基础路径：`http://<listen_ip>:<listen_port>`
- 响应格式：JSON（除文件下载等特殊接口外）
- 认证方式：部分接口需要 Bearer Token（与 WebSocket 共享相同的 `gateway.webhook.token` 配置）

## API 端点列表

### 1. 归档管理接口

归档系统用于存储和管理用户上传的文件附件。

#### 1.1 上传文件

- **端点**：`POST /archive/upload`
- **Content-Type**：`multipart/form-data`
- **描述**：上传文件到归档系统

**请求参数：**

| 字段 | 类型 | 必填 | 描述 |
|------|------|------|------|
| `file` | File | 是 | 要上传的文件 |
| `session_key` | string | 否 | 关联的会话键 |
| `channel` | string | 否 | 关联的通道 |
| `chat_id` | string | 否 | 关联的聊天 ID |
| `message_id` | string | 否 | 关联的消息 ID |

**响应示例（成功）：**

```json
{
  "success": true,
  "record": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "source_kind": "user_upload",
    "media_kind": "document",
    "original_filename": "report.pdf",
    "mime_type": "application/pdf",
    "size_bytes": 1048576,
    "created_at": "2025-05-01T12:00:00Z"
  },
  "error": null
}
```

**响应示例（失败）：**

```json
{
  "success": false,
  "record": null,
  "error": "missing file field"
}
```

**状态码：**

- `200 OK`：上传成功
- `400 Bad Request`：请求参数错误（如缺少文件）
- `500 Internal Server Error`：服务器内部错误
- `503 Service Unavailable`：归档服务不可用

#### 1.2 下载文件

- **端点**：`GET /archive/download/{id}`
- **描述**：根据归档 ID 下载文件

**路径参数：**

| 参数 | 描述 |
|------|------|
| `id` | 归档记录 ID |

**响应：**

- 成功：文件内容（`Content-Type` 为文件的 MIME 类型）
- 响应头包含 `Content-Disposition: attachment; filename="xxx"`

**状态码：**

- `200 OK`：下载成功
- `404 Not Found`：文件不存在
- `503 Service Unavailable`：归档服务不可用

#### 1.3 查询归档列表

- **端点**：`GET /archive/list`
- **描述**：分页查询归档记录列表

**查询参数：**

| 参数 | 类型 | 默认值 | 描述 |
|------|------|--------|------|
| `session_key` | string | - | 按会话键筛选 |
| `chat_id` | string | - | 按聊天 ID 筛选 |
| `source_kind` | string | - | 按来源类型筛选（如 `user_upload`） |
| `media_kind` | string | - | 按媒体类型筛选（如 `document`, `image`, `audio`） |
| `filename` | string | - | 按文件名模糊匹配 |
| `limit` | integer | 20 | 每页数量 |
| `offset` | integer | 0 | 偏移量 |

**响应示例：**

```json
{
  "success": true,
  "records": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "source_kind": "user_upload",
      "media_kind": "document",
      "original_filename": "report.pdf",
      "mime_type": "application/pdf",
      "size_bytes": 1048576,
      "created_at": "2025-05-01T12:00:00Z"
    }
  ],
  "error": null
}
```

**状态码：**

- `200 OK`：查询成功
- `500 Internal Server Error`：服务器内部错误
- `503 Service Unavailable`：归档服务不可用

#### 1.4 获取归档详情

- **端点**：`GET /archive/{id}`
- **描述**：获取单个归档记录的元数据（不包含文件内容）

**路径参数：**

| 参数 | 描述 |
|------|------|
| `id` | 归档记录 ID |

**响应示例（成功）：**

```json
{
  "success": true,
  "record": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "source_kind": "user_upload",
    "media_kind": "document",
    "original_filename": "report.pdf",
    "mime_type": "application/pdf",
    "size_bytes": 1048576,
    "created_at": "2025-05-01T12:00:00Z"
  },
  "error": null
}
```

**响应示例（失败）：**

```json
{
  "success": false,
  "record": null,
  "error": "archive not found"
}
```

**状态码：**

- `200 OK`：查询成功
- `404 Not Found`：归档记录不存在
- `503 Service Unavailable`：归档服务不可用

### 2. 模型提供商接口

#### 2.1 获取提供商列表

- **端点**：`GET /providers/list`
- **描述**：获取所有配置的 LLM 提供商信息

**响应示例：**

```json
{
  "success": true,
  "providers": [
    {
      "id": "anthropic",
      "name": "Anthropic",
      "base_url": "https://api.anthropic.com",
      "wire_api": "messages",
      "default_model": "claude-sonnet-4-5",
      "stream": true,
      "has_api_key": true
    },
    {
      "id": "openai",
      "name": "OpenAI",
      "base_url": "https://api.openai.com/v1",
      "wire_api": "chat_completions",
      "default_model": "gpt-4o-mini",
      "stream": true,
      "has_api_key": true
    }
  ],
  "default_provider": "anthropic",
  "error": null
}
```

**字段说明：**

| 字段 | 类型 | 描述 |
|------|------|------|
| `id` | string | 提供商唯一标识 |
| `name` | string | 提供商显示名称 |
| `base_url` | string | API 基础 URL |
| `wire_api` | string | API 类型（`chat_completions` 或 `messages`） |
| `default_model` | string | 默认模型 |
| `stream` | boolean | 是否支持流式响应 |
| `has_api_key` | boolean | 是否已配置 API 密钥 |

**状态码：**

- `200 OK`：查询成功
- `503 Service Unavailable`：提供商服务不可用

### 3. Webhook 事件接口

#### 3.1 发送结构化事件

- **端点**：`POST /webhook/events`
- **描述**：向系统发送结构化 webhook 事件
- **认证**：`Authorization: Bearer <token>`

**请求体：**

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

**响应示例（成功）：**

```json
{
  "event_id": "2f4e6f1c-8d8d-4b4f-a45e-2f9a71e84384",
  "status": "accepted",
  "session_key": "webhook:github:42"
}
```

**状态码：**

- `202 Accepted`：事件已受理
- `400 Bad Request`：请求体非法
- `401 Unauthorized`：认证失败
- `413 Payload Too Large`：请求体超过大小限制

#### 3.2 Agent Webhook 调用

- **端点**：`POST /webhook/agents`
- **描述**：调用配置的 Agent webhook 模板
- **认证**：`Authorization: Bearer <token>`
- **查询参数：**

| 参数 | 必填 | 描述 |
|------|------|------|
| `hook_id` | 是 | Agent Hook 标识 |
| `session_key` | 否 | 会话键 |
| `base_session_key` | 否 | 基础会话键 |
| `chat_id` | 否 | 聊天 ID |
| `sender_id` | 否 | 发送者 ID |
| `provider` | 否 | 指定模型提供商 |
| `model` | 否 | 指定模型 |

**请求体：** 任意 JSON 对象，作为 Agent 的输入 `body`

**响应示例（成功）：**

```json
{
  "request_id": "req-uuid",
  "status": "accepted",
  "hook_id": "my-agent-hook",
  "session_key": "webhook:agent:123"
}
```

**状态码：**

- `202 Accepted`：请求已受理
- `400 Bad Request`：参数错误
- `401 Unauthorized`：认证失败

### 4. 健康检查接口

#### 4.1 存活检查 (Liveness)

- **端点**：`GET /health/live`
- **描述**：检查服务是否存活

**响应：**

```
ok
```

**状态码：**

- `200 OK`：服务存活
- `503 Service Unavailable`：服务不可用

#### 4.2 就绪检查 (Readiness)

- **端点**：`GET /health/ready`
- **描述**：检查服务是否就绪（可以接收请求）

**响应：**

```
ready
```

或

```
not_ready: <reason>
```

**状态码：**

- `200 OK`：服务就绪
- `503 Service Unavailable`：服务未就绪

#### 4.3 综合状态检查

- **端点**：`GET /health/status`
- **描述**：获取详细的健康状态信息

**响应示例：**

```json
{
  "status": "healthy",
  "components": [
    {
      "name": "websocket",
      "status": "healthy",
      "message": "connected"
    },
    {
      "name": "archive",
      "status": "healthy",
      "message": "available"
    },
    {
      "name": "webhook",
      "status": "degraded",
      "message": "queue backlog > 100"
    }
  ]
}
```

**状态码：**

- `200 OK`：返回状态信息

### 5. 监控指标接口

#### 5.1 Prometheus 指标

- **端点**：`GET /metrics`
- **描述**：获取 Prometheus 格式的监控指标
- **Content-Type**：`text/plain; version=0.0.4; charset=utf-8`

**响应示例：**

```
# HELP gateway_websocket_connections_total Total WebSocket connections
# TYPE gateway_websocket_connections_total counter
gateway_websocket_connections_total 42

# HELP gateway_webhook_events_total Total webhook events received
# TYPE gateway_webhook_events_total counter
gateway_webhook_events_total 128

# HELP gateway_archive_bytes_total Total archive bytes stored
# TYPE gateway_archive_bytes_total counter
gateway_archive_bytes_total 104857600
```

**状态码：**

- `200 OK`：返回指标数据
- `404 Not Found`：Prometheus 指标未启用
- `500 Internal Server Error`：指标渲染失败

### 6. WebUI 静态资源

#### 6.1 聊天页面

- **端点**：`GET /chat`
- **描述**：WebUI 聊天应用主页面
- **响应**：HTML 页面

#### 6.2 WebUI JS 文件

- **端点**：`GET /chat/dist/klaw_webui.js`
- **描述**：WebUI JavaScript 主文件
- **响应**：JavaScript 代码

#### 6.3 WebUI WASM 文件

- **端点**：`GET /chat/dist/klaw_webui_bg.wasm`
- **描述**：WebUI WebAssembly 文件
- **响应**：WASM 二进制

#### 6.4 首页与其他资源

| 端点 | 描述 |
|------|------|
| `GET /` | 网关首页 |
| `GET /logo.webp` | 网关 Logo |
| `GET /favicon.ico` | 网站图标 |
| `GET /images/{filename}` | 图片资源 |

## 错误处理

所有 JSON 响应的错误遵循统一格式：

```json
{
  "success": false,
  "error": "人类可读的错误描述",
  "error_code": "machine_readable_error_code"
}
```

常见错误码：

| 错误码 | HTTP 状态 | 描述 |
|--------|-----------|------|
| `service_unavailable` | 503 | 服务未启用或暂时不可用 |
| `not_found` | 404 | 资源不存在 |
| `invalid_params` | 400 | 请求参数无效 |
| `unauthorized` | 401 | 认证失败 |
| `internal_error` | 500 | 服务器内部错误 |

## 使用示例

### 上传文件并发送消息

```bash
# 1. 上传文件
curl -X POST http://localhost:18080/archive/upload \
  -F "file=@document.pdf" \
  -F "session_key=my-session"

# 响应: {"success":true,"record":{"id":"archive-123"}}

# 2. 通过 WebSocket 发送消息引用该文件
# 使用 session.submit 方法，包含 attachments:
# {
#   "type": "method",
#   "id": "...",
#   "method": "session.submit",
#   "params": {
#     "session_key": "my-session",
#     "input": "请分析这个文档",
#     "attachments": [{"archive_id": "archive-123"}]
#   }
# }
```

### 查询提供商并切换模型

```bash
# 获取提供商列表
curl http://localhost:18080/providers/list

# 使用返回的 provider id 和 model 名称在 WebSocket 中调用 session.submit
```

### 发送 Webhook 事件

```bash
curl -X POST http://localhost:18080/webhook/events \
  -H "Authorization: Bearer your-webhook-token" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "custom-system",
    "event_type": "alert.critical",
    "content": "系统告警：CPU 使用率超过 90%",
    "session_key": "webhook:alerts:cpu"
  }'
```
