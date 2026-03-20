# 渠道（Channels）

渠道模块提供外部通信协议接入能力，当前支持：

- [Stdio](./stdio.md) - 基于标准输入输出的终端交互渠道
- [DingTalk（钉钉）](./dingtalk.md) - 基于 WebSocket 长连接的钉钉机器人渠道
- [Telegram](./telegram.md) - 基于 Bot API long polling 的 Telegram 机器人渠道

## 渠道架构

```
┌─────────────────────────────────────────────────────────────┐
│                     Klaw Runtime                            │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              SharedChannelRuntime                     │  │
│  │  - submit() 提交请求到 Agent Loop                     │  │
│  │  - on_cron_tick() 定时任务                            │  │
│  │  - on_runtime_tick() 运行时心跳                       │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
         ▲                                    ▲
         │                                    │
┌────────┴────────┐                  ┌────────┴────────┐
│  DingTalk       │                  │  (其他渠道)     │
│  Channel        │                  │                 │
│  - WebSocket    │                  │                 │
│  - 事件解析     │                  │                 │
│  - 媒体处理     │                  │                 │
│  - 审批回调     │                  │                 │
└─────────────────┘                  └─────────────────┘
```

## 渠道 trait

所有渠道实现 `Channel` trait：

```rust
#[async_trait::async_trait(?Send)]
pub trait Channel {
    fn name(&self) -> &'static str;
    async fn run(&mut self, runtime: &dyn ChannelRuntime) -> ChannelResult<()>;
}
```

## ChannelRuntime 接口

渠道可使用的运行时能力：

```rust
pub trait ChannelRuntime {
    /// 提交用户请求到 Agent Loop
    async fn submit(&self, request: ChannelRequest) -> Result<Option<ChannelOutput>, ChannelError>;

    /// 定时任务回调
    fn on_cron_tick(&self) -> Pin<Box<dyn Future<Output = ()> + '_>>;

    /// 运行时心跳回调
    fn on_runtime_tick(&self) -> Pin<Box<dyn Future<Output = ()> + '_>>;

    /// Cron 任务间隔
    fn cron_tick_interval(&self) -> Duration;

    /// 运行时心跳间隔
    fn runtime_tick_interval(&self) -> Duration;
}
```

## 配置示例

```toml
# ~/.klaw/config.toml

# DingTalk 渠道
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

# Telegram 渠道
[[channels.telegram]]
id = "default"
enabled = true
bot_token = "123456:ABCDEF"
show_reasoning = false
allowlist = ["*"]

[channels.telegram.proxy]
enabled = false
url = "http://proxy.example.com:8080"
```

## 会话管理

每个渠道使用统一的 session_key 格式：

```
{channel}:{account_id}:{chat_id}
```

例如：
- DingTalk: `dingtalk:default:USER123`
- DingTalk 群聊：`dingtalk:default:conversation456`
- Telegram: `telegram:default:123456789`
- Telegram 群聊：`telegram:default:-1001234567890`

同一 `session_key` 的请求保证串行执行，不同会话可并发处理。

## 媒体素材

渠道接收的媒体（图片、语音、文件）会自动：

1. 下载到内存
2. 提交 Archive 归档
3. 生成 `MediaReference` 传递给 Agent

归档后的元数据包含：
- `archive.id`
- `archive.storage_rel_path`
- `archive.size_bytes`
- `archive.mime_type`

## 审批集成

渠道可集成 `approval` 工具，发送审批卡片：

1. Agent 响应中包含审批 ID
2. 渠道发送 ActionCard 卡片（批准/拒绝按钮）
3. 用户点击后触发回调
4. 回调转化为命令 `/approve {approval_id}` 提交到 Agent

## 可观测性

所有渠道使用 `tracing` 记录审计事件：

- 渠道启动/关闭
- 连接建立/断开
- 消息接收/发送
- 媒体归档
- 错误日志

## 渠道对比

| 特性 | Stdio | DingTalk | Telegram |
|------|-------|----------|----------|
| 交互方式 | 终端输入输出 | 钉钉消息 | Telegram Bot |
| 媒体支持 | 无 | 图片、语音、文件 | 图片、文件 |
| 审批卡片 | 无 | 支持 | 首版不支持 |
| 多会话 | 否 | 是 | 是 |
| 回调机制 | 无 | 支持 | 首版不支持 |
| 适用场景 | 本地调试、CLI | 企业协作 | Bot 私聊/群聊 |
