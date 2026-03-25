# klaw-cli

`klaw-cli` 提供 `klaw` 二进制入口，负责：

- 顶层 `clap` 命令解析
- 配置加载与校验
- 启动交互式、单次请求、网关、会话和归档管理命令
- 管理 `klaw gateway` 的用户级守护进程安装与生命周期
- 将 provider streaming、agent 快照和 channel 输出能力接到同一条 runtime 提交通路

## Commands

- 全局参数：`--config <PATH>`、`--log-level <trace|debug|info|warn|error>`
- `klaw`（默认等同于 `klaw gui`）
- `klaw stdio`
- `klaw agent --input "..."`
- `klaw gateway`
- `klaw gui`
- `klaw session ...`
- `klaw archive list|get|push|pull`
- `klaw config ...`
- `klaw daemon install|status|uninstall|start|stop|restart`

## macOS Packaging

仓库根目录提供面向 GUI 桌面分发的 macOS 打包流程，最终仍然封装现有 `klaw` 二进制和 `klaw gui` 入口：

- `make build-macos-app`
- `make package-macos-dmg`

输出目录固定为 `dist/macos/`，包含：

- `Klaw.app`
- `Klaw-<version>-aarch64-apple-darwin.dmg`

当 `Klaw.app` 通过 Finder / LaunchServices 启动时，`klaw-cli` 会在 GUI 启动早期为当前进程补齐常见 macOS 包管理器目录到 `PATH`，包括：

- `/opt/homebrew/bin`
- `/opt/homebrew/sbin`
- `/usr/local/bin`
- `/usr/local/sbin`
- `/opt/local/bin`
- `/opt/local/sbin`

这样可以让通过 Homebrew / MacPorts 安装的 `rg`、`tmux`、`zellij`、`tailscale` 等外部命令在打包后的 `.app` 中继续被检测和调用，而不依赖用户的 shell 初始化脚本是否被执行。

## Daemon Management

`klaw daemon` 只托管 `klaw gateway`：

- macOS 使用 `launchd` `LaunchAgent`
- Linux 使用 `systemd --user`

安装时会固化：

- 当前 `klaw` 可执行文件绝对路径
- 配置文件绝对路径
- `~/.klaw/logs/` 下的 stdout/stderr 日志路径

## Runtime Integration

- `stdio` 和 `gateway` 复用 runtime bundle 构建逻辑
- runtime 内置通用 IM 会话命令路由：`/help`、`/stop`、`/new`、`/model-provider`、`/model`
- runtime 内置审批命令：`/approve <approval_id>`、`/reject <approval_id>`
- runtime 审批命令与工具审批流统一通过 `klaw-approval` manager 层处理状态流转与消费
- runtime 对 `approval_required` 工具结果会直接透传审批提示，不再包装成通用的 tool failure 文案
- runtime 对 `stop` 工具信号会立即结束当前轮次，并在 outbound metadata 中写入 `turn.stopped` / `turn.stop_signal`
- runtime 和 `klaw session` 命令的会话状态/历史操作统一通过 `klaw-session` manager 层处理
- 普通消息默认按 Base Session -> Active Session 路由；全局默认 provider/model 从当前配置实时解析，session 里的 `model_provider` / `model` 只表示显式 override，不再在建会话时复制默认值
- gateway runtime 现同时支持结构化 `POST /webhook/events` 与模板驱动的 `POST /webhook/agents`；后者通过 URL query 接收 `hook_id` / `session_key` / `provider` / `model` 等控制参数，HTTP body 则保持原始 JSON 并在模板尾部追加为 request JSON 代码块
- `stdio` 启动时会在 runtime 与 MCP 完全就绪后打印 ASCII `KLAW` 标记，以及版本、skills、tools、MCP 加载摘要
- `stdio` 默认会将 tracing 日志写入 `~/.klaw/logs/stdio.log`，避免后台日志覆盖当前输入中的 prompt
- `stdio --verbose-terminal` 可显式把 tracing 日志重新打回终端，便于排查启动或 MCP 问题
- `stdio` 与 `gateway` 都监听统一的 shutdown signal；`stdio` 在运行阶段可中断，在 shutdown 阶段再次收到信号会直接退出
- `gateway` 在收到终止信号时会执行 runtime shutdown，确保 MCP/bootstrap 资源收尾
- `klaw gui` 现在会在技能安装、卸载和 registry sync 后向 GUI runtime 发送技能 prompt 热重载命令，使后续请求可立即看到最新 skills
- `klaw gui` / `klaw gateway` 通过共享 `ChannelManager` 管理运行中的 channel 实例；GUI 保存 channel 配置后会立即发送通用 `SyncChannels` 事件，由 runtime 按最新快照执行 keep/start/stop/restart
- `klaw gui` 的 gateway 状态查询现在会先从磁盘最新配置同步 `configured_enabled` / `auth_configured` / `tailscale_mode` 元数据，避免面板状态停留在旧快照；同时运行时新增按当前配置单独启动 gateway 的命令通道
- runtime 现在会在构建 channel driver factory 时按配置装配共享 `VoiceService`，供 Telegram 等 channel 在入站媒体阶段直接调用 STT
- `klaw gui` 的 MCP 面板现在通过只读运行时快照读取 server 状态和缓存的 `tools/list` 响应，避免状态轮询误触发完整 MCP 同步
- runtime system prompt 采用“skills shortlist + workspace docs list + lazy-load instructions”模式，不再注入 `SKILL.md` 全文
- runtime 会分别注册 `skills_registry`（只读 registry catalog）与 `skills_manager`（已安装 skill 生命周期）两个工具
- runtime 会注册 `archive` 工具，用于列出当前消息附件的 archive 句柄、读取只读 archive 文件，以及复制到 `workspace` 后再进行编辑/转换
