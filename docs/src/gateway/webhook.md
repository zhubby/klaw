# Webhook 事件输入

本文档说明 Gateway Webhook 端点的配置、请求格式、鉴权机制以及处理流程。

## 功能概述

Webhook 现在提供两类 HTTP 输入端点：

- `POST /webhook/events`：结构化事件入口，适合已经完成归一化的外部事件
- `POST /webhook/agents`：模板驱动入口，适合任意 JSON `body` 结合本地 markdown hook 模板直接注入 agent loop

两类入口都遵循同一条运行时语义：

- 每次 webhook 请求都会启动一轮独立的 agent loop
- webhook 自己使用独立的 `webhook:*` 执行 session，不继承 IM 会话上下文
- 若提供 `base_session_key`，运行时会把最终回复投递到该 base session 当前 active session 对应的 channel/chat

主要特性：

- **双 HTTP POST 端点** - 覆盖事件归一化与模板驱动两类场景
- **Bearer Token 鉴权** - 安全的访问控制
- **异步处理** - 快速响应，后台执行
- **持久化记录** - 请求历史可追溯

## 配置

Webhook 配置位于 `gateway.webhook` 段：

```toml
[gateway]
enabled = true
listen_ip = "127.0.0.1"
listen_port = 0

[gateway.webhook]
enabled = false

[gateway.webhook.events]
enabled = true
path = "/webhook/events"
max_body_bytes = 262144

[gateway.webhook.agents]
enabled = false
path = "/webhook/agents"
max_body_bytes = 262144

[gateway.tls]
enabled = false
cert_path = "/path/to/fullchain.pem"
key_path = "/path/to/privkey.pem"
```

### 配置字段说明

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `gateway.webhook.enabled` | bool | `false` | 是否启用 webhook 输入能力 |
| `gateway.webhook.events.enabled` | bool | `true` | 是否启用结构化事件入口 |
| `gateway.webhook.events.path` | String | `/webhook/events` | 结构化事件入口路径 |
| `gateway.webhook.events.max_body_bytes` | u64 | `262144` | 结构化事件入口请求体大小限制 |
| `gateway.webhook.agents.enabled` | bool | `false` | 是否启用模板驱动入口 |
| `gateway.webhook.agents.path` | String | `/webhook/agents` | agent webhook 入口路径 |
| `gateway.webhook.agents.max_body_bytes` | u64 | `262144` | agent webhook 入口请求体大小限制 |

### 配置校验规则

- `events.path` 与 `agents.path` 都必须以 `/` 开头
- `events.max_body_bytes` 与 `agents.max_body_bytes` 都必须大于 `0`
- `events.path` 与 `agents.path` 不能相同
- `tls.enabled = true` 时，`cert_path` 和 `key_path` 不能为空

## 请求格式

### `/webhook/events`

```
POST /webhook/events HTTP/1.1
Host: 127.0.0.1:18080
Authorization: Bearer <token>
Content-Type: application/json
Content-Length: 234

{
  "source": "github",
  "event_type": "issue_comment.created",
  "content": "PR #42 收到新的 review comment",
  "payload": {
    "number": 42,
    "comment_id": 123456
  }
}
```

### `/webhook/events` JSON Payload 结构

```json
{
  "source": "github",
  "event_type": "issue_comment.created",
  "content": "事件描述文本",
  "base_session_key": "dingtalk:acc:chat-1",
  "payload": {
    // 任意 JSON 对象
  }
}
```

| 字段 | 类型 | 必需 | 说明 |
|------|------|------|------|
| `source` | String | 是 | 事件来源标识（如 "github", "gitlab", "custom"） |
| `event_type` | String | 是 | 事件类型（如 "push", "issue.created"） |
| `content` | String | 是 | 事件描述，会作为消息内容 |
| `base_session_key` | String | 否 | 回复投递目标；运行时会解析该 base session 当前 active session |
| `session_key` | String | 否 | 兼容旧字段，等价于 `base_session_key`，后续会移除 |
| `payload` | Object | 否 | 扩展数据，任意 JSON 对象 |

### `/webhook/agents`

```http
POST /webhook/agents?hook_id=order&base_session_key=dingtalk%3Aacc%3Achat-1&provider=openai&model=gpt-4.1 HTTP/1.1
Authorization: Bearer <token>
Content-Type: application/json

{
  "order_id": "A123",
  "status": "paid"
}
```

字段规则：

- URL query:
  - `hook_id`：必填，对应 `(<storage.root_dir 或 ~/.klaw>)/hooks/prompts/<hook_id>.md`
  - `base_session_key`：可选，指定回复投递目标；运行时会先解析到 active session
  - `session_key`：兼容旧字段，等价于 `base_session_key`
  - `provider` / `model`：可选，仅作用于当前请求
  - `chat_id` / `sender_id`：可选
- HTTP body:
  - 原封不动接受任意 JSON value，并作为 request JSON 追加到模板后面

agent 请求会读取本地 markdown hook 模板，并在末尾统一追加 pretty-printed request JSON fenced code block，最终内容再注入独立的 webhook agent loop。

### 响应格式

**成功响应**：

```
HTTP/1.1 202 Accepted
Content-Type: application/json

{
  "status": "accepted",
  "message": "Webhook event received and queued for processing"
}
```

**错误响应**：

```
HTTP/1.1 401 Unauthorized
Content-Type: application/json

{
  "error": "Unauthorized",
  "message": "Missing or invalid Authorization header"
}
```

```
HTTP/1.1 400 Bad Request
Content-Type: application/json

{
  "error": "InvalidRequest",
  "message": "Request body exceeds maximum size"
}
```

## 鉴权机制

### Bearer Token 验证

所有 webhook 请求必须包含 `Authorization` 头：

```http
Authorization: Bearer <token>
```

验证流程：

1. 检查 `Authorization` 头是否存在
2. 解析 Bearer token
3. 与配置的 token 比对
4. 验证通过后处理请求

### 验证失败响应

| 状态码 | 原因 |
|--------|------|
| `401 Unauthorized` | 缺少 Authorization 头 |
| `401 Unauthorized` | Token 格式错误 |
| `401 Unauthorized` | Token 不匹配 |
| `403 Forbidden` | 请求方法不允许 |

## 事件处理流程

```
┌──────────────┐
│ 外部系统     │
│ (GitHub等)   │
└──────┬───────┘
       │ HTTP POST
       ▼
┌──────────────────────────────────┐
│ Gateway Webhook Handler          │
│ 1. Bearer token 鉴权             │
│ 2. 请求体大小检查                │
│ 3. JSON 解析                     │
│ 4. 返回 202 Accepted             │
└──────┬───────────────────────────┘
       │
       ▼
┌──────────────────────────────────┐
│ 异步处理队列                     │
│ 1. 构建 WebhookRequest          │
│ 2. 持久化到数据库                │
│ 3. 发布到 inbound transport      │
└──────┬───────────────────────────┘
       │
       ▼
┌──────────────────────────────────┐
│ Runtime 处理                     │
│ 1. 转换为 InboundMessage         │
│ 2. Agent 处理                    │
│ 3. 生成响应                      │
└──────┬───────────────────────────┘
       │
       ▼
┌──────────────────────────────────┐
│ 数据库记录                       │
│ - webhook_request 表             │
│ - 状态跟踪                       │
│ - 结果摘要                       │
└──────────────────────────────────┘
```

### 处理步骤详解

1. **HTTP 接收**
   - 接收 POST 请求
   - 验证 Bearer token
   - 检查请求体大小

2. **立即响应**
   - 验证通过后立即返回 `202 Accepted`
   - 客户端无需等待处理完成

3. **异步处理**
   - 构建标准化 `WebhookRequest` 对象
   - 持久化到 `webhook_request` 表
   - 发布到 `agent.inbound` transport

4. **Agent 执行**
   - Runtime 接收消息
   - Agent 处理事件
   - 生成响应

5. **状态更新**
   - 更新处理状态
   - 记录结果摘要

## GUI Webhook 面板

### 功能特性

| 功能 | 描述 |
|------|------|
| **请求列表** | 显示所有 webhook 请求历史 |
| **状态查看** | 显示处理状态 (pending/success/failed) |
| **详情查看** | 查看请求内容和响应结果 |
| **过滤功能** | 按状态、时间范围过滤 |

### 面板字段

| 字段 | 说明 |
|------|------|
| Time | 请求时间 |
| Source | 事件来源 |
| Event Type | 事件类型 |
| Status | 处理状态 |
| Content | 事件描述 |

## 使用示例

### GitHub Webhook 集成

**配置 GitHub Webhook**：

1. 进入 GitHub Repository Settings -> Webhooks
2. 添加 webhook URL：`http://your-server:18080/webhook/events`
3. Content type 选择 `application/json`
4. 设置 Secret（即 Bearer token）
5. 选择触发事件

**处理 GitHub Push 事件**：

```json
{
  "source": "github",
  "event_type": "push",
  "content": "New push to main branch by user@example.com",
  "payload": {
    "ref": "refs/heads/main",
    "repository": {
      "name": "my-project",
      "full_name": "org/my-project"
    },
    "pusher": {
      "name": "user",
      "email": "user@example.com"
    },
    "commits": [
      {
        "id": "abc123",
        "message": "Fix bug in auth module"
      }
    ]
  }
}
```

### GitLab Webhook 集成

```json
{
  "source": "gitlab",
  "event_type": "merge_request",
  "content": "Merge request !42 opened",
  "payload": {
    "object_kind": "merge_request",
    "project": {
      "name": "my-project"
    },
    "object_attributes": {
      "iid": 42,
      "title": "Add new feature",
      "state": "opened"
    }
  }
}
```

### 自定义事件

```bash
# 使用 curl 发送自定义事件
curl -X POST http://127.0.0.1:18080/webhook/events \
  -H 'Authorization: Bearer my-secret-token' \
  -H 'Content-Type: application/json' \
  -d '{
    "source": "monitoring",
    "event_type": "alert.fired",
    "content": "CPU usage exceeded 90% on production server",
    "payload": {
      "server": "prod-01",
      "cpu_percent": 92,
      "timestamp": "2024-01-15T10:30:00Z"
    }
  }'
```

### Rust 客户端示例

```rust
use reqwest::Client;
use serde_json::json;

async fn send_webhook_event() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    
    let payload = json!({
        "source": "rust-app",
        "event_type": "task.completed",
        "content": "Background task finished successfully",
        "payload": {
            "task_id": "task-123",
            "duration_ms": 5000
        }
    });
    
    let response = client
        .post("http://127.0.0.1:18080/webhook/events")
        .header("Authorization", "Bearer my-secret-token")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;
    
    println!("Response status: {}", response.status());
    
    Ok(())
}
```

完整示例代码位于：

- `klaw-gateway/examples/webhook_request.rs`
- `klaw-gateway/examples/webhook_agents_request.rs`

```bash
# events 示例：使用默认配置
cargo run -p klaw-gateway --example webhook_request

# events 示例：使用自定义配置
WEBHOOK_TOKEN=my-token BASE_URL=http://127.0.0.1:18080 \
  cargo run -p klaw-gateway --example webhook_request

# agents 示例：使用默认配置
cargo run -p klaw-gateway --example webhook_agents_request

# agents 示例：使用自定义配置
WEBHOOK_TOKEN=my-token BASE_URL=http://127.0.0.1:18080 \
  cargo run -p klaw-gateway --example webhook_agents_request
```

## 安全注意事项

### Token 管理

- **不要硬编码 token**：使用环境变量或配置文件
- **定期轮换 token**：建议每隔一段时间更换
- **限制 token 权限**：仅用于 webhook 鉴权

### HTTPS 推荐

生产环境强烈建议启用 HTTPS：

```toml
[gateway.tls]
enabled = true
cert_path = "/etc/letsencrypt/live/example.com/fullchain.pem"
key_path = "/etc/letsencrypt/live/example.com/privkey.pem"
```

### 请求大小限制

默认限制为 256KB，可根据需要调整：

```toml
[gateway.webhook]
max_body_bytes = 524288  # 512KB
```

### 网络隔离

建议将 Gateway 部署在内网，通过反向代理（如 Nginx）暴露：

```
Internet -> Nginx (HTTPS) -> Gateway (HTTP, 内网)
```

### 日志审计

所有 webhook 请求都会被记录，包括：

- 请求时间
- 来源 IP（通过反向代理传递）
- 事件类型
- 处理状态

## 错误排查

### 401 Unauthorized

**原因**：

- 缺少 `Authorization` 头
- Token 格式错误（不是 `Bearer <token>`）
- Token 不匹配

**解决**：

```bash
# 检查 token 配置
grep -A5 "webhook" ~/.klaw/config.toml

# 或检查环境变量
echo $KLAW_GATEWAY_WEBHOOK_TOKEN
```

### 400 Bad Request

**原因**：

- JSON 格式错误
- 缺少必需字段
- 请求体超过大小限制

**解决**：

- 使用 JSON 格式验证工具
- 检查 `source`、`event_type`、`content` 字段
- 减小请求体大小或增加 `max_body_bytes`

### 404 Not Found

**原因**：

- Webhook 未启用
- 路径配置错误

**解决**：

```toml
[gateway.webhook]
enabled = true
path = "/webhook/events"
```

### 请求处理失败

**原因**：

- Runtime 未启动
- 数据库连接失败
- Agent 处理错误

**解决**：

- 查看 GUI Logs 面板
- 查看 Webhook 面板的错误信息
- 检查数据库状态

## 当前限制

- 仅支持 `Authorization: Bearer <token>` 鉴权，不支持自定义认证头
- TLS 仅有配置模型，尚未实现 HTTPS/WSS 监听
- 请求状态为进程内内存结构，重启后不保留执行状态

## 后续演进

- 支持自定义认证头
- 接入 `rustls` 实现 HTTPS
- 增加 replay / 重跑等运维能力
- 支持跨实例共享后端（如 Redis pub/sub）
- 增加请求签名验证（如 HMAC）

## 相关文档

- [Gateway 概述](./README.md) - Gateway 整体架构
- [WebSocket](./websocket.md) - WebSocket 连接管理
- [配置概述](../configration/overview.md) - 配置模型
