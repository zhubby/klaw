# CHANGELOG

## 2026-03-13

### Added

- 新增 `klaw-channel` crate README，说明通道职责与 `stdio` 交互模型

### Changed

- `stdio` 通道保持标准行输入，避免 raw mode 对终端和中文输入法的兼容性问题
- `stdio` 模式的 tracing 日志默认写入 `~/.klaw/logs/stdio.log`，避免覆盖当前输入行
- `ChannelRequest` 新增 `media_references` 字段，钉钉非文本消息会产出结构化媒体占位信息
- `stdio` 在等待后台 tick、agent 提交和普通输入期间都能响应 `SIGINT` / `SIGTERM`，避免仅在主提示符下可中断
