# CHANGELOG

## 2026-03-15

### Added

- `channels.dingtalk` 新增 `proxy` 配置：`proxy.enabled`（默认 `false`）与 `proxy.url`
- 新增 `[storage].root_dir` 配置项，支持声明自定义数据目录根路径
- 新增 `ConfigStore` / `ConfigSnapshot`，支持进程内共享配置快照与保存后内存同步
- `ConfigStore` 新增 `validate_raw_toml`，支持仅校验不落盘

### Changed

- `channels.dingtalk.proxy.enabled=true` 时会校验 `proxy.url` 非空且必须为 `http/https` URL
- 配置校验新增 `storage.root_dir` 非空白约束

## 2026-03-14

### Added

- 新增 `channels.disable_session_commands_for` 配置项，用于按 channel 关闭通用会话命令（`/new`、`/help`、`/model-provider`、`/model`）

## 2026-03-13

### Added

- 新增根级可选 `model` 配置，用于覆盖活跃 `model_provider` 的 `default_model`。

### Fixed

- 配置校验新增 `model` 非空白约束，避免设置空字符串导致运行时模型选择异常。
