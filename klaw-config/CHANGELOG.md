# CHANGELOG

## 2026-03-19

### Added

- `model_providers.<id>.tokenizer_path` 配置项，支持为本地 token 估算回退指定 `tokenizer.json` 文件
- `tools.archive.enabled` 配置项，支持启用 archive 浏览/读取与复制到 workspace 的运行时工具

## 2026-03-18

### Added

- `tools.skills_manager.enabled` 配置项，支持独立控制已安装 skill 生命周期工具

### Changed

- `tools.skills_registry` 现在仅表示只读 registry 浏览工具，不再混合安装/卸载职责

## 2026-03-15

### Added

- `channels.dingtalk` 新增 `proxy` 配置：`proxy.enabled`（默认 `false`）与 `proxy.url`
- 新增 `[storage].root_dir` 配置项，支持声明自定义数据目录根路径
- 新增 `ConfigStore` / `ConfigSnapshot`，支持进程内共享配置快照与保存后内存同步
- `ConfigStore` 新增 `validate_raw_toml`，支持仅校验不落盘
- 新增 `ToolEnabled` trait，用于统一返回工具配置的开关状态（`enabled()`）
- `tools` 下新增开关配置块：`approval`、`local_search`、`terminal_multiplexers`、`cron_manager`、`skills_registry`

### Changed

- `channels.dingtalk.proxy.enabled=true` 时会校验 `proxy.url` 非空且必须为 `http/https` URL
- 配置校验新增 `storage.root_dir` 非空白约束
- `tools.apply_patch`、`tools.shell` 新增 `enabled` 配置（默认 `true`）
- 对 `apply_patch`/`shell`/`memory`/`sub_agent` 的参数校验改为仅在对应工具 `enabled=true` 时生效

## 2026-03-14

### Added

- 新增 `channels.disable_session_commands_for` 配置项，用于按 channel 关闭通用会话命令（`/new`、`/help`、`/model-provider`、`/model`）

## 2026-03-13

### Added

- 新增根级可选 `model` 配置，用于覆盖活跃 `model_provider` 的 `default_model`。

### Fixed

- 配置校验新增 `model` 非空白约束，避免设置空字符串导致运行时模型选择异常。
