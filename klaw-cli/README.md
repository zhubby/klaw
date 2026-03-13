# klaw-cli

`klaw-cli` 提供 `klaw` 二进制入口，负责：

- 顶层 `clap` 命令解析
- 配置加载与校验
- 启动交互式、单次请求、网关和会话管理命令
- 管理 `klaw gateway` 的用户级守护进程安装与生命周期

## Commands

- `klaw stdio`
- `klaw agent --input "..."`
- `klaw gateway`
- `klaw session ...`
- `klaw config ...`
- `klaw daemon install|status|uninstall|start|stop|restart`

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
- `stdio` 启动时会在 runtime 与 MCP 完全就绪后打印 ASCII `KLAW` 标记，以及版本、skills、tools、MCP 加载摘要
- `gateway` 在收到终止信号时会执行 runtime shutdown，确保 MCP/bootstrap 资源收尾
