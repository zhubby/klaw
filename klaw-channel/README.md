# klaw-channel

`klaw-channel` 负责接入不同输入输出通道，并把消息转成统一的 `ChannelRequest` / `ChannelResponse` 结构。

当前包含：

- `stdio`：本地终端交互通道
- `dingtalk`：钉钉事件与 websocket 通道

## Design

- `Channel` trait 定义通道生命周期与运行入口
- `ChannelRuntime` trait 抽象上层 runtime 提交、定时 tick 和后台服务能力
- 通道层只负责 I/O、协议适配和交互体验，不承载 agent 业务逻辑

## Stdio Interaction

- `stdio` 保持标准行输入，兼容普通终端和中文输入法
- 交互模式下的 tracing 日志默认分流到 `~/.klaw/logs/stdio.log`，避免覆盖当前输入
- `Ctrl+C`、`/exit` 和空输入都在通道层统一处理
