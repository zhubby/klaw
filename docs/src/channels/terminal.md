# Terminal 渠道与 TUI

本文档记录本地 **terminal** 渠道与 `klaw tui` 终端界面：`klaw-channel` 中的会话标识解析、`klaw-tui` 中的全屏 TUI 交互，以及 `klaw-cli` 的启动入口。

> 说明：本文的 **terminal** 指 Klaw 的本地会话渠道名（`channel = "terminal"`）。这与 MCP server 的 **stdio 传输模式**（`mcp.servers[].mode = "stdio"`）是不同概念；后者见 [配置字段详解](../configration/fields.md) 中的 MCP 章节。

## 目标

- 在终端内提供交互式对话（ratatui + crossterm 全屏 UI）
- 支持本地调试与直接命令行使用
- 将会话绑定到统一的 `session_key`，便于与 session 存储、heartbeat、cron 等能力对齐
- 支持可选的推理过程展示
- 响应关闭信号并配合 runtime 优雅收尾

## 代码位置

- 渠道常量与会话 key 解析：`klaw-channel/src/terminal.rs`
- TUI 主循环与提交：`klaw-tui/src/app.rs`、`klaw-tui/src/state.rs`、`klaw-tui/src/view.rs`
- CLI 入口：`klaw-cli/src/commands/tui.rs`

## 配置与启动

### 命令行参数

```bash
klaw tui [OPTIONS]
```

| 参数 | 类型 | 必填 | 描述 | 默认值 |
|------|------|------|------|--------|
| `--session-key` | `string` | 否 | 本地会话标识 | 自动生成 `terminal:<uuid>` |
| `--show-reasoning` | `bool` | 否 | 是否展示模型推理过程 | `false` |
| `--verbose-terminal` | `bool` | 否 | 是否在终端打印 tracing 日志（而非写入日志文件） | `false` |

### 启动示例

```bash
# 使用自动生成的 session key
klaw tui

# 指定 session key
klaw tui --session-key "terminal:my-local-session"

# 展示推理过程
klaw tui --show-reasoning

# 在终端显示日志（便于排查 MCP bootstrap 等）
klaw tui --verbose-terminal
```

## 会话管理

### Session Key 生成

`klaw-channel::terminal::resolve_session_key`：

```rust
pub const TERMINAL_CHANNEL_NAME: &str = "terminal";

pub fn resolve_session_key(session_key: Option<String>) -> String {
    session_key.unwrap_or_else(|| format!("{TERMINAL_CHANNEL_NAME}:{}", Uuid::new_v4()))
}
```

- 未指定时使用 `terminal:<uuid-v4>` 格式
- `chat_id` 由 `session_key` 的第二段解析（见 `resolve_chat_id`）

### Session Key 格式

```
terminal:<chat_id>
```

例如：

- `terminal:abc123-def456-...`（自动生成）
- `terminal:my-local-session`（手动指定）

## 快捷键（TUI）

界面底部会显示简要提示，常见操作：

- `Enter`：提交当前输入
- `Shift+Enter`：在输入区内换行
- `Ctrl+R`：切换是否展示推理过程（与 `--show-reasoning` 启动默认值配合）
- `Ctrl+L`：清空消息区
- `Ctrl+N`：新建会话（生成新的 `terminal:<uuid>`）
- `Esc` 或 `Ctrl+C`：退出并恢复终端

## 交互与运行时

TUI 在主循环中：

- 绘制界面并处理键盘输入
- 按配置的间隔驱动 `on_cron_tick` / `on_runtime_tick`
- 用户提交时构造 `ChannelRequest`，其中 `channel` 为 `"terminal"`，`session_key` / `chat_id` 与元数据一致
- 通过 `submit_streaming` 消费流式事件并更新界面

本地 terminal 渠道当前**不**携带 `media_references`（与旧版行模式 stdin 渠道相同，媒体由 IM 类渠道负责）。

## ChannelRequest 构造（概念）

```rust
let request = ChannelRequest {
    channel: "terminal".to_string(),
    input: input.to_string(),
    session_key: self.session_key.clone(),
    chat_id: self.chat_id.clone(),
    media_references: Vec::new(),
    // ...
};
```

## 关闭处理

TUI 在 Unix 上监听 `SIGINT` / `SIGTERM`（与 Ctrl+C 组合），退出前恢复终端（离开 alternate screen、关闭 raw mode）。`klaw tui` 命令层在收到信号时还会打印提示并走 `shutdown_runtime_bundle`，与 `gateway` 等命令共享同一套 shutdown 语义。

## 可观测性

- 使用 `--verbose-terminal` 时，tracing 直接输出到终端。
- 否则默认写入 `~/.klaw/logs/terminal.log`，避免刷屏干扰 TUI 布局。

## 特点与限制

### 特点

- **开箱即用**：无需额外渠道配置即可本地对话
- **全屏 TUI**：适合多行输入与结构化展示
- **调试友好**：`--show-reasoning` 与 `--verbose-terminal`
- **与会话系统一致**：`terminal:*` 与 storage / CLI `session` 子命令对齐

### 限制

- **不支持媒体入站**：终端渠道不处理图片/文件附件
- **单会话界面**：一次启动对应一个 `session_key`（可在 TUI 内触发新会话，由实现决定具体语义）
- **需真实 TTY**：全屏模式不适合非交互管道场景；脚本化单次请求请使用 `klaw agent --input "..."`

## 测试覆盖

`klaw-channel/src/terminal.rs` 覆盖会话 key 默认值、`chat_id` 解析与终端渲染样式相关用例；`klaw-tui` crate 含状态与集成向测试。

## 使用场景

```bash
# 本地调试（带推理与终端日志）
klaw tui --show-reasoning --verbose-terminal

# 固定会话名，便于 heartbeat / cron 引用同一 session_key
klaw tui --session-key "terminal:daily"
```

## 与其他渠道的对比

| 特性 | Terminal (TUI) | DingTalk |
|------|----------------|----------|
| 交互方式 | 全屏终端 UI | 钉钉消息 |
| 媒体支持 | 无 | 图片、语音、文件 |
| 审批卡片 | 无 | 支持 |
| 多会话 | 视 TUI 实现 | 是 |
| 回调机制 | 无 | 支持 |
| 适用场景 | 本地调试、CLI | 企业协作 |
