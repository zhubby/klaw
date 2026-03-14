# CHANGELOG

## 2026-03-14

### Changed

- `dingtalk` 通道在 `debug` 日志输出订阅回调原始事件 payload（保留原始事件排查能力）
- README 补充 IM channel 适配契约，明确会话命令与 provider/model 路由由 runtime 统一处理

## 2026-03-13

### Added

- 新增 `klaw-channel` crate README，说明通道职责与 `stdio` 交互模型

### Changed

- `stdio` 通道保持标准行输入，避免 raw mode 对终端和中文输入法的兼容性问题
- `stdio` 模式的 tracing 日志默认写入 `~/.klaw/logs/stdio.log`，避免覆盖当前输入行
- `ChannelRequest` 新增 `media_references` 字段，钉钉非文本消息会产出结构化媒体占位信息
- `stdio` 在等待后台 tick、agent 提交和普通输入期间都能响应 `SIGINT` / `SIGTERM`，避免仅在主提示符下可中断
