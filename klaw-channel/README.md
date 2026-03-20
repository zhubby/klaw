# klaw-channel

`klaw-channel` 负责接入不同输入输出通道，并把消息转成统一的 `ChannelRequest` / `ChannelResponse` 结构。

当前包含：

- `stdio`：本地终端交互通道
- `dingtalk`：钉钉事件与 websocket 通道

## Design

- `Channel` trait 定义通道生命周期与运行入口
- `ChannelRuntime` trait 抽象上层 runtime 提交、定时 tick 和后台服务能力
- 通道层只负责 I/O、协议适配和交互体验，不承载 agent 业务逻辑
- crate 内提供共享 `media` / `render` 模块，复用媒体引用构造、归档回填和通道输出渲染逻辑，避免各 channel 重复实现
- `ChannelRequest` 可携带 `media_references`；`dingtalk` 会在入站阶段解析图片、语音、视频和通用文件附件，下载媒体并写入 archive，再把媒体引用透传给 runtime

## IM Channel 适配契约

- channel 只负责把平台消息规范化为 `ChannelRequest`（文本、chat_id、session_key）
- 统一会话命令（`/new`、`/help`、`/model-provider`、`/model`）由 runtime 处理，不在 channel 层实现业务分支
- channel 仅消费 `ChannelResponse` 并回发，不持有 provider/model 路由策略
- `dingtalk` 通道会在响应中检测 `approval_id` 并渲染 ActionCard，把卡片回调映射为 `/approve` 或 `/reject` 指令回送 runtime

## Stdio Interaction

- `stdio` 保持标准行输入，兼容普通终端和中文输入法
- 交互模式下的 tracing 日志默认分流到 `~/.klaw/logs/stdio.log`，避免覆盖当前输入
- `Ctrl+C`、`SIGTERM`、`/exit` 和空输入都在通道层统一处理
- 在等待 stdin、后台 tick 或 agent 响应时都可以被终止信号打断
