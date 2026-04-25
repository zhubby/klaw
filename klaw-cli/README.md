# klaw-cli

`klaw-cli` 提供 `klaw` 二进制入口，负责：

- 顶层 `clap` 命令解析
- 配置加载与校验
- 启动交互式、单次请求、网关、会话和归档管理命令
- 管理 `klaw gateway` 的用户级守护进程安装与生命周期
- 把共享宿主/runtime 组装层委托给 `klaw-runtime`

## Commands

- 全局参数：`--config <PATH>`、`--log-level <trace|debug|info|warn|error>`
- `klaw`（默认等同于 `klaw gui`）
- `klaw tui`
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

- `klaw-cli` 不再内嵌共享 runtime 实现；`tui`、`agent`、`gateway` 与 `gui` 统一通过 `klaw-runtime` 获取宿主能力
- `klaw-runtime` 暴露统一的 runtime facade，包括 bundle 构建/关闭、submit 流程、channel runtime、后台服务、webhook glue 和 gateway 管理
- 通用 IM 会话命令路由、审批命令、卡片回答命令与 session policy 已迁入 `klaw-runtime`
- `klaw-cli` 继续保留纯入口相关逻辑，例如 tracing 初始化、GUI 进程环境准备、`daemon` 命令和子命令分发
- `tui` 启动时会在 runtime 与 MCP 完全就绪后展示启动摘要（版本、skills、tools、MCP 等）
- `tui` 默认会将 tracing 日志写入 `~/.klaw/logs/terminal.log`，避免后台日志干扰全屏界面
- `tui --verbose-terminal` 可显式把 tracing 日志重新打回终端，便于排查启动或 MCP 问题
- `tui` 与 `gateway` 都监听统一的 shutdown signal；`tui` 在运行阶段可中断，在 shutdown 阶段再次收到信号会直接退出
- `gateway` 在收到终止信号时会执行 runtime shutdown，确保 MCP/bootstrap 资源收尾
- `klaw gui` 现在会在技能安装、卸载和 registry sync 后向 GUI runtime 发送技能 prompt 热重载命令，使后续请求可立即看到最新 skills
- `klaw gui` / `klaw gateway` 通过共享 `ChannelManager` 管理运行中的 channel 实例；GUI 保存 channel 配置后会立即发送通用 `SyncChannels` 事件，由 runtime 按最新快照执行 keep/start/stop/restart
- `klaw gui` 的 gateway 状态查询现在会先从磁盘最新配置同步 `configured_enabled` / `auth_configured` / `tailscale_mode` 元数据，并重新探测本机 Tailscale host 状态，避免面板状态停留在旧快照；同时运行时新增按当前配置单独启动 gateway 的命令通道
- `klaw gui` now exposes runtime commands for the Knowledge panel to query status, search configured knowledge, inspect entries, and run incremental index/vector sync
- runtime 现在会在构建 channel driver factory 时按配置装配共享 `VoiceService`，供 Telegram 等 channel 在入站媒体阶段直接调用 STT
- `klaw gui` 的 MCP 面板现在通过只读运行时快照读取 server 状态和缓存的 `tools/list` 响应，避免状态轮询误触发完整 MCP 同步
- runtime system prompt 采用“skills shortlist + workspace docs list + lazy-load instructions”模式，不再注入 `SKILL.md` 全文
- runtime 会分别注册 `skills_registry`（只读 registry catalog）与 `skills_manager`（已安装 skill 生命周期）两个工具
- runtime 会注册 `archive` 工具，用于列出当前消息附件的 archive 句柄、读取只读 archive 文件，以及复制到 `workspace` 后再进行编辑/转换
