# Terminal Multiplexer 工具

## 功能描述

`TerminalMultiplexer` 工具基于 **tmux** 管理持久化终端会话，支持：
- 创建独立的 tmux 会话
- 发送命令到会话
- 读取输出历史
- 附加到现有会话
- 清理会话

设计目标是稳定驱动交互式 TTY 程序（如 codemcp、REPL、调试器），工具会统一使用独立 socket、显式 pane target、结构化输出与轮询同步。

## 依赖

需要系统安装 `tmux`：

```bash
# macOS
brew install tmux

# Ubuntu/Debian
apt install tmux
```

## 配置

```toml
[tools.terminal_multiplexer]
enabled = true
socket_dir = "~/.klaw/tmux-sockets"
default_timeout_secs = 20
max_history_lines = 2000
max_auto_observe_steps = 5
```

## 参数说明

### 创建新会话

```json
{
  "action": "create",
  "purpose": "Interactive REPL for Python",
  "working_dir": "/home/user/project"
}
```

参数：
- `action`: `"create"` - 创建新会话
- `purpose`: `string` - 会话用途描述
- `working_dir`: `string` (可选) - 工作目录

### 发送命令

```json
{
  "action": "send",
  "session_id": "klaw-xxx",
  "command": "npm install",
  "blocking": true,
  "wait_ms": 2000
}
```

参数：
- `action`: `"send"` - 发送命令
- `session_id`: `string` - 会话 ID
- `command`: `string` - 命令内容
- `blocking`: `boolean` - 是否等待执行完成
- `wait_ms`: `number` (可选) - 执行后等待时间（毫秒）

### 读取输出

```json
{
  "action": "read",
  "session_id": "klaw-xxx",
  "max_lines": 500
}
```

### 列出会话

```json
{
  "action": "list"
}
```

### 销毁会话

```json
{
  "action": "destroy",
  "session_id": "klaw-xxx"
}
```

### 自动观察

```json
{
  "action": "observe",
  "session_id": "klaw-xxx",
  "max_steps": 5
}
```

自动轮询读取新输出，直到提示符出现或达到最大步数。

## 输出说明

创建返回会话元数据（socket 路径、会话名、默认 pane）。读取返回输出内容和状态。

## 设计特点

- **独立 sockets**：每个 Klaw 会话使用独立 tmux socket，避免冲突
- **持久化**：tmux 会话在后台保持，跨 Agent 运行仍然存活
- **结构化元数据**：所有会话元数据持久化在 JSON 文件中
- **轮询同步**：通过轮询读取输出，避免 PTY 异步问题
- **边界安全**：socket 目录限制在 `~/.klaw` 内，不使用默认 tmux 目录

## 使用场景

- 运行交互式 REPL
- 长期运行的构建/部署任务
- 调试会话保持
- 多步交互式命令执行

## 安全说明

工具本身不提供沙箱，仍然依赖 `Shell` 工具的安全策略（禁止模式/不安全模式检查）。请在使用前理解安全风险。
