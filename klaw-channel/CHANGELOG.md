# CHANGELOG

## 2026-03-13

### Added

- 新增 `klaw-channel` crate README，说明通道职责与 `stdio` 交互模型

### Changed

- `stdio` 通道改为按键级输入缓冲，支持在后台日志输出后恢复 prompt 和用户已输入内容
- 为 `stdio` 模式新增终端日志 writer，避免 `tracing` 输出直接打断当前输入行
