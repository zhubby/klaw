# 快速开始

## 安装与构建

```bash
# 构建整个工作空间
cargo build --workspace

# 运行测试
cargo test --workspace
```

根目录直接执行 `cargo build` 时，会使用 workspace `default-members`，默认不包含 `klaw-webui`。如果需要刷新浏览器端聊天页面资源，请先在仓库根目录执行 `make webui-wasm`，再编译 `klaw-gateway`。

## 配置

首次运行会自动创建配置文件 `~/.klaw/config.toml`：

```toml
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
proxy = false
env_key = "OPENAI_API_KEY"
```

## 运行模式

### TUI 模式（本地终端交互）

```bash
klaw tui
```

如果要直接在终端看到 tracing / MCP bootstrap 日志，可改用：

```bash
klaw tui --verbose-terminal
```

- 在输入区编辑文本，回车提交；`Shift+Enter` 换行
- `Esc` 或 `Ctrl+C` 退出

### 单次请求

```bash
klaw agent --input "你的问题"
```

### Gateway 模式（WebSocket）

```bash
klaw gateway
```

启动后连接 `ws://127.0.0.1:8080/ws/chat?session_key=your-room`

### Daemon 模式（用户级守护进程）

```bash
klaw daemon install
klaw daemon status
```

- `install` 会把 `klaw gateway` 注册为当前用户的系统服务并立即启动
- macOS 使用 `launchd`，Linux 使用 `systemd --user`
- 如需停止或卸载：

```bash
klaw daemon stop
klaw daemon uninstall
```

## 配置工具

### 启用 Web Search

```toml
[tools.web_search]
enabled = true
provider = "tavily"

[tools.web_search.tavily]
env_key = "TAVILY_API_KEY"
search_depth = "basic"
include_answer = true
```

### 启用 Memory

```toml
[tools.memory]
enabled = true
search_limit = 8
use_vector = true

[memory.embedding]
provider = "openai"
model = "text-embedding-3-small"
```

### 配置 Skills

```toml
[skills]
sync_timeout = 60

[skills.anthropic]
address = "https://github.com/anthropics/skills"

[skills.vercel]
address = "https://github.com/vercel-labs/skills"
installed = ["brainstorming"]
```

## 会话管理

```bash
# 列出所有会话
klaw session list

# 查看会话详情
klaw session get --session-key "terminal:my-chat"
```

## 下一步

- 阅读 [Agent Core 文档](./agent-core/README.md) 了解消息协议和运行时
- 查看 [工具文档](./tools/README.md) 了解可用工具
- 参考 [设计计划](./plans/README.md) 了解演进路线
