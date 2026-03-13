# Daemon Management 设计与实施计划

## Summary

`klaw daemon` 为 `klaw gateway` 提供用户级守护进程管理能力，优先复用操作系统原生服务管理器：

- macOS：`launchd` `LaunchAgent`
- Linux：`systemd --user`

命令面分两阶段一次性交付到代码中：

- Phase 1：`install` / `status` / `uninstall`
- Phase 2：`start` / `stop` / `restart`

首版与后续阶段都只管理 `gateway`，不支持任意子命令托管，不支持系统级安装。

## Goals

- 让 `klaw gateway` 能被系统服务管理器托管、开机自启、失败重启。
- 用统一 CLI 屏蔽 `launchd` / `systemd` 差异。
- 固化 `klaw` 可执行文件和配置文件绝对路径，避免服务环境与交互 shell 不一致。
- 在停止或重启守护进程时，对 runtime 的 MCP/bootstrap 资源执行最小必要的优雅收尾。

## CLI Shape

```bash
klaw daemon install
klaw daemon status
klaw daemon uninstall
klaw daemon start
klaw daemon stop
klaw daemon restart
```

全局 `--config` 保留有效：

```bash
klaw --config /abs/path/config.toml daemon install
```

## Implementation

### Service Target

- 只托管 `klaw gateway`
- `ExecStart` / `ProgramArguments` 固定为 `klaw --config <abs> gateway`
- 服务文件命名固定：
  - Linux：`klaw-gateway.service`
  - macOS：`com.klaw.gateway.plist`

### User-Level Install Paths

- Linux：`~/.config/systemd/user/klaw-gateway.service`
- macOS：`~/Library/LaunchAgents/com.klaw.gateway.plist`

日志固定写入 `~/.klaw/logs/`：

- `gateway.stdout.log`
- `gateway.stderr.log`

工作目录固定为 `~/.klaw/`。

### Phase 1

- `install`
  - 生成 service 文件
  - 创建所需目录
  - Linux 执行 `systemctl --user daemon-reload` 和 `enable --now`
  - macOS 对同 label 先 `bootout`，再 `bootstrap`
- `status`
  - 返回是否已安装、是否已加载、是否正在运行
  - 同时展示 service 文件路径和日志路径
- `uninstall`
  - 先停止/卸载已加载 service
  - 删除 service 文件
  - Linux 额外 `daemon-reload`

### Phase 2

- `start`
  - 仅对已安装 service 生效
  - 未安装直接报错，不隐式安装
- `stop`
  - 对已安装 service 生效
  - 对未运行实例保持幂等
- `restart`
  - 对已安装 service 生效
  - 若当前未运行，则按 `start` 语义启动
- `status`
  - 增补平台状态摘要
  - Linux 输出 `ActiveState` / `SubState` / `UnitFileState`
  - macOS 输出 `launchctl print` 提取出的状态摘要

### Runtime Shutdown

`klaw gateway` 增加终止信号收敛：

- 监听 `SIGTERM` / `Ctrl-C`
- 收到信号后停止网关主任务
- 调用 runtime 的 shutdown 路径关闭 MCP/bootstrap 资源

## Platform Mapping

### Linux

- `install` -> `systemctl --user enable --now klaw-gateway.service`
- `status` -> `is-enabled` + `is-active` + `show`
- `uninstall` -> `disable --now` + 删除 unit + `daemon-reload`
- `start` -> `systemctl --user start klaw-gateway.service`
- `stop` -> `systemctl --user stop klaw-gateway.service`
- `restart` -> `systemctl --user restart klaw-gateway.service`

### macOS

- `install` -> `launchctl bootout` + `launchctl bootstrap`
- `status` -> `launchctl print gui/<uid>/com.klaw.gateway`
- `uninstall` -> `launchctl bootout` + 删除 plist
- `start` -> `launchctl bootstrap`
- `stop` -> `launchctl bootout`
- `restart` -> `launchctl bootout` + `launchctl bootstrap`

## Error Semantics

CLI 需要对以下失败给出明确错误：

- 当前平台不支持
- 默认配置路径无法解析
- `systemctl` / `launchctl` / `id` 不存在
- service 文件目录创建失败
- service 文件写入失败
- 平台管理命令执行失败
- 对未安装 service 执行 `start` / `stop` / `restart`

## Test Plan

- CLI 解析：
  - `install/status/uninstall/start/stop/restart` 均能被 `clap` 正确解析
- 文件渲染：
  - systemd unit 含正确 `ExecStart`、重启策略、日志路径
  - launchd plist 含正确 `ProgramArguments`、`KeepAlive`、日志路径
- 状态解析：
  - systemd `show` 输出能正确提取 `LoadState` / `ActiveState` / `SubState`
  - launchd `print` 输出能识别 running / waiting
- 运行时收敛：
  - gateway 在收到终止信号后走 runtime shutdown 路径
- 文档：
  - `mdbook build docs` 通过

## Defaults

- 计划文档与后续落地实现都使用同一份文档维护，不拆 Phase 1 / Phase 2 两篇。
- `install` 默认即启用并启动 service。
- `restart` 默认采用“已安装即可执行，未运行则启动”的宽松语义。
- 暂不提供 `klaw daemon logs`、`enable/disable`、系统级安装或任意服务托管。
