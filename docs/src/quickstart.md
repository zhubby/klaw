# 快速开始

## 安装与构建

```bash
# 克隆项目
git clone https://github.com/your-org/klaw.git
cd klaw

# 构建整个工作空间
cargo build --workspace

# 安装到 ~/.cargo/bin
cargo install --path klaw-cli

# 运行测试
cargo test --workspace
```

根目录直接执行 `cargo build` 时，会使用 workspace `default-members`，默认不包含 `klaw-webui`。如果需要刷新浏览器端聊天页面资源，请先在仓库根目录执行 `make webui-wasm`，再编译 `klaw-gateway`。

## 配置

首次运行会自动创建配置文件 `~/.klaw/config.toml`。你需要设置环境变量或者编辑配置文件来添加 API Key。

### 最小配置（OpenAI）

```toml
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
```

然后设置环境变量：

```bash
export OPENAI_API_KEY="sk-xxx"
```

### 使用 Anthropic

```toml
model_provider = "anthropic"

[model_providers.anthropic]
base_url = "https://api.anthropic.com/v1"
default_model = "claude-3-5-sonnet-20241022"
env_key = "ANTHROPIC_API_KEY"
```

## 运行模式

### GUI 模式（桌面工作台）

不带参数直接运行 `klaw` 启动原生桌面 GUI，这是最推荐的本地开发体验：

```bash
klaw
# 或者
klaw gui
```

Klaw GUI 提供：
- 会话管理面板
- 实时配置编辑器
- 对话交互界面
- 语音输入支持
- 工具调用可视化
- 可观测面板

### TUI 模式（终端交互）

```bash
klaw tui
```

如果要直接在终端看到 tracing / MCP bootstrap 日志，可改用：

```bash
klaw tui --verbose-terminal
```

- 在输入区编辑文本，回车提交；`Shift+Enter` 换行
- `Esc` 或 `Ctrl+C` 退出

### 单次请求（非交互式）

```bash
klaw agent --input "你的问题"
```

适合脚本调用和批量处理。

### Gateway 模式（WebSocket 网关）

```bash
klaw gateway
```

启动后连接 `ws://127.0.0.1:8080/ws/chat?session_key=your-room`。

支持 Tailscale 内网穿透，可以远程访问。

### Daemon 模式（用户级守护进程）

将 `klaw gateway` 注册为系统用户级服务，开机自启：

```bash
klaw daemon install
klaw daemon status
```

- `install` 会注册为当前用户的系统服务并立即启动
- macOS 使用 `launchd`，Linux 使用 `systemd --user`

如需停止或卸载：

```bash
klaw daemon stop
klaw daemon uninstall
```

## 常用工具配置

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

### 启用长期记忆

```toml
[tools.memory]
enabled = true
search_limit = 8
use_vector = true

[memory.embedding]
enabled = true
provider = "openai"
model = "text-embedding-3-small"

[memory.archive]
schedule = "0 0 2 * * *"
max_age_days = 30
summary_max_sources = 8
```

### 配置 Skills

从 GitHub 同步第三方 Skills：

```toml
[skills]
sync_timeout = 60

[skills.anthropic]
address = "https://github.com/anthropics/skills"

[skills.vercel]
address = "https://github.com/vercel-labs/skills"
installed = ["brainstorming"]
```

### 配置 MCP (Model Context Protocol)

```toml
[mcp]
servers_dir = "~/.klaw/mcp-servers"

[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "~/"]
```

### 配置语音（ASR/TTS）

```toml
# 启用语音输入
[voice.asr]
enabled = true
provider = "deepgram"
env_key = "DEEPGRAM_API_KEY"

# 启用语音输出
[voice.tts]
enabled = true
provider = "elevenlabs"
env_key = "ELEVENLABS_API_KEY"
voice_id = "pNInz6obpgDQGcFmaJgB"
```

支持的 ASR 提供商：`deepgram`、`assemblyai`
支持的 TTS 提供商：`elevenlabs`

## 配置管理

使用 `config` 命令查看和编辑配置：

```bash
# 显示当前配置路径和内容
klaw config show

# 打开编辑器编辑配置
klaw config edit

# 验证配置格式
klaw config validate
```

## 会话管理

```bash
# 列出所有会话
klaw session list

# 查看会话详情
klaw session get --session-key "terminal:my-chat"
```

## 归档管理

Klaw 支持归档媒体文件：

```bash
# 归档目录中的媒体文件
klaw archive import ./downloads

# 列出已归档文件
klaw archive list

# 搜索归档
klaw archive search --query "vacation"
```

## 下一步

- 阅读 [Agent Core 文档](./agent-core/README.md) 了解消息协议和运行时
- 查看 [工具文档](./tools/README.md) 了解可用工具
- 参考 [存储文档](./storage/README.md) 了解持久化配置
- 查看 [网关文档](./gateway/README.md) 了解远程部署

