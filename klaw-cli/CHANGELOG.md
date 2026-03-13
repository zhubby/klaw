# CHANGELOG

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
