# Gateway HTTP API 文档

本文档描述 `klaw-gateway` 模块提供的 HTTP RESTful API 接口，供 WebUI 和第三方客户端调用。这些接口与 WebSocket v1 协议互补，提供文件归档、模型提供商查询、健康检查、监控指标等功能。

## 代码位置

- Gateway 路由与启动：`klaw-gateway/src/runtime.rs`
- 归档处理器：`klaw-gateway/src/archive.rs`
- 提供商处理器：`klaw-gateway/src/providers.rs`
- 健康检查与指标处理器：`klaw-gateway/src/handlers.rs`
- Webhook 处理器：`klaw-gateway/src/webhook.rs`
- Gateway 认证：`klaw-gateway/src/auth.rs`
- 静态资源与嵌入：`klaw-gateway/src/home.rs`、`klaw-gateway/src/chat_page.rs`、`klaw-gateway/src/embedded.rs`
- 归档模型与服务：`klaw-archive/src/model.rs`、`klaw-archive/src/service.rs`
- 健康注册表：`klaw-observability/src/health.rs`
- 路由常量：`klaw-gateway/src/routes.rs`
- OpenAPI/Scalar 文档：`klaw-gateway/src/openapi.rs`
- Gateway 配置结构：`klaw-config/src/lib.rs`

## 概述

- 基础路径：`http://<listen_ip>:<listen_port>`
- 响应格式：JSON（除文件下载和健康检查文本响应外）
- 认证方式：
  - **Gateway Auth**：保护 `/ws/chat`、`/archive/*`、`/providers/list`、`/mcp/*` 等路径，使用 `gateway.auth.token` 或 `gateway.auth.env_key` 配置的 Bearer Token。
  - **Webhook Auth**：保护 `/webhook/events` 和 `/webhook/agents`，支持多种验证模式（Bearer Token、GitHub HMAC、GitLab Token/签名），同样使用 `gateway.auth` 配置的密钥。
- API 文档：
  - `GET /openapi.json` 返回 Gateway HTTP API 的 OpenAPI JSON。
  - `GET /scalar` 返回 Scalar API reference UI。
  - 这两个端点始终不做 Gateway Auth，方便浏览器直接打开；生产暴露时请将其视为公开文档端点。

## 路由注册条件

部分路由仅在对应服务启用时注册：

- `/archive/*`：仅当 archive service 配置时注册
- `/providers/list`：仅当 providers state 配置时注册
- `/mcp/*`：仅当 MCP handler 配置时注册
- `/webhook/events`：仅当 `gateway.webhook.enabled = true` 且 `gateway.webhook.events.enabled = true` 时注册
- `/webhook/agents`：仅当 `gateway.webhook.enabled = true` 且 `gateway.webhook.agents.enabled = true` 时注册

`/openapi.json` 和 `/scalar` 始终注册。OpenAPI 首版覆盖 Gateway HTTP API；`/ws/chat` 只作为 WebSocket upgrade 入口描述，内部 JSON-RPC v1 协议仍以 WebSocket 协议文档为准。

未注册的路由返回 `404 Not Found`。

## API 端点列表

### 0. API 文档接口

#### 0.1 OpenAPI JSON

- **端点**：`GET /openapi.json`
- **认证**：无
- **描述**：返回 Gateway HTTP API 的 OpenAPI 3.1 JSON 文档，覆盖 archive、providers、MCP、webhook、health、metrics 和 `/ws/chat` upgrade 描述。

#### 0.2 Scalar API Reference

- **端点**：`GET /scalar`
- **认证**：无
- **描述**：返回 Scalar API reference UI。页面内嵌当前 OpenAPI spec，并标记 `/openapi.json` 作为机器可读文档地址。

### 1. 归档管理接口

归档系统用于存储和管理用户上传的文件附件，底层使用 SQLite + 文件存储。

#### 1.1 上传文件

- **端点**：`POST /archive/upload`
- **Content-Type**：`multipart/form-data`
- **认证**：Gateway Auth（`Authorization: Bearer <token>`）
- **描述**：上传文件到归档系统

**请求参数（multipart fields）：**

| 字段 | 类型 | 必填 | 描述 |
|------|------|------|------|
| `file` | File | 是 | 要上传的文件（支持任意 MIME 类型，服务端通过 MIME sniffing 自动检测） |
| `session_key` | string | 否 | 关联的会话键 |
| `channel` | string | 否 | 关联的通道 |
| `chat_id` | string | 否 | 关联的聊天 ID |
| `message_id` | string | 否 | 关联的消息 ID |

上传时 `source_kind` 自动设为 `user_upload`，`declared_mime_type` 不传入（由 MIME sniffing 检测）。

**响应示例（成功）：**

```json
{
  "success": true,
  "record": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "source_kind": "user_upload",
    "media_kind": "pdf",
    "mime_type": "application/pdf",
    "extension": "pdf",
    "original_filename": "report.pdf",
    "content_sha256": "a1b2c3d4...",
    "size_bytes": 1048576,
    "storage_rel_path": "2025/05/550e8400...",
    "session_key": "terminal:test",
    "channel": "terminal",
    "chat_id": "test-chat",
    "message_id": null,
    "metadata_json": "{}",
    "created_at_ms": 1714200000000
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
- `401 Unauthorized`：Gateway Auth 认证失败
- `500 Internal Server Error`：归档存储失败
- `503 Service Unavailable`：归档服务未配置

#### 1.2 下载文件

- **端点**：`GET /archive/download/{id}`
- **认证**：Gateway Auth
- **描述**：根据归档 ID 下载文件原始内容

**路径参数：**

| 参数 | 描述 |
|------|------|
| `id` | 归档记录 ID |

**响应：**

- 成功：文件二进制内容
- 响应头 `Content-Type` 为记录的 MIME 类型（缺失时回退 `application/octet-stream`）
- 响应头 `Content-Disposition: attachment; filename="<original_filename>"`（缺失时回退 `"download"`）

**状态码：**

- `200 OK`：下载成功
- `401 Unauthorized`：Gateway Auth 认证失败
- `404 Not Found`：文件不存在（响应体为 `{"error": "..."}`)
- `503 Service Unavailable`：归档服务未配置

#### 1.3 查询归档列表

- **端点**：`GET /archive/list`
- **认证**：Gateway Auth
- **描述**：分页查询归档记录列表

**查询参数：**

| 参数 | 类型 | 默认值 | 描述 |
|------|------|--------|------|
| `session_key` | string | - | 按会话键筛选 |
| `chat_id` | string | - | 按聊天 ID 筛选 |
| `source_kind` | string | - | 按来源类型筛选（`user_upload`、`channel_inbound`、`model_generated`） |
| `media_kind` | string | - | 按媒体类型筛选（`pdf`、`image`、`video`、`audio`、`other`） |
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
      "media_kind": "pdf",
      "mime_type": "application/pdf",
      "extension": "pdf",
      "original_filename": "report.pdf",
      "content_sha256": "a1b2c3d4...",
      "size_bytes": 1048576,
      "storage_rel_path": "2025/05/550e8400...",
      "session_key": "terminal:test",
      "channel": "terminal",
      "chat_id": "test-chat",
      "message_id": null,
      "metadata_json": "{}",
      "created_at_ms": 1714200000000
    }
  ],
  "error": null
}
```

**状态码：**

- `200 OK`：查询成功
- `401 Unauthorized`：Gateway Auth 认证失败
- `500 Internal Server Error`：查询失败
- `503 Service Unavailable`：归档服务未配置

#### 1.4 获取归档详情

- **端点**：`GET /archive/{id}`
- **认证**：Gateway Auth
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
    "media_kind": "pdf",
    "mime_type": "application/pdf",
    "extension": "pdf",
    "original_filename": "report.pdf",
    "content_sha256": "a1b2c3d4...",
    "size_bytes": 1048576,
    "storage_rel_path": "2025/05/550e8400...",
    "session_key": "terminal:test",
    "channel": "terminal",
    "chat_id": "test-chat",
    "message_id": null,
    "metadata_json": "{}",
    "created_at_ms": 1714200000000
  },
  "error": null
}
```

**响应示例（失败）：**

```json
{
  "success": false,
  "record": null,
  "error": "archive not found: ..."
}
```

**状态码：**

- `200 OK`：查询成功
- `401 Unauthorized`：Gateway Auth 认证失败
- `404 Not Found`：归档记录不存在
- `503 Service Unavailable`：归档服务未配置

#### ArchiveRecord 字段说明

| 字段 | 类型 | 描述 |
|------|------|------|
| `id` | string | 归档记录 UUID |
| `source_kind` | string | 来源类型：`user_upload`、`channel_inbound`、`model_generated` |
| `media_kind` | string | 媒体类型：`pdf`、`image`、`video`、`audio`、`other` |
| `mime_type` | string? | MIME 类型（可能为 null） |
| `extension` | string? | 文件扩展名 |
| `original_filename` | string? | 原始文件名 |
| `content_sha256` | string | 文件内容 SHA-256 哈希 |
| `size_bytes` | integer | 文件大小（字节） |
| `storage_rel_path` | string | 存储相对路径 |
| `session_key` | string? | 关联会话键 |
| `channel` | string? | 关联通道 |
| `chat_id` | string? | 关联聊天 ID |
| `message_id` | string? | 关联消息 ID |
| `metadata_json` | string | 元数据 JSON 字符串 |
| `created_at_ms` | integer | 创建时间戳（毫秒） |

### 2. 模型提供商接口

#### 2.1 获取提供商列表

- **端点**：`GET /providers/list`
- **认证**：Gateway Auth
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
      "name": null,
      "base_url": "https://api.openai.com/v1",
      "wire_api": "chat_completions",
      "default_model": "gpt-4o-mini",
      "stream": false,
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
| `id` | string | 提供商唯一标识（配置中的 key） |
| `name` | string? | 提供商显示名称（可选） |
| `base_url` | string | API 基础 URL |
| `wire_api` | string | API 协议类型（`chat_completions` 或 `messages`） |
| `default_model` | string | 默认模型 |
| `stream` | boolean | 是否启用流式响应 |
| `has_api_key` | boolean | 是否已配置 API 密钥（检查 `api_key` 直接值或 `env_key` 环境变量） |

`default_provider` 为可选字段，可能为 `null`（当服务未配置时）。

**状态码：**

- `200 OK`：查询成功
- `401 Unauthorized`：Gateway Auth 认证失败
- `503 Service Unavailable`：提供商服务未配置

### 3. MCP 管理接口

MCP 管理接口用于查看和管理 `mcp.servers` 配置，并将配置变更同步到运行中的 MCP manager。所有接口使用 Gateway Auth；当 `gateway.auth.enabled = false` 时不做认证检查。响应中的 server 配置默认脱敏，只返回 `env_keys` 和 `header_keys`，不返回 secret 原文。

#### 3.1 获取 MCP runtime 状态

- **端点**：`GET /mcp/status`
- **认证**：Gateway Auth
- **描述**：返回 MCP server 运行状态和最近一次 `tools/list` detail

**响应示例：**

```json
{
  "success": true,
  "runtime": {
    "statuses": [
      {
        "id": "filesystem",
        "mode": "stdio",
        "enabled": true,
        "state": "running",
        "last_error": null,
        "tool_count": 3
      }
    ],
    "details": [
      {
        "id": "filesystem",
        "tools_list_response": {
          "tools": [
            {
              "name": "read_file",
              "description": "Read a file",
              "inputSchema": {"type": "object"}
            }
          ]
        }
      }
    ]
  },
  "error": null
}
```

#### 3.2 列出 MCP server 配置

- **端点**：`GET /mcp/servers`
- **认证**：Gateway Auth
- **描述**：返回脱敏后的 MCP server 配置列表和 runtime snapshot

**响应示例：**

```json
{
  "success": true,
  "servers": [
    {
      "id": "filesystem",
      "enabled": true,
      "mode": "stdio",
      "tool_timeout_seconds": 60,
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem"],
      "env_keys": ["API_KEY"],
      "header_keys": []
    }
  ],
  "runtime": {
    "statuses": [],
    "details": []
  },
  "error": null
}
```

#### 3.3 获取单个 MCP server

- **端点**：`GET /mcp/servers/{id}`
- **认证**：Gateway Auth
- **描述**：返回单个脱敏配置、对应 status 和 detail

#### 3.4 新增 MCP server

- **端点**：`POST /mcp/servers`
- **认证**：Gateway Auth
- **描述**：新增 MCP server 配置，保存后立即同步 MCP runtime

**请求体：**

```json
{
  "id": "filesystem",
  "enabled": true,
  "mode": "stdio",
  "tool_timeout_seconds": 60,
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem"],
  "env": {"API_KEY": "secret"},
  "headers": {}
}
```

`enabled` 默认 `true`，`tool_timeout_seconds` 默认 `60`，`env` / `headers` 默认 `{}`。

#### 3.5 替换 MCP server

- **端点**：`PUT /mcp/servers/{id}`
- **认证**：Gateway Auth
- **描述**：替换 path id 对应的 MCP server 配置，保存后立即同步 MCP runtime。body 中 `id` 可与 path id 不同，用于重命名。

`PUT` 省略 `env` 或 `headers` 时保留旧 map；显式发送 `{}` 时清空对应 map。

#### 3.6 删除 MCP server

- **端点**：`DELETE /mcp/servers/{id}`
- **认证**：Gateway Auth
- **描述**：删除 MCP server 配置，保存后立即同步 MCP runtime

#### 3.7 同步 MCP runtime

- **端点**：`POST /mcp/sync`
- **认证**：Gateway Auth
- **描述**：按磁盘最新配置同步 MCP manager

#### 3.8 重启 MCP server

- **端点**：`POST /mcp/servers/{id}/restart`
- **认证**：Gateway Auth
- **描述**：重启已启用的 stdio MCP server。SSE server 和 disabled server 返回错误。

**状态码：**

- `200 OK`：操作成功
- `400 Bad Request`：请求非法或 runtime 拒绝操作
- `401 Unauthorized`：Gateway Auth 认证失败
- `404 Not Found`：server 不存在
- `409 Conflict`：server id 重复
- `503 Service Unavailable`：MCP handler 未配置或 manager 忙

### 4. Webhook 事件接口

Webhook 认证使用 `WebhookAuth`，支持多种验证模式。当 `gateway.auth.enabled = true` 时，webhook 认证启用，使用 `gateway.auth` 配置的密钥作为共享 secret。

#### 4.1 发送结构化事件

- **端点**：`POST /webhook/events`
- **认证**：Webhook Auth（Bearer Token、GitHub HMAC SHA-256/SHA-1、GitLab Token/Signature）
- **描述**：向系统发送结构化 webhook 事件
- **Body 大小限制**：由 `gateway.webhook.events.max_body_bytes` 控制（默认 `262144` = 256 KiB）

**请求体：**

```json
{
  "source": "github",
  "event_type": "issue_comment.created",
  "content": "PR #42 收到新的 review comment",
  "base_session_key": "webhook:github:42",
  "session_key": "webhook:github:42",
  "chat_id": "repo-42",
  "sender_id": "github:webhook",
  "payload": {"number": 42},
  "metadata": {"repo": "openclaw/klaw"}
}
```

**请求字段说明：**

| 字段 | 类型 | 必填 | 描述 |
|------|------|------|------|
| `source` | string | 是 | 事件来源标识（如 `github`、`gitlab`、`custom-system`），不可为空 |
| `event_type` | string | 是 | 事件类型（如 `issue_comment.created`、`alert.critical`），不可为空 |
| `content` | string | 是 | 事件正文内容，不可为空 |
| `base_session_key` | string? | 否 | 基础会话键（用于恢复已有会话），优先级高于 `session_key` |
| `session_key` | string? | 否 | 会话键（当提供时作为 `base_session_key` 的回退） |
| `chat_id` | string? | 否 | 聊天 ID（缺失时自动生成） |
| `sender_id` | string? | 否 | 发送者 ID（缺失时自动生成 `{source}:webhook`） |
| `payload` | object? | 否 | 附加 JSON payload |
| `metadata` | object? | 否 | 附加元数据 |

**请求规范化行为：**

服务端收到请求后执行规范化：

- `event_id`：自动生成 UUID
- `session_key`：自动生成 `webhook:{source}:{uuid}`（不使用客户端传入的值，客户端传入的值映射到 `base_session_key`）
- `base_session_key`：优先取客户端的 `base_session_key`，其次取 `session_key`，均为空时为 null
- `chat_id`：缺失时回退为自动生成的 `session_key`
- `sender_id`：缺失时回退为 `{source}:webhook`
- `received_at_ms`：自动填充当前时间戳
- `metadata`：自动注入 `trigger.kind=webhook`、`webhook.source`、`webhook.event_type`、`webhook.event_id`；若有 `base_session_key`，也注入 `webhook.base_session_key`

**响应示例（成功）：**

```json
{
  "event_id": "2f4e6f1c-8d8d-4b4f-a45e-2f9a71e84384",
  "status": "accepted",
  "session_key": "webhook:github:2f4e6f1c-..."
}
```

**状态码：**

- `202 Accepted`：事件已受理
- `400 Bad Request`：请求体非法（JSON 解析失败、`source`/`event_type`/`content` 为空）
- `401 Unauthorized`：Webhook Auth 认证失败
- `404 Not Found`：webhook 未启用
- `413 Payload Too Large`：请求体超过 `max_body_bytes` 限制

#### 4.2 Agent Webhook 调用

- **端点**：`POST /webhook/agents`
- **认证**：Webhook Auth（同 `/webhook/events`）
- **描述**：调用配置的 Agent webhook 模板
- **Body 大小限制**：由 `gateway.webhook.agents.max_body_bytes` 控制（默认 `262144` = 256 KiB）

**查询参数：**

| 参数 | 必填 | 描述 |
|------|------|------|
| `hook_id` | 是 | Agent Hook 标识（仅允许字母、数字、`-`、`_`） |
| `base_session_key` | 否 | 基础会话键（用于恢复已有会话），优先级高于 `session_key` |
| `session_key` | 否 | 会话键（作为 `base_session_key` 的回退） |
| `chat_id` | 否 | 聊天 ID |
| `sender_id` | 否 | 发送者 ID |
| `provider` | 否 | 指定模型提供商 |
| `model` | 否 | 指定模型 |

**请求体：** 任意 JSON 对象，作为 Agent 的输入 `body`

**请求规范化行为：**

- `request_id`：自动生成 UUID
- `session_key`：自动生成 `webhook:{hook_id}:{uuid}`
- `base_session_key`：优先取 `base_session_key`，其次取 `session_key`，均为空时为 null
- `chat_id`：缺失时回退为自动生成的 `session_key`
- `sender_id`：缺失时回退为 `webhook-agent:{hook_id}`
- `received_at_ms`：自动填充当前时间戳
- `metadata`：自动注入 `trigger.kind=webhook_agents`、`webhook.kind=agents`、`webhook.agents.hook_id`、`webhook.agents.request_id`；若有 `base_session_key`，也注入 `webhook.base_session_key`

**响应示例（成功）：**

```json
{
  "request_id": "req-uuid",
  "status": "accepted",
  "hook_id": "order",
  "session_key": "webhook:order:req-uuid"
}
```

**状态码：**

- `202 Accepted`：请求已受理
- `400 Bad Request`：参数错误（`hook_id` 为空或含非法字符、JSON 解析失败）
- `401 Unauthorized`：Webhook Auth 认证失败
- `404 Not Found`：webhook agents 未启用

### 5. 健康检查接口

健康检查基于 `HealthRegistry`，注册组件后各组件可独立报告状态。

#### 5.1 存活检查 (Liveness)

- **端点**：`GET /health/live`
- **描述**：检查服务是否存活
- **认证**：无

**响应：**

```
Live
```

或

```
Unavailable
```

任何组件 `Unavailable` 时整体 liveness 返回 `Unavailable`。无注册组件时默认 `Live`。

**状态码：**

- `200 OK`：服务存活（`Live`）
- `503 Service Unavailable`：服务不可用（`Unavailable`）

#### 5.2 就绪检查 (Readiness)

- **端点**：`GET /health/ready`
- **描述**：检查服务是否就绪（可以接收请求）
- **认证**：无

**响应：**

```
Ready
```

或

```
Degraded
```

或

```
Unavailable
```

任何组件 `Unavailable` 时整体 readiness 返回 `Unavailable`；有组件 `Degraded` 但无 `Unavailable` 时返回 `Degraded`；全部健康时返回 `Ready`。无注册组件时默认 `Ready`。

**状态码：**

- `200 OK`：服务就绪（`Ready` 或 `Degraded`）
- `503 Service Unavailable`：服务未就绪（`Unavailable`）

#### 5.3 综合状态检查

- **端点**：`GET /health/status`
- **描述**：获取详细的健康状态信息（JSON）
- **认证**：无

**响应示例：**

```json
{
  "status": "Ready",
  "components": [
    {
      "name": "gateway",
      "status": "Ready",
      "message": null
    },
    {
      "name": "provider",
      "status": "Degraded",
      "message": "api key expired"
    }
  ]
}
```

`status` 取值：`Ready`、`Live`、`Degraded`、`Unavailable`。`message` 为可选字符串，可能为 `null`。

**状态码：**

- `200 OK`：始终返回当前状态信息（即使状态为 `Unavailable`）

### 6. 监控指标接口

#### 6.1 Prometheus 指标

- **端点**：`GET /metrics`
- **描述**：获取 Prometheus 格式的监控指标
- **Content-Type**：`text/plain; version=0.0.4; charset=utf-8`
- **认证**：无

**状态码：**

- `200 OK`：返回指标数据
- `404 Not Found`：Prometheus 指标未启用（响应体为 `Prometheus metrics not enabled\n`）
- `500 Internal Server Error`：指标渲染失败

### 7. WebUI 与静态资源

所有静态资源使用 `RustEmbed` 编译时嵌入，从 `static/` 目录打包。

#### 7.1 聊天页面

- **端点**：`GET /chat`
- **描述**：WebUI 聊天应用主页面
- **Content-Type**：`text/html; charset=utf-8`
- **认证**：无（页面本身无需认证，但 WebSocket `/ws/chat` 连接需要 Gateway Auth）

#### 7.2 WebUI JS 文件

- **端点**：`GET /chat/dist/klaw_webui.js`
- **描述**：WebUI JavaScript 主文件
- **Content-Type**：`application/javascript; charset=utf-8`
- **Cache-Control**：`public, max-age=3600`

#### 7.3 WebUI WASM 文件

- **端点**：`GET /chat/dist/klaw_webui_bg.wasm`
- **描述**：WebUI WebAssembly 文件
- **Content-Type**：`application/wasm`
- **Cache-Control**：`public, max-age=3600`

#### 7.4 首页与其他资源

| 端点 | Content-Type | Cache-Control | 描述 |
|------|-------------|---------------|------|
| `GET /` | `text/html; charset=utf-8` | 无 | 网关首页 |
| `GET /logo.webp` | `image/webp` | `public, max-age=86400` | 网关 Logo |
| `GET /favicon.ico` | `image/x-icon` | `public, max-age=86400` | 网站图标 |
| `GET /images/{filename}` | 按扩展名推断 | `public, max-age=86400` | 图片资源 |

`/images/{filename}` 支持的扩展名与 Content-Type 映射：`.png` → `image/png`、`.jpg`/`.jpeg` → `image/jpeg`、`.webp` → `image/webp`、`.gif` → `image/gif`、`.svg` → `image/svg+xml`、`.ico` → `image/x-icon`、其他 → `application/octet-stream`。

未找到的嵌入资源返回 `404 Not Found`。

## 认证机制

### Gateway Auth

Gateway Auth 使用 `gateway.auth` 配置的密钥保护关键路由：

```toml
[gateway.auth]
enabled = true
token = "secret-token"
env_key = "KLAW_GATEWAY_AUTH_TOKEN"
```

- `token` 直接指定密钥，`env_key` 指定环境变量名。优先使用 `token`，其次从环境变量读取。
- 保护的路由：`/ws/chat`、`/archive/upload`、`/archive/list`、`/archive/download/{id}`、`/archive/{id}`、`/providers/list`、`/mcp/*`
- 认证方式：`Authorization: Bearer <token>`；`/ws/chat` 还接受 `?token=` 或 `?access_token=` query 参数（浏览器 WebSocket 兼容）
- 未启用时不做认证检查

### Webhook Auth

Webhook Auth 使用 `WebhookAuth` 多验证器链，同样使用 `gateway.auth` 配置的密钥作为共享 secret：

| 验证器 | Header | 描述 |
|--------|--------|------|
| Bearer Token | `Authorization: Bearer <secret>` | 直接 Bearer token 验证 |
| GitHub HMAC SHA-256 | `X-Hub-Signature-256: sha256=<hex>` | GitHub webhook HMAC-SHA256 签名 |
| GitHub HMAC SHA-1 | `X-Hub-Signature: sha1=<hex>` | GitHub webhook HMAC-SHA1 签名 |
| GitLab Token | `X-Gitlab-Token: <secret>` | GitLab webhook token |
| GitLab Signature | `X-Gitlab-Signature: <hex>` | GitLab webhook HMAC 签名 |

验证器按顺序尝试，第一个匹配的返回成功并附带 `mode` 标记（如 `"bearer"`、`"github_sha256"` 等）。全部不匹配时返回 `401 Unauthorized`。

当 `gateway.auth.enabled = false` 时，Webhook Auth 跳过验证（mode 返回 `"disabled"`）。

## 错误处理

### JSON 响应格式

归档和提供商接口使用统一 JSON 格式：

```json
{
  "success": false,
  "error": "人类可读的错误描述"
}
```

注意：JSON 错误响应不含 `error_code` 字段。

### Webhook 错误

Webhook 认证失败返回纯文本响应：

```
missing or invalid webhook authentication header
```

请求规范化失败返回纯文本：

```
source is required
```

### 文件下载错误

归档下载 404 返回 JSON：

```json
{"error": "archive not found: ..."}
```

### 健康检查错误

健康检查使用纯文本响应，不带 JSON 包装。

## 请求体大小限制

- 全局默认 body 限制：`100 MiB`（`DefaultBodyLimit::max(100 * 1024 * 1024)`）
- Webhook events：`gateway.webhook.events.max_body_bytes`（默认 `262144` = 256 KiB）
- Webhook agents：`gateway.webhook.agents.max_body_bytes`（默认 `262144` = 256 KiB）

## 使用示例

### 上传文件并发送消息

```bash
# 1. 上传文件
curl -X POST http://localhost:18080/archive/upload \
  -H "Authorization: Bearer secret-token" \
  -F "file=@document.pdf" \
  -F "session_key=my-session"

# 响应: {"success":true,"record":{"id":"archive-123","source_kind":"user_upload","media_kind":"pdf",...}}

# 2. 通过 WebSocket v1 发送消息引用该文件
# 使用 turn/start 方法，content blocks 包含 attachment:
# {
#   "id": "...",
#   "method": "turn/start",
#   "params": {
#     "session_id": "my-session",
#     "thread_id": "my-session",
#     "input": [
#       {"type": "text", "text": "请分析这个文档"},
#       {"type": "attachment", "archive_id": "archive-123", "mime_type": "application/pdf", "size_bytes": 1048576}
#     ]
#   }
# }
```

### 查询提供商并切换模型

```bash
# 获取提供商列表
curl -H "Authorization: Bearer secret-token" http://localhost:18080/providers/list

# 使用返回的 provider id 和 model 名称在 WebSocket v1 中调用 turn/start
```

### 发送 Webhook 事件（Bearer Token）

```bash
curl -X POST http://localhost:18080/webhook/events \
  -H "Authorization: Bearer secret-token" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "custom-system",
    "event_type": "alert.critical",
    "content": "系统告警：CPU 使用率超过 90%",
    "base_session_key": "webhook:alerts:cpu",
    "chat_id": "alerts-channel"
  }'
```

### 发送 GitHub Webhook 事件（HMAC 签名）

```bash
# GitHub 自动发送带 X-Hub-Signature-256 的请求
# Gateway 使用 shared secret 验证 HMAC 签名
```

### 调用 Agent Webhook

```bash
curl -X POST "http://localhost:18080/webhook/agents?hook_id=order&base_session_key=dingtalk:acc:chat-1" \
  -H "Authorization: Bearer secret-token" \
  -H "Content-Type: application/json" \
  -d '{"order_id":"A123","status":"paid","amount":100}'
```

## 验证入口

维护本接口时至少运行：

```bash
cargo test -p klaw-gateway --lib
cargo test -p klaw-gateway --test protocol_v1
cargo test -p klaw-archive --lib
mdbook build docs
```
