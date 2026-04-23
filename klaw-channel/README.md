# klaw-channel

`klaw-channel` 负责接入不同输入输出通道，并把消息转成统一的 `ChannelRequest` / `ChannelResponse` 结构。

当前包含：

- `terminal`：本地终端（TUI）交互渠道；会话 key 形如 `terminal:<id>`
- `dingtalk`：钉钉事件与 websocket 通道（已按目录模块拆分为 `dingtalk/`）
- `telegram`：Telegram Bot API long polling 通道
- `manager`：运行中的 channel 实例生命周期管理与配置快照同步

## Design

- `Channel` trait 定义通道生命周期与运行入口
- `ChannelRuntime` trait 抽象上层 runtime 提交、定时 tick 和后台服务能力，并支持一次性 `submit` 与带快照回调的 `submit_streaming`
- `ChannelManager` 负责按实例键（`{type}:{id}`）统一管理多类型、多实例 channel 的 `keep/start/stop/restart`
- `ChannelConfigSnapshot` / `ChannelInstanceConfig` 提供运行时统一实例快照层，把分类型配置映射成可 diff 的 channel 集合
- `ManagedChannelDriver` / `ChannelDriverFactory` 提供具体 channel driver 边界，后续 `telegram` / `feishu` 可复用同一生命周期接口
- 通道层只负责 I/O、协议适配和交互体验，不承载 agent 业务逻辑
- crate 内提供共享 `media` / `render` / `im_card` 模块，复用媒体引用构造、归档回填、通道输出渲染，以及 IM 卡片解析逻辑，避免各 channel 重复实现
- `ChannelRequest` 可携带 `media_references`；`dingtalk` / `telegram` 会在入站阶段解析媒体附件，下载媒体并写入 archive，再把媒体引用透传给 runtime
- `ChannelResponse` 现可携带结构化 `attachments`；channel 会按 `archive_id` 或受策略约束的本地绝对路径读取文件，并按渠道能力发送图片/文件出站消息
- `dingtalk` 入站媒体下载现在优先使用消息体里的 `downloadCode`，仅在缺失或失败时再回退 `pictureDownloadCode`，减少图片附件在钉钉下载接口上返回 `unknownError` 的概率
- `telegram` 可在 `stream_output=true` 时通过 `sendMessage + editMessageText` 渐进刷新同一条回复；不支持编辑的 channel 则退回完整回复
- `dingtalk` 现在会在 `stream_output=true` 且配置了 `stream_template_id` 时，把普通文本回复改走 AI 卡片模板实例流：先创建并投递卡片实例，再按快照更新配置的内容字段（默认 `content`）；审批 `ActionCard` 和附件发送保持现有路径
- `telegram` HTTP 客户端默认沿用环境代理设置；若配置 `channels.telegram[].proxy`，则显式覆盖为该代理地址
- `telegram` 现在可注入共享 `VoiceService`；当收到 `voice` / `audio` 入站媒体时，会在下载与 archive 入库后尝试 STT，并将 transcript 作为真正的 runtime 输入

## IM Channel 适配契约

- channel 只负责把平台消息规范化为 `ChannelRequest`（文本、chat_id、session_key）
- 统一会话命令（`/new`、`/start`、`/help`、`/model_provider`、`/model`）由 runtime 处理，不在 channel 层实现业务分支
- channel 仅消费 `ChannelResponse` 并回发，不持有 provider/model 路由策略
- 审批类交互现在优先通过共享 `im.card` 元数据模型表达；若上游尚未提供 `im.card`，channel 会兼容回退到既有 `approval.id` / `approval.signal` / 正文 `approval_id` 解析
- `dingtalk` 通道会把共享审批卡片映射为 ActionCard，并把卡片回调映射为 `/approve` 或 `/reject` 指令回送 runtime
- `dingtalk` 入站现在会区分私聊与群聊：私聊保持直接对话，群聊仅在结构化 `@` 字段、富文本 mention block，或正文中显式 `@{bot_title}` 命中当前机器人时才触发 runtime，并会在提交前剥离命中的 bot mention
- `telegram` 通道现在使用目录模块拆分（`telegram/`），并提供 Telegram 专用 HTML 渲染、面向常见 Markdown 的格式映射（标题、引用、列表、链接、行内样式、代码块）、`/start` 新会话兼容、图片/文件/音视频媒体入站、基于共享审批卡片模型映射出的 inline keyboard 回调，以及 archive 驱动的出站图片/文件回复
- `telegram` 入站现在会解析 `chat.type`、`entities` / `caption_entities`、以及 `reply_to_message`：私聊保持直接对话，群组 / 超级群仅在显式 `@bot`、`/command@bot` 或回复 bot 消息时才触发 runtime，并会把命中的 mention / 定向命令规范化为干净输入
- `dingtalk` 应用机器人现支持 archive 驱动的出站附件回复：图片会先上传媒体并在 Markdown 中直接显示；文件仅对 `pdf/doc/docx/xlsx/zip/rar` 发送原生文件消息，并按文件名/MIME 自动补齐 webhook `fileType`，其他类型降级为 Markdown 提示

## Terminal / TUI Interaction

- `klaw tui` 使用全屏 TUI（ratatui / crossterm），由 `klaw-tui` 负责输入与渲染
- 交互模式下的 tracing 日志默认分流到 `~/.klaw/logs/terminal.log`，避免覆盖全屏 UI
- `Ctrl+C`、`SIGTERM` 与 TUI 内退出路径会恢复终端并配合 runtime shutdown
- 在等待输入、后台 tick 或 agent 响应时都可以被终止信号打断
