# klaw-channel

`klaw-channel` 负责接入不同输入输出通道，并把消息转成统一的 `ChannelRequest` / `ChannelResponse` 结构。

当前包含：

- `stdio`：本地终端交互通道
- `dingtalk`：钉钉事件与 websocket 通道
- `telegram`：Telegram Bot API long polling 通道
- `manager`：运行中的 channel 实例生命周期管理与配置快照同步

## Design

- `Channel` trait 定义通道生命周期与运行入口
- `ChannelRuntime` trait 抽象上层 runtime 提交、定时 tick 和后台服务能力，并支持一次性 `submit` 与带快照回调的 `submit_streaming`
- `ChannelManager` 负责按实例键（`{type}:{id}`）统一管理多类型、多实例 channel 的 `keep/start/stop/restart`
- `ChannelConfigSnapshot` / `ChannelInstanceConfig` 提供运行时统一实例快照层，把分类型配置映射成可 diff 的 channel 集合
- `ManagedChannelDriver` / `ChannelDriverFactory` 提供具体 channel driver 边界，后续 `telegram` / `feishu` 可复用同一生命周期接口
- 通道层只负责 I/O、协议适配和交互体验，不承载 agent 业务逻辑
- crate 内提供共享 `media` / `render` 模块，复用媒体引用构造、归档回填和通道输出渲染逻辑，避免各 channel 重复实现
- `ChannelRequest` 可携带 `media_references`；`dingtalk` / `telegram` 会在入站阶段解析媒体附件，下载媒体并写入 archive，再把媒体引用透传给 runtime
- `ChannelResponse` 现可携带结构化 `attachments`；channel 会按 `archive_id` 或受策略约束的本地绝对路径读取文件，并按渠道能力发送图片/文件出站消息
- `dingtalk` 入站媒体下载现在优先使用消息体里的 `downloadCode`，仅在缺失或失败时再回退 `pictureDownloadCode`，减少图片附件在钉钉下载接口上返回 `unknownError` 的概率
- `telegram` 可在 `stream_output=true` 时通过 `sendMessage + editMessageText` 渐进刷新同一条回复；不支持编辑的 channel 则退回完整回复
- `telegram` HTTP 客户端默认沿用环境代理设置；若配置 `channels.telegram[].proxy`，则显式覆盖为该代理地址
- `telegram` 现在可注入共享 `VoiceService`；当收到 `voice` / `audio` 入站媒体时，会在下载与 archive 入库后尝试 STT，并将 transcript 作为真正的 runtime 输入

## IM Channel 适配契约

- channel 只负责把平台消息规范化为 `ChannelRequest`（文本、chat_id、session_key）
- 统一会话命令（`/new`、`/start`、`/help`、`/model_provider`、`/model`）由 runtime 处理，不在 channel 层实现业务分支
- channel 仅消费 `ChannelResponse` 并回发，不持有 provider/model 路由策略
- `dingtalk` 通道会在响应中检测 `approval_id` 并渲染 ActionCard，把卡片回调映射为 `/approve` 或 `/reject` 指令回送 runtime
- `telegram` 通道现在使用目录模块拆分（`telegram/`），并提供 Telegram 专用 HTML 渲染、面向常见 Markdown 的格式映射（标题、引用、列表、链接、行内样式、代码块）、`/start` 新会话兼容、图片/文件/音视频媒体入站、基于 inline keyboard 的审批回调，以及 archive 驱动的出站图片/文件回复
- `dingtalk` 应用机器人现支持 archive 驱动的出站附件回复：图片会先上传媒体并在 Markdown 中直接显示，文件会作为原生文件消息发送

## Stdio Interaction

- `stdio` 保持标准行输入，兼容普通终端和中文输入法
- 交互模式下的 tracing 日志默认分流到 `~/.klaw/logs/stdio.log`，避免覆盖当前输入
- `Ctrl+C`、`SIGTERM`、`/exit` 和空输入都在通道层统一处理
- 在等待 stdin、后台 tick 或 agent 响应时都可以被终止信号打断
