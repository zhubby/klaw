# CHANGELOG

## 2026-04-10

### Added

- 新增 `mcp.servers[].tool_timeout_seconds` 配置项，默认值为 `60`，用于限制单个 MCP server 上 `tools/call` 的执行时长

### Changed

- MCP 配置校验现在要求 `mcp.servers[].tool_timeout_seconds` 必须大于 `0`

## 2026-04-07

### Added

- 新增 `tools.ask_question.enabled` 配置项，默认开启，用于控制 `ask_question` 交互提问工具是否向 runtime 暴露

## 2026-03-30

### Added

- 新增顶层 `acp` 配置块，支持声明 ACP agent 启动超时和 agent 列表
- 新增 `AcpAgentConfig`，用于描述外部 ACP agent 的 `id`、`command`、`args`、`env` 和描述信息

### Changed

- 默认 ACP agent 模板现在预填两套 Zed adapter：`npx -y @zed-industries/claude-agent-acp` 与 `npx -y @zed-industries/codex-acp`
- ACP agent 配置不再持久化独立 `cwd`；实际运行目录统一由调用时传入的 `working_directory` 决定
- `cron` 配置新增 `missed_run_policy`，默认值为 `skip`，用于控制服务停机后重启时是否补偿错过的定时触发

## 2026-03-29

### Changed

- `gateway.webhook.events.path` 与 `gateway.webhook.agents.path` 不再作为有效配置项；webhook 路径固定为 `/webhook/events` 和 `/webhook/agents`，旧配置值会被兼容解析但忽略

## 2026-03-27

### Added

- 新增 `tools.geo.enabled` 配置项，默认值为 `false`，用于控制 macOS `geo` 定位工具是否向 runtime 暴露
- 新增 `tools.channel_attachment.enabled` 与 `tools.channel_attachment.local_attachments`，支持控制附件回复工具注册、声明本地附件 allowlist 和最大文件大小限制

## 2026-03-26

### Changed

- 删除 `mcp.enabled` 配置项，MCP runtime 现在始终可用；配置仅保留 `mcp.startup_timeout_seconds` 与 `mcp.servers`
- 根级 `model` 现明确标记为 legacy 兼容字段；默认 provider/model 路由不再读取它，实际默认模型始终来自 `model_providers.<id>.default_model`
- `voice.enabled` 重新恢复为 voice runtime 的正式开关；`voice` tool 现在要求它与 `tools.voice.enabled` 同时开启才会注册

## 2026-03-25

### Added

- `gateway.webhook` 现支持独立的 `events` / `agents` 子配置，分别声明 endpoint 是否启用、路径与请求体大小限制

### Fixed

- `ConfigStore` 新增基于磁盘最新 `config.toml` 的原子更新路径，避免多个 GUI 面板分别持有旧快照时把彼此已保存的配置整份覆盖掉
- `save_observability_config()` 现在复用统一更新逻辑，并补充 stale snapshot 回归测试，确保跨面板保存不会抹掉新增 provider 等其他已落盘字段

### Changed

- `heartbeat.defaults.timezone` 的默认值改为启动时探测到的系统 timezone，不再硬编码为 `UTC`

## 2026-03-24

### Added

- 新增完整 `voice` 配置块，支持声明默认 STT/TTS provider、默认语言/音色，以及 Deepgram、AssemblyAI、ElevenLabs 的 provider 参数与校验规则
- 新增 `tools.voice.enabled` 配置项，用于控制 runtime 是否向模型暴露 `voice` tool

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
