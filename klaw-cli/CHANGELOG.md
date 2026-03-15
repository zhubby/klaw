# CHANGELOG

## 2026-03-15

### Changed

- runtime tool registration now injects `session_store` into `ShellTool`, enabling persistent approval request creation/validation for shell commands
- runtime now also registers `ApprovalTool` with shared `session_store`, enabling generic approval lifecycle operations via tool calls
- added `/approve <approval_id>` channel command to approve pending shell requests within the current/base session scope and return retry guidance
- added `/reject <approval_id>` channel command to reject pending shell requests within the current/base session scope
- `/approve <approval_id>` now attempts immediate shell execution after approval and returns execution output/failure hint in the same response
- `/approve <approval_id>` now re-enters `submit_and_get_output` after shell execution, so the model can produce a final user-facing response from tool output
- `submit_and_get_output` 增加媒体引用参数，并将 channel 传入的 `media_references` 写入 `InboundMessage`，避免 runtime 丢失入站媒体

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
