# Telegram

本文档记录 `klaw-channel` 中 Telegram channel 的能力：基于 Bot API `getUpdates` 的 long polling、文本/多媒体入站、统一会话命令、Telegram HTML 渲染、以及 inline keyboard 审批回调。

## 实现位置

- 渠道实现：`klaw-channel/src/telegram/`
- 运行时注册：`klaw-channel/src/manager.rs`
- 配置模型：`klaw-config/src/lib.rs`（`channels.telegram`）
- GUI 面板：`klaw-gui/src/panels/channel.rs`

## 配置示例

```toml
[[channels.telegram]]
id = "default"
enabled = true
bot_token = "123456:ABCDEF"
show_reasoning = false
allowlist = ["*"]

[channels.telegram.proxy]
enabled = false
url = "http://127.0.0.1:8888"
```

## 会话与路由

- 会话键格式：`telegram:{account_id}:{chat_id}`
- `chat_id` 直接使用 Telegram Bot API 的 chat id
- 私聊和群聊都按 chat 级别建会话
- `/new`、`/help`、`/model_provider`、`/model` 等通用命令仍由 runtime 统一处理
- `/start` 会兼容映射到帮助页，适配 Telegram 机器人默认入口习惯

## 入站能力

- 支持 `message.text`
- 支持 `caption` 回填为输入正文
- 支持 `photo`、`document`、`audio`、`voice`、`video` 附件
- 支持 `callback_query`，可将 inline keyboard 审批按钮映射为 `/approve` / `/reject`
- 会忽略机器人自身消息、无可处理正文的更新、以及非 `message` 类更新
- `allowlist` 规则与 dingtalk 一致：空表示全放行，`"*"` 表示通配，其余按 `sender_id` 精确匹配

## 媒体处理

- `photo` / `document` / `audio` / `voice` / `video` 会先调用 `getFile`
- 再按返回 `file_path` 下载原始字节
- 下载结果复用 `klaw-channel::media` 写入 archive，并生成 `MediaReference`
- 当消息只有媒体没有文本时，会生成简短占位文本，避免 runtime 收到空输入

## 出站能力

- 渠道会将 `ChannelResponse` 渲染为 Telegram `HTML` 文本，并通过 `sendMessage` 直接回复当前 chat
- Telegram 渲染层会把常见 Markdown 映射为 Bot API 支持的 HTML：标题、粗体/斜体/下划线/删除线、引用块、列表、行内代码、fenced code block、链接、剧透
- 继续使用 Telegram `HTML` parse mode，而不是直接把原始 markdown 作为 `MarkdownV2` 发出；这样可以避免 Telegram `MarkdownV2` 在普通文本、链接、代码块场景下大量上下文相关转义带来的错乱
- 当响应包含 `approval_id` 时，会自动发送带 `Approve` / `Reject` inline keyboard 的审批消息
- 仍未实现 Telegram 专属异步 outbound dispatcher；当前仍以“入站请求即时回复”为主
