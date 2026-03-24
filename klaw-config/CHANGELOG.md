# CHANGELOG

## 2026-03-24

### Changed

- `conversation_history_limit` 默认值由 `20` 调整为 `40`，在保持 `N/2` 压缩触发规则不变的前提下扩大默认历史窗口

## 2026-03-21

### Added

- 新增 `gateway.enabled` 配置项，用于控制 GUI/runtime 是否自动启动 gateway
- 新增 `gateway.webhook` 配置块，用于声明 Bearer 鉴权的 webhook 事件入口、token 来源和请求体大小限制
- 新增 `observability.local_store.enabled`、`observability.local_store.retention_days` 与 `observability.local_store.flush_interval_seconds` 配置项，用于控制本地分析存储
- 新增 `tools.heartbeat_manager.enabled` 配置项，用于控制 heartbeat 管理工具注册

### Changed

- `gateway.listen_port` 默认值改为 `0`，允许启动时由系统分配随机端口
- 观测配置校验新增 `local_store` 保留天数和刷新间隔的正整数约束
- `tools.shell` 配置移除了 `safe_commands` 与 `approval_policy`，并调整为双层规则：`blocked_patterns` 命中即拒绝，`unsafe_patterns` 命中则请求审批，其余命令默认允许执行
- `heartbeat.*` 配置块保留解析兼容性，但不再参与运行时校验或作为 heartbeat 真源

## 2026-03-20

### Added

- 新增 `model_providers.<id>.stream`、`channels.telegram[].stream_output` 与 `channels.dingtalk[].stream_output` 配置项，用于分别控制 provider 侧 stream API 和 channel 侧增量输出
- 新增 `channels.telegram`、`TelegramConfig` 与 `TelegramProxyConfig`，支持 Telegram channel 的 bot token、allowlist、reasoning 显示和代理配置

### Changed

- `default_config_path()` now resolves `~/.klaw/config.toml` through the shared `klaw-util` path helpers instead of rebuilding the default path locally
- `ChannelsConfig`、`DingtalkConfig` 与 `DingtalkProxyConfig` 现在实现 `PartialEq` / `Eq`，供运行时 channel 实例 diff 使用
- `channels` 配置校验新增 `telegram`：校验重复 id、`bot_token` 非空以及 `proxy.url` 的 `http/https` 约束

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
