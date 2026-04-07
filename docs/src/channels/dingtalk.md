# DingTalk 渠道设计与实现

本文档记录 `klaw-channel` 中 DingTalk（钉钉）渠道的实现：WebSocket 长连接、消息事件处理、基于共享 IM 卡片模型的审批交互、媒体素材归档与语音转写。

## 目标

- 基于钉钉开放平台 WebSocket 长连接协议，实现双向消息通道
- 支持文本、图片、语音、视频、文件、富文本等多种消息类型的解析与响应
- 集成审批交互卡片，支持 approve/reject 决策回调
- 支持媒体素材自动下载、归档与语音转写（ASR）
- 事件去重、发送者白名单、代理支持等安全控制

## 共享卡片抽象

- DingTalk 出站审批消息优先消费 `ChannelResponse.metadata["im.card"]`
- 若上游仍只提供旧字段，渠道会兼容 `approval.id`、`approval.signal.approval_id`，以及正文里的 `approval_id=...` / 自然语言审批 ID 回退
- 解析出的共享审批卡片会映射为钉钉 `actionCard`，按钮点击后再归一成 `/approve <id>` 或 `/reject <id>` 提交回 runtime

## 代码位置

- 渠道实现：`klaw-channel/src/dingtalk.rs`
- 运行时注册：`klaw-cli/src/commands/dingtalk_runtime.rs`
- 配置模型：`klaw-config/src/lib.rs`（`channels.dingtalk`）

## 配置模型

在 `~/.klaw/config.toml` 中配置：

```toml
[[channels.dingtalk]]
id = "default"
enabled = true
client_id = "your-app-key"
client_secret = "your-app-secret"
bot_title = "Klaw"
show_reasoning = false
allowlist = ["USER123", "*"]

[channels.dingtalk.proxy]
enabled = false
url = "http://proxy.example.com:8080"
```

配置字段说明：

| 字段 | 类型 | 必填 | 描述 | 默认值 |
|------|------|------|------|--------|
| `id` | `string` | 是 | 渠道账户标识，用于生成 `session_key` | `"default"` |
| `enabled` | `bool` | 否 | 是否启用 | `true` |
| `client_id` | `string` | 是 | 钉钉应用 AppKey | - |
| `client_secret` | `string` | 是 | 钉钉应用 AppSecret | - |
| `bot_title` | `string` | 否 | 机器人显示名称 | `"Klaw"` |
| `show_reasoning` | `bool` | 否 | 响应中是否展示推理过程 | `false` |
| `allowlist` | `string[]` | 否 | 发送者白名单（`*` 表示允许所有） | `[]`（允许所有） |
| `proxy.enabled` | `bool` | 否 | 是否启用代理 | `false` |
| `proxy.url` | `string` | 条件必填 | 代理地址 | - |

配置校验：
- `client_id` 和 `client_secret` 不能为空
- `proxy.enabled=true` 时 `proxy.url` 必填

## 连接建立流程

DingTalk 渠道使用 WebSocket 长连接协议，连接建立流程如下：

```
1. 调用 /v1.0/gateway/connections/open 获取 WebSocket 接入点
   请求体：
   {
     "clientId": "<client_id>",
     "clientSecret": "<client_secret>",
     "subscriptions": [{
       "type": "CALLBACK",
       "topic": "/v1.0/im/bot/messages/get"
     }],
     "ua": "klaw/dingtalk"
   }

   响应：
   {
     "endpoint": "wss://xyz.dingtalk.com/ws",
     "ticket": "abc123..."
   }

2. 构建 WebSocket URL: `{endpoint}?ticket={ticket}`

3. 建立 WebSocket 连接并监听消息
```

### 重连机制

连接断开时自动重连：
- 重连延迟：3 秒
- 连接超时：20 秒
- 关闭检测：收到 `Close` 帧或流结束时触发重连

### Keepalive

WebSocket 层每 10 秒发送 Ping 帧保活。

## 消息协议

### 流式信封（StreamEnvelope）

所有 WebSocket 消息均包装在统一的信封结构中：

```json
{
  "type": "EVENT",
  "headers": {
    "messageId": "msg-123",
    "topic": "/v1.0/im/bot/messages/get"
  },
  "data": "{\"msgId\":\"123\",\"senderStaffId\":\"USER123\",...}"
}
```

消息类型：
- `SYSTEM`：系统消息（如 Ping）
- `EVENT` / `CALLBACK`：事件与回调消息

### ACK 响应

所有消息必须回复 ACK：

```json
{
  "code": 200,
  "headers": {
    "messageId": "msg-123",
    "contentType": "application/json"
  },
  "message": "OK",
  "data": ""
}
```

##  inbound 事件处理

### InboundEvent 结构

解析后的入站事件包含：

| 字段 | 类型 | 描述 |
|------|------|------|
| `event_id` | `string` | 消息 ID（`msgId`） |
| `chat_id` | `string` | 会话 ID（私聊为 `sender_id`） |
| `robot_code` | `string` | 机器人编码 |
| `msg_type` | `string` | 消息类型（`text`、`picture`、`audio` 等） |
| `sender_id` | `string` | 发送者 StaffId |
| `session_webhook` | `string` | 会话 Webhook URL |
| `text` | `string` | 消息文本内容 |
| `audio_recognition` | `Option<string>` | 语音转写文本（如有） |
| `media_references` | `Vec<MediaReference>` | 媒体素材引用 |

### 消息类型解析

#### 文本消息（`text`）

```json
{
  "msgtype": "text",
  "text": {
    "content": "你好"
  }
}
```

#### 富文本消息（`richtext`）

```json
{
  "msgtype": "richtext",
  "content": {
    "richText": [
      {"type": "text", "text": "Hello"},
      {"type": "picture", "fileName": "img.png", "downloadCode": "abc123"},
      {"type": "file", "fileName": "spec.pdf", "downloadCode": "file123"}
    ]
  }
}
```

#### 图片消息（`picture` / `image`）

```json
{
  "msgtype": "picture",
  "picture": {
    "fileName": "photo.png",
    "downloadCode": "xyz789"
  }
}
```

#### 语音消息（`audio` / `voice`）

```json
{
  "msgtype": "audio",
  "audio": {
    "duration": 15,
    "downloadCode": "voice456",
    "recognition": "你好这是语音转写结果"
  }
}
```

#### 视频消息（`video`）

```json
{
  "msgtype": "video",
  "video": {
    "fileName": "demo.mp4",
    "downloadCode": "video123"
  }
}
```

#### 文件消息（`file` / `document` / `doc` / `attachment`）

```json
{
  "msgtype": "file",
  "file": {
    "fileName": "report.xlsx",
    "downloadCode": "file456"
  }
}
```

### 事件去重

使用 `EventDeduper` 进行事件去重：
- TTL：60 分钟
- 最大条目数：20,000
- 基于 `event_id` 判重
- 自动清理过期条目

### 发送者白名单

```rust
fn is_sender_allowed(allowlist: &[String], sender_id: &str) -> bool {
    if allowlist.is_empty() {
        return true;
    }
    allowlist.iter().any(|entry| entry == "*" || entry == sender_id)
}
```

## 媒体素材处理

### 下载流程

1. 从事件中提取 `downloadCode` 或 `pictureDownloadCode`
2. 调用 `/v1.0/robot/messageFiles/download` 获取下载 URL
3. 下载媒体文件到内存
4. 提交归档服务

当前会进入该流程的入站消息包括：
- 独立图片消息（`picture` / `image` / `photo`）
- 语音消息（`audio` / `voice`）
- 视频消息（`video`）
- 文件消息（`file` / `document` / `doc` / `attachment`）
- 富文本里的图片、视频、文件附件块

### 归档流程

```rust
let ingest_input = ArchiveIngestInput {
    source_kind: ArchiveSourceKind::ChannelInbound,
    filename: media.filename.clone(),
    declared_mime_type: media.mime_type.clone(),
    session_key: Some(session_key),
    channel: Some("dingtalk".to_string()),
    chat_id: Some(chat_id),
    message_id: Some(event_id),
    metadata,
};

archive_service.ingest_bytes(ingest_input, &bytes).await?;
```

归档后元数据更新：
- `archive.id`
- `archive.storage_rel_path`
- `archive.size_bytes`
- `archive.mime_type`
- `dingtalk.inline_media`（是否内联 base64）

### 内联媒体

- ≤ 20MB 的媒体文件会转为 base64 内联
- 超过 20MB 仅存储归档引用

### 语音转写（ASR）

语音消息自动触发 ASR 转写：

```rust
// 1. 上传语音到钉钉媒体库
let media_id = self.upload_voice_media(&access_token, &audio_bytes).await?;

// 2. 调用 ASR 接口
let transcript = self
    .transcribe_audio(&access_token, &bytes)
    .await?;

// 3. 更新 inbound.text 为转写结果
inbound.text = transcript;
```

ASR 接口：
- URL：`/topapi/asr/voice/translate`
- 请求：`{"media_id": "..."}`
- 响应：`{"errcode": 0, "result": "转写文本"}`

## 响应发送

### Markdown 消息

```rust
async fn send_session_webhook_markdown(
    &self,
    session_webhook: &str,
    title: &str,
    text: &str,
) -> ChannelResult<()> {
    self.http
        .post(session_webhook)
        .json(&serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": title,
                "text": text,
            }
        }))
        .send()
        .await?;
    Ok(())
}
```

### 审批动作卡片（ActionCard）

当响应中包含审批 ID 时，发送审批卡片：

```rust
async fn send_session_webhook_action_card(
    &self,
    session_webhook: &str,
    title: &str,
    text: &str,
    approval_id: &str,
) -> ChannelResult<()> {
    let approve_url = dingtalk_command_action_url("approve", approval_id);
    let reject_url = dingtalk_command_action_url("reject", approval_id);

    self.http
        .post(session_webhook)
        .json(&serde_json::json!({
            "msgtype": "actionCard",
            "actionCard": {
                "title": title,
                "text": text,
                "btnOrientation": "1",
                "btns": [
                    {"title": "批准", "actionURL": approve_url},
                    {"title": "拒绝", "actionURL": reject_url}
                ]
            }
        }))
        .send()
        .await?;
    Ok(())
}
```

动作 URL 协议：
```
dtmd://dingtalkclient/sendMessage?content=%2Fapprove%20{approval_id}
```

## 回调事件处理

### CardCallbackEvent

用户点击审批卡片后触发回调：

| 字段 | 类型 | 描述 |
|------|------|------|
| `event_id` | `Option<string>` | 回调事件 ID |
| `action` | `ApprovalAction` | 审批动作（`approve` / `reject`） |
| `approval_id` | `string` | 审批单 ID |
| `sender_id` | `string` | 操作人 ID |
| `chat_id` | `string` | 会话 ID |
| `session_webhook` | `Option<string>` | 回调 Webhook |

### 动作解析

支持多种回调数据格式：

```rust
fn extract_callback_action_value(value: &Value) -> Option<String> {
    [
        "/value", "/actionValue", "/action/value",
        "/callbackData/value", "/callbackData/action",
        "/cardPrivateData/value", "/content/value",
    ]
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
}
```

动作 Token 解析：
- `approve:APPROVAL_ID`
- `approve_APPROVAL_ID`
- `reject:APPROVAL_ID`

### 回调处理流程

```
1. 解析回调事件 → CardCallbackEvent
2. 校验发送者白名单
3. 事件去重
4. 构造命令：`/{action} {approval_id}`
5. 提交到运行时：session_key = "dingtalk:{account_id}:{chat_id}"
6. 等待响应
7. 通过 session_webhook 返回响应
```

## 会话管理

### Session Key 生成

```rust
let session_key = format!("dingtalk:{}:{}", account_id, chat_id);
```

- 同一会话（相同 `account_id` + `chat_id`）保证串行执行
- 私聊会话 `chat_id = sender_id`
- 群聊消息只有在命中当前 bot 时才会进入 runtime：
  - 优先识别事件里的结构化 `@` 信息（如 `atUsers` / `atOpenIds`）
  - 富文本消息会识别 mention / at block
  - 如果事件没有结构化 `@`，则回退到正文中的显式 `@{bot_title}`
- 命中的 `@{bot_title}` 会在提交给 runtime 前从正文中剥离，避免模型看到多余称呼

### 会话 Webhook

每个会话有独立的 `session_webhook`，用于发送响应：
- 来自钉钉回调事件的 `sessionWebhook` 字段
- 通过 webhook 发送 Markdown 或 ActionCard 消息

## 运行时集成

### 启动流程

```rust
// klaw-cli/src/commands/dingtalk_runtime.rs
pub fn spawn_enabled_channels(
    configs: Vec<DingtalkConfig>,
    adapter: Arc<SharedChannelRuntime>,
    shutdown_rx: watch::Receiver<bool>,
) -> Vec<JoinHandle<()>> {
    configs
        .into_iter()
        .filter(|cfg| cfg.enabled)
        .map(|channel_config| {
            tokio::task::spawn_local(async move {
                let mut channel = DingtalkChannel::new(config)?;
                channel.run_until_shutdown(adapter.as_ref(), &mut shutdown_rx).await
            })
        })
        .collect()
}
```

### 关闭流程

- 发送 WebSocket Close 帧
- 等待最多 2 秒
- 超时强制终止

## 可观测性

使用 `tracing` 记录关键事件：

| 日志级别 | 事件 |
|----------|------|
| `info` | 渠道启动、连接建立、媒体归档、ASR 转写成功 |
| `warn` | 连接失败、媒体下载失败、ASR 失败、白名单拦截、审批卡片降级 |
| `debug` | 原始事件 payload、忽略的消息类型 |

## 安全考虑

1. **发送者白名单**：仅允许受信用户触发机器人响应
2. **事件去重**：防止重放攻击
3. **代理支持**：支持企业内网代理配置
4. **媒体归档**：所有媒体素材持久化存储，便于审计
5. **会话隔离**：不同会话独立处理，避免状态泄漏

## 错误处理

| 错误场景 | 处理策略 |
|----------|----------|
| 连接建立失败 | 延迟 3 秒后重试 |
| WebSocket 断线 | 自动重连 |
| 媒体下载失败 | 记录警告，继续处理其他附件 |
| ASR 转写失败 | 降级为通用语音摘要 |
| 归档服务不可用 | 记录警告，保留原始下载码 |
| Webhook 发送失败 | 记录警告，不影响主流程 |

## 限制

- 不支持直接处理视频消息
- 语音 ASR 依赖钉钉开放平台接口（可能有配额限制）
- 内联媒体限制 20MB
