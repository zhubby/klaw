# CHANGELOG

## 2026-03-21

### Added

- runtime now writes provider request/response audit records to `llm_audit` via a bounded asynchronous background writer so LLM auditing does not block the main request path

## 2026-03-20

### Added

- channel runtime submission now has a streaming path that wires provider deltas through the agent/runtime stack into channel-specific writers
- added repo-level macOS packaging support for the `klaw gui` desktop entrypoint, including `make build-macos-app`, `make package-macos-dmg`, and a GitHub Actions workflow that builds `Klaw.app` and a versioned `.dmg`

### Changed

- running `klaw` without any subcommand now defaults to the GUI entrypoint, equivalent to `klaw gui`
- `/approve <approval_id>` now returns a user-facing “already approved and executed” message for consumed shell approvals instead of exposing the internal `consumed` status word
- `klaw gui` / `klaw gateway` 现在通过 `klaw-channel::ChannelManager` 托管 channel 生命周期，并统一用 `SyncChannels` 配置快照同步代替 dingtalk 专用重载逻辑
- `klaw gui` / `klaw gateway` 现在也可通过同一 `ChannelManager` 生命周期层启动和同步 `telegram` channel 实例
- IM 命令路由现在兼容 `/start`，会返回与 `/help` 相同的帮助内容，适配 Telegram 机器人默认入口
- `/new` 现在显式读取当前全局 active provider 及其默认模型创建新会话，不再继承当前会话里已持久化的 provider/model

## 2026-03-19

### Added

- runtime now persists request-level LLM token usage from outbound metadata into `klaw.db` and `klaw session list/get` now show aggregated token totals per session
- runtime now registers an `archive` tool for current-message attachment lookup, archive record inspection, text reads, and copy-to-workspace flows

### Changed

- runtime system prompt now states that files under `archives/` are read-only source material and must be copied into `workspace/` before modification

### Fixed

- fresh environments now keep the default `openai` config but still allow `klaw gui` to start when provider credentials are missing; unavailable providers are registered as placeholders and report clear setup guidance when first used
- `klaw gui` startup now reports runtime initialization failures directly, instead of surfacing a misleading `startup channel closed` error when the worker exits before sending the startup report
- `klaw gui` manual `Run Now` cron command no longer blocks the GUI until runtime drain and outbound webhook delivery complete; the follow-up drain is now scheduled asynchronously on the runtime thread
- dingtalk outbound delivery now runs in a dedicated background dispatcher thread with an explicit per-message timeout, so stuck webhook sends no longer block runtime message handling

## 2026-03-18

### Changed

- runtime 技能工具注册拆分为 `skills_registry`（只读 registry catalog）与 `skills_manager`（已安装 skill 生命周期）
- runtime 加载已安装 skills 时改为通过新的 `SkillsManager` 接口读取合并后的 installed 视图

## 2026-03-17

### Changed

- runtime system prompt 组装改为使用 `klaw-core::compose_runtime_prompt`，不再把已安装 skills 的 `SKILL.md` 全文拼接进 prompt
- runtime 启动与 skills prompt 热重载时会调用 `ensure_workspace_prompt_templates`，确保 `~/.klaw/workspace` 下的引导文档模板存在（仅缺失时写入）
- runtime prompt 现在注入 skills shortlist（name/path/source/description）与 workspace docs 列表，指引模型按需读取文件而不是预加载全文

## 2026-03-16

### Changed

- `/approve <approval_id>` 与 `/reject <approval_id>` 现在支持所有工具类型的审批单（不再仅限 `shell`）
- 非 `shell` 审批在通过后会返回“请重试触发审批的原操作”；`shell` 审批保持“批准即执行并回传结果”的原行为
- `klaw gui` 现在支持在技能安装、卸载和 registry sync 后热重载运行中 runtime 的 skills system prompt，无需重启 GUI runtime
- `klaw gui` 现在支持接收 cron 面板的 `Run Now` 运行时命令，立即触发指定 cron job 并在同一运行时线程内补跑一次 inbound drain

## 2026-03-15

### Changed

- added `klaw gui` subcommand to launch the desktop workbench UI via `klaw-gui`
- runtime approval commands and approval tool registration now route approval lifecycle operations through `klaw-approval`
- runtime session routing/history operations and `klaw session` commands now route through `klaw-session`
- runtime tool registration now injects `session_store` into `ShellTool`, enabling persistent approval request creation/validation for shell commands
- runtime now also registers `ApprovalTool` with shared `session_store`, enabling generic approval lifecycle operations via tool calls
- added `/approve <approval_id>` channel command to approve pending shell requests within the current/base session scope and return retry guidance
- added `/reject <approval_id>` channel command to reject pending shell requests within the current/base session scope
- `/approve <approval_id>` now attempts immediate shell execution after approval and returns execution output/failure hint in the same response
- `/approve <approval_id>` now re-enters `submit_and_get_output` after shell execution, so the model can produce a final user-facing response from tool output
- `submit_and_get_output` 增加媒体引用参数，并将 channel 传入的 `media_references` 写入 `InboundMessage`，避免 runtime 丢失入站媒体
- `klaw gateway` 现在支持按 `channels.dingtalk[].proxy` 初始化 dingtalk 通道代理策略；默认禁用代理
- `klaw gateway` 收到退出信号时会先广播 dingtalk 通道 shutdown，再等待通道优雅关闭 websocket（超时后记录警告）
- `klaw gui` 启动路径改为和 `gateway` 对齐：进入 runtime 启动/初始化与 dingtalk channel 生命周期管理，但不启动 web gateway 服务
- `klaw gateway`/`klaw gui` 共享 dingtalk channel 启停辅助逻辑，并统一在主任务退出后执行 channel 关闭等待
- 修复 macOS 下 `klaw gui` 在非主线程初始化 `winit EventLoop` 导致 panic 的问题：GUI 事件循环改为主线程运行
- runtime 工具注册改为统一读取 `tools.*.enabled` 开关：`apply_patch`、`shell`、`approval`、`local_search`、`terminal_multiplexers`、`cron_manager`、`skills_registry`、`memory`、`web_fetch`、`web_search`、`sub_agent`

## 2026-03-14

### Added

- 新增通用 IM 会话命令路由（`/help`、`/new`、`/model-provider`、`/model`），默认对 channel 开启并支持按 channel opt-out
- 新增 Base Session -> Active Session 路由策略，普通消息会自动落到 active session

### Changed

- 统一在 `ChannelRequest -> InboundMessage` 转换点输出 `info` 日志，打印各 channel 入站规范化后的 `inbound` 数据
- Runtime 启动改为加载可用 provider registry，并在每条消息按会话 metadata 选择 provider/model
- Runtime 默认 tool 循环上限改为不设限：`max_tool_iterations=0`、`max_tool_calls=0`（`0` 表示不设限）

## 2026-03-13

### Added

- 新增 `klaw daemon` 子命令，支持 `install`、`status`、`uninstall`、`start`、`stop`、`restart`
- 新增 `klaw archive` 子命令，支持 `list`、`get`、`push`、`pull`
- 新增 `systemd --user` 与 `launchd` 用户级服务文件渲染与管理逻辑
- 新增 daemon 相关单元测试和计划文档
- 新增 `klaw stdio` 启动 ASCII `KLAW` 标记与版本、skills、tools、MCP 加载摘要输出
- 新增 `klaw gateway` 启动成功后的监听地址 stdout 输出
- 新增全局 `--log-level <trace|debug|info|warn|error>` 参数，可直接设置 tracing 日志级别

### Changed

- `klaw gateway` 增加终止信号处理，并在退出时执行 runtime shutdown
- `klaw stdio` 在进入交互前等待 MCP bootstrap 完成，避免启动后首条消息才触发就绪校验
- `klaw stdio` 的 tracing 日志改为默认写入 `~/.klaw/logs/stdio.log`，避免后台日志覆盖当前输入
- `klaw stdio` 新增 `--verbose-terminal` 开关，允许调试时显式把 tracing 日志输出回终端
- `--log-level` 显式设置为 `debug/trace` 时，默认将 `sqlx` 查询日志降为 `warn`，减少 cron 轮询 SQL 刷屏
- `--log-level` 显式设置为 `debug/trace` 时，默认将 Turso/SQLite 引擎内部 target 降为 `warn`，抑制 `normal_step/_prepare/read_page` 类高频日志
- `klaw stdio` 现在和 `gateway` 共享统一的 shutdown signal 处理，并在 runtime shutdown 阶段支持第二次信号直接终止进程
- `klaw stdio`/`agent` 运行时现在会在发起本轮请求前读取会话 JSONL 历史，并把上一轮对话注入到 LLM 请求中
