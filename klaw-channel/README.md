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

- `stdio` 使用按键级输入缓冲，而不是简单的按行阻塞读取
- 终端日志会在输出前临时清空当前输入行，并在输出后恢复 prompt 和已输入内容
- `Ctrl+C`、`/exit` 和空输入都在通道层统一处理
