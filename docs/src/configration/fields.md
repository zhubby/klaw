# 配置字段详解

本文档详细介绍 Klaw 的所有配置字段、默认值与使用示例。

## 目录

1. [模型配置](#模型配置)
2. [网关配置](#网关配置)
3. [渠道配置](#渠道配置)
4. [工具配置](#工具配置)
5. [存储配置](#存储配置)
6. [定时任务配置](#定时任务配置)
7. [心跳配置](#心跳配置)
8. [MCP 配置](#mcp 配置)
9. [Skills 配置](#skills 配置)
10. [内存配置](#内存配置)
11. [语音配置](#语音配置)
12. [ACP 配置](#acp 配置)
13. [可观测性配置](#可观测性配置)

---

## 模型配置

### `model_provider`

**类型**: `string`
**默认值**: `"openai"`
**必填**: 是

当前使用的模型 Provider 名称，必须与 `model_providers` 中的某个键匹配。

```toml
model_provider = "openai"
```

### `model`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

兼容旧配置的遗留字段。当前 runtime 默认路由不会再使用它来覆盖 Provider 的 `default_model`。

未显式执行 `/model` 的会话，默认模型始终跟随当前 Provider 的 `default_model` 变化。只有会话级显式 model override 才会脱离 Provider 默认值。

```toml
# legacy compatibility field; default routing ignores this value
model = "gpt-4-turbo"
```

### `conversation_history_limit`

**类型**: `usize`
**默认值**: `40`
**必填**: 否

控制每次发送给模型的历史消息窗口大小（按条数，保留最近 N 条）。

- `0`：不限制（会把会话历史全部传给模型）
- `N > 0`：只保留最近 `N` 条历史消息

当 `N > 0` 时，runtime 还会启用结构化增量压缩：

- 每新增 `N/2` 条消息（最小 1）触发一次压缩；
- 压缩摘要只用于运行时上下文替换，不会改写会话存储中的原始历史；
- 压缩失败时自动回退到普通滑动窗口，不影响主流程。

```toml
conversation_history_limit = 40  # 推荐默认值
```

### `model_providers.<name>`

**类型**: `BTreeMap<string, ModelProviderConfig>`
**必填**: 是

模型 Provider 配置映射。

#### `model_providers.<name>.base_url`

**类型**: `string`
**默认值**: `"https://api.openai.com/v1"`
**必填**: 是

API 基础 URL。

```toml
[model_providers.openai]
base_url = "https://api.openai.com/v1"

[model_providers.anthropic]
base_url = "https://api.anthropic.com"
```

#### `model_providers.<name>.wire_api`

**类型**: `string`
**默认值**: `"chat_completions"`
**必填**: 是

API 协议类型。

- `chat_completions`: OpenAI 兼容的聊天完成 API
- 其他值根据具体 Provider 而定

```toml
[model_providers.openai]
wire_api = "chat_completions"
```

#### `model_providers.<name>.default_model`

**类型**: `string`
**默认值**: `"gpt-4o-mini"`
**必填**: 是

Provider 的默认模型。

```toml
[model_providers.openai]
default_model = "gpt-4o-mini"
```

#### `model_providers.<name>.proxy`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用系统代理发送大模型请求。

- `false`（默认）：不走代理，强制直连。
- `true`：按系统代理环境变量（如 `HTTP_PROXY` / `HTTPS_PROXY`）发送请求。

```toml
[model_providers.openai]
proxy = true
```

#### `model_providers.<name>.api_key`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

直接配置 API 密钥（不推荐）。

```toml
[model_providers.openai]
api_key = "sk-xxx"  # 不推荐
```

#### `model_providers.<name>.env_key`

**类型**: `string` (可选)
**默认值**: `"OPENAI_API_KEY"`
**必填**: 条件必填

环境变量名，用于读取 API 密钥（推荐）。

```toml
[model_providers.openai]
env_key = "OPENAI_API_KEY"

# 设置环境变量
export OPENAI_API_KEY="sk-xxx"
```

---

## 网关配置

### `gateway.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用 gateway。`klaw gui` 启动时会根据此字段决定是否自动拉起内置 gateway。

```toml
[gateway]
enabled = true
```

### `gateway.listen_ip`

**类型**: `string`
**默认值**: `"127.0.0.1"`
**必填**: 是

WebSocket 网关监听 IP。

```toml
[gateway]
listen_ip = "127.0.0.1"  # 仅本地访问
# listen_ip = "0.0.0.0"  # 所有网络接口
```

### `gateway.listen_port`

**类型**: `u16`
**默认值**: `0`
**必填**: 是

WebSocket 网关监听端口。设置为 `0` 时，由系统分配随机可用端口。

```toml
[gateway]
listen_port = 0
```

### `gateway.webhook.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用 gateway webhook 输入能力。启用后会根据 endpoint 子配置分别注册 `POST <gateway.webhook.events.path>` 与 `POST <gateway.webhook.agents.path>`。

```toml
[gateway.webhook]
enabled = true
```

### `gateway.webhook.events.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用结构化事件入口。

```toml
[gateway.webhook.events]
enabled = true
```

### `gateway.webhook.events.path`

**类型**: `string`
**默认值**: `"/webhook/events"`
**必填**: 否

结构化事件入口路径，必须以 `/` 开头。

```toml
[gateway.webhook.events]
path = "/webhook/events"
```

### `gateway.webhook.events.max_body_bytes`

**类型**: `usize`
**默认值**: `262144`
**必填**: 否

结构化事件入口请求体大小限制，单位为字节，必须大于 `0`。

```toml
[gateway.webhook.events]
max_body_bytes = 262144
```

### `gateway.webhook.agents.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用模板驱动的 agent webhook 入口。

```toml
[gateway.webhook.agents]
enabled = true
```

### `gateway.webhook.agents.path`

**类型**: `string`
**默认值**: `"/webhook/agents"`
**必填**: 否

agent webhook 入口路径，必须以 `/` 开头。

```toml
[gateway.webhook.agents]
path = "/webhook/agents"
```

### `gateway.webhook.agents.max_body_bytes`

**类型**: `usize`
**默认值**: `262144`
**必填**: 否

agent webhook 入口请求体大小限制，单位为字节，必须大于 `0`。

```toml
[gateway.webhook.agents]
max_body_bytes = 262144
```

### `gateway.tls.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用 TLS。

```toml
[gateway.tls]
enabled = true
```

### `gateway.tls.cert_path`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 条件必填

TLS 证书路径（`enabled=true` 时必填）。

```toml
[gateway.tls]
enabled = true
cert_path = "/path/to/cert.pem"
```

### `gateway.tls.key_path`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 条件必填

TLS 私钥路径（`enabled=true` 时必填）。

```toml
[gateway.tls]
enabled = true
key_path = "/path/to/key.pem"
```

---

## 渠道配置

### `channels.dingtalk`

**类型**: `array`
**默认值**: `[]`
**必填**: 否

钉钉渠道配置列表。

```toml
[[channels.dingtalk]]
id = "default"
enabled = true
client_id = "your-app-key"
client_secret = "your-app-secret"
bot_title = "Klaw"
show_reasoning = false
stream_output = false
stream_template_id = ""
stream_content_key = "content"
stream_reasoning_key = "reasoning"
allowlist = ["USER123", "*"]
```

#### `channels.dingtalk[].id`

**类型**: `string`
**默认值**: `"default"`
**必填**: 是

渠道账户标识，不能重复。

```toml
[[channels.dingtalk]]
id = "company-a"

[[channels.dingtalk]]
id = "company-b"
```

#### `channels.dingtalk[].enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用该渠道。

```toml
channels.dingtalk.enabled = false
```

#### `channels.dingtalk[].client_id`

**类型**: `string`
**默认值**: `""`
**必填**: 是

钉钉应用 AppKey。

```toml
channels.dingtalk.client_id = "ding123456"
```

#### `channels.dingtalk[].client_secret`

**类型**: `string`
**默认值**: `""`
**必填**: 是

钉钉应用 AppSecret。

```toml
channels.dingtalk.client_secret = "secret123456"
```

#### `channels.dingtalk[].bot_title`

**类型**: `string`
**默认值**: `"Klaw"`
**必填**: 否

机器人显示名称。

```toml
channels.dingtalk.bot_title = "我的助手"
```

#### `channels.dingtalk[].show_reasoning`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否在响应中展示推理过程。

```toml
channels.dingtalk.show_reasoning = true
```

当 `stream_output=true` 且钉钉 AI 卡片模板包含独立 reasoning 变量时，开启该项会把模型流式返回的 reasoning 写入 `stream_reasoning_key` 指定的模板变量；正文变量不会再重复拼接 reasoning 块。

#### `channels.dingtalk[].stream_output`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否允许 DingTalk 渠道使用 AI 卡片模板流式刷新回复。

```toml
channels.dingtalk.stream_output = true
```

#### `channels.dingtalk[].stream_template_id`

**类型**: `string`
**默认值**: `""`
**必填**: `stream_output=true` 时必填

钉钉 AI 卡片模板 ID。

```toml
channels.dingtalk.stream_template_id = "template-1.schema"
```

#### `channels.dingtalk[].stream_content_key`

**类型**: `string`
**默认值**: `"content"`
**必填**: `stream_output=true` 时必填

钉钉 AI 卡片模板中承载正文内容的变量名。

```toml
channels.dingtalk.stream_content_key = "content"
```

#### `channels.dingtalk[].stream_reasoning_key`

**类型**: `string`
**默认值**: `"reasoning"`
**必填**: `stream_output=true` 时必填

钉钉 AI 卡片模板中承载 reasoning 内容的变量名。该变量会用全量覆盖方式刷新，适合每轮 reasoning 不同的展示块；不能与 `stream_content_key` 相同。

```toml
channels.dingtalk.stream_reasoning_key = "reasoning"
```

#### `channels.dingtalk[].allowlist`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

发送者白名单。`"*"` 表示允许所有用户。

```toml
# 仅允许特定用户
channels.dingtalk.allowlist = ["USER123", "USER456"]

# 允许所有用户
channels.dingtalk.allowlist = ["*"]
```

#### `channels.dingtalk[].proxy.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用代理。

```toml
[channels.dingtalk.proxy]
enabled = true
url = "http://proxy.example.com:8080"
```

#### `channels.dingtalk[].proxy.url`

**类型**: `string`
**默认值**: `""`
**必填**: 条件必填

代理 URL（`proxy.enabled=true` 时必填）。

```toml
[channels.dingtalk.proxy]
enabled = true
url = "http://127.0.0.1:8888"
```

### `channels.telegram`

**类型**: `array`
**默认值**: `[]`
**必填**: 否

Telegram 渠道配置列表。

```toml
[[channels.telegram]]
id = "default"
enabled = true
bot_token = "123456:ABCDEF"
show_reasoning = false
allowlist = ["*"]
```

#### `channels.telegram[].id`

**类型**: `string`
**默认值**: `"default"`
**必填**: 是

渠道账户标识，不能重复。

#### `channels.telegram[].enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用该渠道。

#### `channels.telegram[].bot_token`

**类型**: `string`
**默认值**: `""`
**必填**: 是

Telegram Bot Token。

#### `channels.telegram[].show_reasoning`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否在响应中展示推理过程。

#### `channels.telegram[].allowlist`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

发送者白名单。`"*"` 表示允许所有用户。

```toml
channels.telegram.allowlist = ["123456789", "*"]
```

#### `channels.telegram[].proxy.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用代理。

```toml
[channels.telegram.proxy]
enabled = true
url = "http://proxy.example.com:8080"
```

#### `channels.telegram[].proxy.url`

**类型**: `string`
**默认值**: `""`
**必填**: 条件必填

代理 URL（`proxy.enabled=true` 时必填）。

```toml
[channels.telegram.proxy]
enabled = true
url = "http://127.0.0.1:8888"
```

### `channels.websocket`

**类型**: `array`
**默认值**: `[]`
**必填**: 否

WebSocket 渠道配置列表。用于配置外部客户端（如网页 UI、自定义集成）的接入。

```toml
[[channels.websocket]]
id = "browser"
enabled = true
show_reasoning = false
stream_output = true
```

#### `channels.websocket[].id`

**类型**: `string`
**默认值**: `"default"`
**必填**: 是

渠道标识，不能重复。

```toml
[[channels.websocket]]
id = "web-ui"
```

#### `channels.websocket[].enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用该渠道。

```toml
channels.websocket.enabled = false
```

#### `channels.websocket[].show_reasoning`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否在响应中展示推理过程。

```toml
channels.websocket.show_reasoning = true
```

#### `channels.websocket[].stream_output`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否流式输出 token。开启后，token 生成时会逐帧发送给客户端；关闭则一次性发送完整响应。

```toml
channels.websocket.stream_output = false
```

### `channels.disable_session_commands_for`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

禁用会话命令的渠道列表。

```toml
channels.disable_session_commands_for = ["dingtalk", "telegram", "terminal"]
```

---

## 工具配置

### `tools.archive`

Archive 归档工具配置。

#### `tools.archive.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用 `archive` 工具。

该工具用于：

- 列出当前消息中的归档附件
- 查询归档记录
- 只读读取文本类归档文件
- 将归档文件复制到 `workspace/` 后再编辑

```toml
tools.archive.enabled = true
```

### `tools.shell`

Shell 执行工具配置。

#### `tools.shell.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用 Shell 工具。

```toml
tools.shell.enabled = false
```

#### `tools.shell.workspace`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

工作目录。
未显式设置且请求 metadata 也未提供 `workspace` 时，将回退到 `(<storage.root_dir 或 ~/.klaw>)/workspace`。

```toml
tools.shell.workspace = "/path/to/workspace"
```

#### `tools.shell.blocked_patterns`

**类型**: `array<string>`
**默认值**: `[":(){ :|:& };:"]`
**必填**: 否

直接拒绝执行的命令模式列表。

```toml
tools.shell.blocked_patterns = [":(){ :|:& };:"]
```

#### `tools.shell.unsafe_patterns`

**类型**: `array<string>`
**默认值**: `["rm -rf /", "rm -rf ~", "mkfs", "shutdown", "reboot"]`
**必填**: 否

需要审批的危险命令模式列表。

```toml
tools.shell.unsafe_patterns = ["rm -rf", "dd if=", "mkfs"]
```

`tools.shell` 现在按顺序应用规则：先匹配 `blocked_patterns` 并直接拒绝，再匹配 `unsafe_patterns` 请求审批，未命中任一模式的命令默认允许执行。

#### `tools.shell.allow_login_shell`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否允许使用 login shell。

```toml
tools.shell.allow_login_shell = false
```

#### `tools.shell.max_timeout_ms`

**类型**: `u64`
**默认值**: `120000` (2 分钟)
**必填**: 否

最大执行超时（毫秒）。

```toml
tools.shell.max_timeout_ms = 60000  # 1 分钟
```

#### `tools.shell.max_output_bytes`

**类型**: `usize`
**默认值**: `131072` (128KB)
**必填**: 否

最大输出字节数。

```toml
tools.shell.max_output_bytes = 262144  # 256KB
```

### `tools.apply_patch`

文件编辑工具配置。

#### `tools.apply_patch.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用 Apply Patch 工具。

```toml
tools.apply_patch.enabled = false
```

#### `tools.apply_patch.workspace`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

工作目录。
未显式设置且请求 metadata 也未提供 `workspace` 时，将回退到 `(<storage.root_dir 或 ~/.klaw>)/workspace`。

```toml
tools.apply_patch.workspace = "/path/to/project"
```

#### `tools.apply_patch.allow_absolute_paths`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否允许绝对路径。

```toml
tools.apply_patch.allow_absolute_paths = true
```

#### `tools.apply_patch.allowed_roots`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

允许的根目录列表。

```toml
tools.apply_patch.allowed_roots = ["/path/to/project", "/tmp"]
```

### `tools.approval`

审批工具配置。

#### `tools.approval.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用审批工具。

```toml
tools.approval.enabled = false
```

### `tools.geo`

地理位置工具配置。

#### `tools.geo.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用 `geo` 工具。

该工具用于：

- 通过系统定位服务获取当前环境坐标
- 返回经纬度、精度和定位时间戳
- 在权限被拒或定位失败时返回明确错误

```toml
tools.geo.enabled = false
```

### `tools.local_search`

本地搜索工具配置。

#### `tools.local_search.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用本地搜索工具。

```toml
tools.local_search.enabled = false
```

### `tools.terminal_multiplexers`

终端复用器工具配置。

#### `tools.terminal_multiplexers.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用终端复用器工具。

```toml
tools.terminal_multiplexers.enabled = false
```

### `tools.cron_manager`

定时任务管理工具配置。

#### `tools.cron_manager.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用定时任务管理工具。

```toml
tools.cron_manager.enabled = false
```

### `tools.skills_registry`

只读 Skills 注册表工具配置。

#### `tools.skills_registry.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用只读 Skills 注册表工具。

```toml
tools.skills_registry.enabled = false
```

### `tools.skills_manager`

已安装 Skills 管理工具配置。

#### `tools.skills_manager.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用已安装 Skills 管理工具。

```toml
tools.skills_manager.enabled = false
```

### `tools.memory`

记忆工具配置。

#### `tools.memory.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用记忆工具。

```toml
tools.memory.enabled = false
```

#### `tools.memory.search_limit`

**类型**: `usize`
**默认值**: `8`
**必填**: 否

全文搜索返回结果数量上限。

```toml
tools.memory.search_limit = 20
```

#### `tools.memory.fts_limit`

**类型**: `usize`
**默认值**: `20`
**必填**: 否

全文检索返回结果数量上限。

```toml
tools.memory.fts_limit = 50
```

#### `tools.memory.vector_limit`

**类型**: `usize`
**默认值**: `20`
**必填**: 否

向量搜索返回结果数量上限。

```toml
tools.memory.vector_limit = 50
```

#### `tools.memory.use_vector`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否使用向量搜索。

```toml
tools.memory.use_vector = false
```

### `tools.web_fetch`

网页抓取工具配置。

#### `tools.web_fetch.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用网页抓取工具。

```toml
tools.web_fetch.enabled = false
```

#### `tools.web_fetch.max_chars`

**类型**: `usize`
**默认值**: `50000`
**必填**: 否

单次抓取最大字符数。

```toml
tools.web_fetch.max_chars = 100000
```

#### `tools.web_fetch.timeout_seconds`

**类型**: `u64`
**默认值**: `15`
**必填**: 否

抓取超时（秒）。

```toml
tools.web_fetch.timeout_seconds = 30
```

#### `tools.web_fetch.cache_ttl_minutes`

**类型**: `u64`
**默认值**: `10`
**必填**: 否

抓取缓存有效期（分钟）。

```toml
tools.web_fetch.cache_ttl_minutes = 60
```

#### `tools.web_fetch.max_redirects`

**类型**: `u8`
**默认值**: `3`
**必填**: 否

最大重定向次数。

```toml
tools.web_fetch.max_redirects = 5
```

#### `tools.web_fetch.readability`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否使用 Readability 提取正文。

```toml
tools.web_fetch.readability = false
```

#### `tools.web_fetch.ssrf_allowlist`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

SSRF 白名单域名列表。

```toml
tools.web_fetch.ssrf_allowlist = ["example.com", "api.github.com"]
```

### `tools.web_search`

网页搜索工具配置。

#### `tools.web_search.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用网页搜索工具。

```toml
tools.web_search.enabled = false
```

#### `tools.web_search.provider`

**类型**: `string`
**默认值**: `"tavily"`
**必填**: 否

搜索服务提供商。

- `tavily`: Tavily Search API
- `brave`: Brave Search API

```toml
tools.web_search.provider = "brave"
```

#### `tools.web_search.tavily`

Tavily 搜索配置。

##### `tools.web_search.tavily.base_url`

**类型**: `string` (可选)
**默认值**: `"https://api.tavily.com"`
**必填**: 否

API 基础 URL。

```toml
tools.web_search.tavily.base_url = "https://api.tavily.com"
```

##### `tools.web_search.tavily.api_key`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 条件必填

API 密钥（与 `env_key` 二选一）。

```toml
tools.web_search.tavily.api_key = "tvly-xxx"
```

##### `tools.web_search.tavily.env_key`

**类型**: `string` (可选)
**默认值**: `"TAVILY_API_KEY"`
**必填**: 条件必填

环境变量名（与 `api_key` 二选一）。

```toml
tools.web_search.tavily.env_key = "TAVILY_API_KEY"
```

##### `tools.web_search.tavily.search_depth`

**类型**: `string`
**默认值**: `"basic"`
**必填**: 否

搜索深度。

- `basic`: 基础搜索
- `advanced`: 深度搜索

```toml
tools.web_search.tavily.search_depth = "advanced"
```

##### `tools.web_search.tavily.topic`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

搜索主题过滤。

```toml
tools.web_search.tavily.topic = "news"
```

##### `tools.web_search.tavily.include_answer`

**类型**: `boolean` (可选)
**默认值**: `null`
**必填**: 否

是否包含 AI 答案。

```toml
tools.web_search.tavily.include_answer = true
```

##### `tools.web_search.tavily.include_raw_content`

**类型**: `boolean` (可选)
**默认值**: `null`
**必填**: 否

是否包含原始内容。

```toml
tools.web_search.tavily.include_raw_content = true
```

##### `tools.web_search.tavily.include_images`

**类型**: `boolean` (可选)
**默认值**: `null`
**必填**: 否

是否包含图片。

```toml
tools.web_search.tavily.include_images = true
```

#### `tools.web_search.brave`

Brave 搜索配置。

##### `tools.web_search.brave.base_url`

**类型**: `string` (可选)
**默认值**: `"https://api.search.brave.com"`
**必填**: 否

API 基础 URL。

```toml
tools.web_search.brave.base_url = "https://api.search.brave.com"
```

##### `tools.web_search.brave.api_key`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 条件必填

API 密钥（与 `env_key` 二选一）。

```toml
tools.web_search.brave.api_key = "BSAxxx"
```

##### `tools.web_search.brave.env_key`

**类型**: `string` (可选)
**默认值**: `"BRAVE_SEARCH_API_KEY"`
**必填**: 条件必填

环境变量名（与 `api_key` 二选一）。

```toml
tools.web_search.brave.env_key = "BRAVE_SEARCH_API_KEY"
```

##### `tools.web_search.brave.country`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

搜索结果国家代码。

```toml
tools.web_search.brave.country = "us"
```

##### `tools.web_search.brave.search_lang`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

搜索语言。

```toml
tools.web_search.brave.search_lang = "en"
```

##### `tools.web_search.brave.ui_lang`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

界面语言。

```toml
tools.web_search.brave.ui_lang = "en"
```

##### `tools.web_search.brave.safesearch`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

安全搜索级别。

- `off`: 关闭
- `moderate`: 中等
- `strict`: 严格

```toml
tools.web_search.brave.safesearch = "moderate"
```

##### `tools.web_search.brave.freshness`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

结果新鲜度。

- `any`: 任意时间
- `past_day`: 过去 24 小时
- `past_week`: 过去一周
- `past_month`: 过去一个月
- `past_year`: 过去一年

```toml
tools.web_search.brave.freshness = "past_week"
```

### `tools.sub_agent`

子代理工具配置。

#### `tools.sub_agent.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用子代理工具。

```toml
tools.sub_agent.enabled = false
```

#### `tools.sub_agent.max_iterations`

**类型**: `u32`
**默认值**: `6`
**必填**: 否

子代理最大迭代次数。

```toml
tools.sub_agent.max_iterations = 10
```

#### `tools.sub_agent.max_tool_calls`

**类型**: `u32`
**默认值**: `12`
**必填**: 否

子代理单次执行最大工具调用次数。

```toml
tools.sub_agent.max_tool_calls = 20
```

#### `tools.sub_agent.inherit_parent_tools`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否继承父代理的工具。

```toml
tools.sub_agent.inherit_parent_tools = false
```

#### `tools.sub_agent.exclude_tools`

**类型**: `array<string>`
**默认值**: `["sub_agent"]`
**必填**: 否

排除的工具列表。

```toml
tools.sub_agent.exclude_tools = ["sub_agent", "shell"]
```

---

## 存储配置

### `storage.root_dir`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

存储根目录。

```toml
[storage]
root_dir = "~/.klaw"
```

---

## 定时任务配置

### `cron.tick_ms`

**类型**: `u64`
**默认值**: `1000` (1 秒)
**必填**: 否

Cron 任务检查间隔（毫秒）。

```toml
[cron]
tick_ms = 500  # 0.5 秒
```

### `cron.runtime_tick_ms`

**类型**: `u64`
**默认值**: `200`
**必填**: 否

运行时任务检查间隔（毫秒）。

```toml
[cron]
runtime_tick_ms = 100  # 0.1 秒
```

### `cron.runtime_drain_batch`

**类型**: `usize`
**默认值**: `8`
**必填**: 否

运行时任务批次处理大小。

```toml
[cron]
runtime_drain_batch = 16
```

### `cron.batch_limit`

**类型**: `i64`
**默认值**: `64`
**必填**: 否

批次处理上限。

```toml
[cron]
batch_limit = 128
```

---

## 心跳配置

### `heartbeat.defaults.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用心跳。

```toml
[heartbeat.defaults]
enabled = false
```

### `heartbeat.defaults.every`

**类型**: `string`
**默认值**: `"30m"`
**必填**: 否

心跳间隔（人类可读时长）。

```toml
[heartbeat.defaults]
every = "1h"    # 1 小时
every = "15m"   # 15 分钟
every = "1d"    # 1 天
```

### `heartbeat.defaults.prompt`

**类型**: `string`
**默认值**: `"Review the session state. If no user-visible action is needed, reply with exactly HEARTBEAT_OK and nothing else."`
**必填**: 否

心跳的默认自定义提示词。运行时会在这段内容后面追加固定 heartbeat 指令，并要求在无需用户可见动作时只返回静默 token。

```toml
[heartbeat.defaults]
prompt = "检查是否有新的待处理提醒、未读消息或需要主动告知用户的事项。"
```

### `heartbeat.defaults.silent_ack_token`

**类型**: `string`
**默认值**: `"HEARTBEAT_OK"`
**必填**: 否

静默确认标记（匹配后不通知用户）。

```toml
[heartbeat.defaults]
silent_ack_token = "HEARTBEAT_OK"
```

### `heartbeat.defaults.timezone`

**类型**: `string`
**默认值**: `"UTC"`
**必填**: 否

时区设置。

```toml
[heartbeat.defaults]
timezone = "Asia/Shanghai"
```

### `heartbeat.sessions[]`

**类型**: `array`
**默认值**: `[]`
**必填**: 否

特定会话的心跳配置（覆盖默认值）。

```toml
[[heartbeat.sessions]]
session_key = "dingtalk:default:USER123"
chat_id = "USER123"
channel = "dingtalk"
every = "1h"
prompt = "专属提示词..."
silent_ack_token = "OK"
timezone = "Asia/Shanghai"
```

#### `heartbeat.sessions[].session_key`

**类型**: `string`
**必填**: 是

会话标识。

#### `heartbeat.sessions[].chat_id`

**类型**: `string`
**必填**: 是

聊天标识。

#### `heartbeat.sessions[].channel`

**类型**: `string`
**必填**: 是

渠道名称。

#### `heartbeat.sessions[].every`

**类型**: `string` (可选)
**必填**: 否

心跳间隔（覆盖默认值）。

#### `heartbeat.sessions[].prompt`

**类型**: `string` (可选)
**必填**: 否

心跳提示词（覆盖默认值）。

#### `heartbeat.sessions[].silent_ack_token`

**类型**: `string` (可选)
**必填**: 否

静默确认标记（覆盖默认值）。

#### `heartbeat.sessions[].timezone`

**类型**: `string` (可选)
**必填**: 否

时区（覆盖默认值）。

---

## MCP 配置

### `mcp.startup_timeout_seconds`

**类型**: `u64`
**默认值**: `60`
**必填**: 否

MCP 服务启动超时（秒）。

```toml
[mcp]
startup_timeout_seconds = 120
```

### `mcp.servers[]`

**类型**: `array`
**默认值**: `[]`
**必填**: 否

MCP 服务器列表。

```toml
[[mcp.servers]]
id = "filesystem"
enabled = true
mode = "stdio"
tool_timeout_seconds = 45
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/data"]

[[mcp.servers]]
id = "remote-server"
enabled = true
mode = "sse"
url = "http://localhost:8080/sse"
headers = { "Authorization" = "Bearer xxx" }
```

#### `mcp.servers[].id`

**类型**: `string`
**必填**: 是

服务器标识，不能重复。

```toml
mcp.servers.id = "filesystem"
```

#### `mcp.servers[].enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用该服务器。

```toml
mcp.servers.enabled = false
```

#### `mcp.servers[].mode`

**类型**: `enum`
**必填**: 是

服务器模式。

- `stdio`: 标准输入输出
- `sse`: Server-Sent Events

```toml
mcp.servers.mode = "stdio"
```

#### `mcp.servers[].tool_timeout_seconds`

**类型**: `u64`
**默认值**: `60`
**必填**: 否

该 MCP server 上 `tools/call` 调用的执行超时（秒）。

```toml
mcp.servers.tool_timeout_seconds = 60
```

#### `mcp.servers[].command`

**类型**: `string` (可选)
**必填**: 条件必填

启动命令（`mode=stdio` 时必填）。

```toml
mcp.servers.command = "npx"
```

#### `mcp.servers[].args`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

命令参数。

```toml
mcp.servers.args = ["-y", "@modelcontextprotocol/server-filesystem"]
```

#### `mcp.servers[].env`

**类型**: `map<string, string>`
**默认值**: `{}`
**必填**: 否

环境变量。

```toml
mcp.servers.env = { "NODE_ENV" = "production" }
```

#### `mcp.servers[].cwd`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

工作目录。

```toml
mcp.servers.cwd = "/path/to/working/dir"
```

#### `mcp.servers[].url`

**类型**: `string` (可选)
**必填**: 条件必填

SSE 端点 URL（`mode=sse` 时必填）。

```toml
mcp.servers.url = "http://localhost:8080/sse"
```

#### `mcp.servers[].headers`

**类型**: `map<string, string>`
**默认值**: `{}`
**必填**: 否

HTTP 请求头。

```toml
mcp.servers.headers = { "Authorization" = "Bearer xxx" }
```

---

## Skills 配置

### `skills.sync_timeout`

**类型**: `u64`
**默认值**: `60`
**必填**: 否

Skills 同步超时（秒）。

```toml
[skills]
sync_timeout = 120
```

### `skills.registries.<name>`

**类型**: `map<string, SkillsRegistryConfig>`
**默认值**: `{"anthropic": {...}}`
**必填**: 否

Skills 注册表配置。

```toml
[skills.registries.anthropic]
address = "https://github.com/anthropics/skills"
installed = ["skill-1", "skill-2"]
```

#### `skills.registries.<name>.address`

**类型**: `string`
**必填**: 是

注册表地址（Git 仓库 URL）。

```toml
skills.registries.anthropic.address = "https://github.com/anthropics/skills"
```

#### `skills.registries.<name>.installed`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

已安装的 Skills 列表。

```toml
skills.registries.anthropic.installed = ["debugging", "testing"]
```

---

## 内存配置

### `memory.embedding.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用嵌入功能。

```toml
[memory.embedding]
enabled = true
```

### `memory.embedding.provider`

**类型**: `string`
**默认值**: `"openai"`
**必填**: 条件必填

嵌入 Provider（`enabled=true` 时必填）。

```toml
[memory.embedding]
provider = "openai"
```

### `memory.embedding.model`

**类型**: `string`
**默认值**: `"text-embedding-3-small"`
**必填**: 条件必填

嵌入模型（`enabled=true` 时必填）。

```toml
[memory.embedding]
model = "text-embedding-3-small"
```

### `memory.archive.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用长期记忆自动归档与摘要。

```toml
[memory.archive]
enabled = true
```

### `memory.archive.schedule`

**类型**: `string`
**默认值**: `"0 0 2 * * *"`
**必填**: 条件必填

自动归档的 cron 表达式，按系统时区解释。默认表示每天凌晨 `2:00`。

```toml
[memory.archive]
schedule = "0 0 2 * * *"
```

### `memory.archive.max_age_days`

**类型**: `integer`
**默认值**: `30`
**必填**: 否

超过多少天未更新的低优先级长期记忆会进入归档候选集。

```toml
[memory.archive]
max_age_days = 30
```

### `memory.archive.summary_max_sources`

**类型**: `integer`
**默认值**: `8`
**必填**: 否

单条归档摘要最多内联多少条来源内容片段。

```toml
[memory.archive]
summary_max_sources = 8
```

---

## 语音配置

语音功能（语音转文字 STT 和文字转语音 TTS）配置。

### `voice.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用语音功能。语音工具只有在 `voice.enabled` **且** `tools.voice.enabled` 同时为 true 时才会注册。

```toml
[voice]
enabled = true
```

### `voice.stt_provider`

**类型**: `enum`
**默认值**: `"deepgram"`
**必填**: 否

语音转文字（STT）服务商。

可选值：
- `deepgram`: Deepgram API
- `assemblyai`: AssemblyAI API

```toml
[voice]
stt_provider = "deepgram"
```

### `voice.tts_provider`

**类型**: `enum`
**默认值**: `"elevenlabs"`
**必填**: 否

文字转语音（TTS）服务商。

可选值：
- `elevenlabs`: ElevenLabs API

```toml
[voice]
tts_provider = "elevenlabs"
```

### `voice.default_language`

**类型**: `string`
**默认值**: `"zh"`
**必填**: 否

默认语音识别语言代码。

```toml
[voice]
default_language = "en"
```

### `voice.default_voice_id`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

默认 TTS 语音 ID。

```toml
[voice]
default_voice_id = "pNInz6obpgDQGcFmaJgB"
```

### `voice.providers.elevenlabs`

ElevenLabs TTS 配置。

#### `voice.providers.elevenlabs.api_key`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 条件必填

API 密钥（与 `api_key_env` 二选一）。

```toml
[voice.providers.elevenlabs]
api_key = "sk_xxx"
```

#### `voice.providers.elevenlabs.api_key_env`

**类型**: `string`
**默认值**: `"ELEVENLABS_API_KEY"`
**必填**: 否

环境变量名，用于读取 API 密钥（推荐）。

```toml
[voice.providers.elevenlabs]
api_key_env = "ELEVENLABS_API_KEY"
```

#### `voice.providers.elevenlabs.base_url`

**类型**: `string`
**默认值**: `"https://api.elevenlabs.io/v1"`
**必填**: 否

API 基础 URL。

```toml
[voice.providers.elevenlabs]
base_url = "https://api.elevenlabs.io/v1"
```

#### `voice.providers.elevenlabs.streaming_base_url`

**类型**: `string`
**默认值**: `"wss://api.elevenlabs.io/v1"`
**必填**: 否

流式输出 WebSocket 基础 URL。

```toml
[voice.providers.elevenlabs]
streaming_base_url = "wss://api.elevenlabs.io/v1"
```

#### `voice.providers.elevenlabs.default_model`

**类型**: `string`
**默认值**: `"eleven_multilingual_v2"`
**必填**: 否

默认 TTS 模型。

```toml
[voice.providers.elevenlabs]
default_model = "eleven_multilingual_v2"
```

#### `voice.providers.elevenlabs.default_voice_id`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

默认语音 ID（覆盖顶层 `voice.default_voice_id`）。

```toml
[voice.providers.elevenlabs]
default_voice_id = "pNInz6obpgDQGcFmaJgB"
```

### `voice.providers.deepgram`

Deepgram STT 配置。

#### `voice.providers.deepgram.api_key`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 条件必填

API 密钥（与 `api_key_env` 二选一）。

```toml
[voice.providers.deepgram]
api_key = "xxx"
```

#### `voice.providers.deepgram.api_key_env`

**类型**: `string`
**默认值**: `"DEEPGRAM_API_KEY"`
**必填**: 否

环境变量名，用于读取 API 密钥。

```toml
[voice.providers.deepgram]
api_key_env = "DEEPGRAM_API_KEY"
```

#### `voice.providers.deepgram.base_url`

**类型**: `string`
**默认值**: `"https://api.deepgram.com"`
**必填**: 否

API 基础 URL。

```toml
[voice.providers.deepgram]
base_url = "https://api.deepgram.com"
```

#### `voice.providers.deepgram.streaming_base_url`

**类型**: `string`
**默认值**: `"wss://api.deepgram.com"`
**必填**: 否

流式识别 WebSocket 基础 URL。

```toml
[voice.providers.deepgram]
streaming_base_url = "wss://api.deepgram.com"
```

#### `voice.providers.deepgram.stt_model`

**类型**: `string`
**默认值**: `"nova-2"`
**必填**: 否

STT 模型名称。

```toml
[voice.providers.deepgram]
stt_model = "nova-3"
```

### `voice.providers.assemblyai`

AssemblyAI STT 配置。

#### `voice.providers.assemblyai.api_key`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 条件必填

API 密钥（与 `api_key_env` 二选一）。

```toml
[voice.providers.assemblyai]
api_key = "xxx"
```

#### `voice.providers.assemblyai.api_key_env`

**类型**: `string`
**默认值**: `"ASSEMBLYAI_API_KEY"`
**必填**: 否

环境变量名，用于读取 API 密钥。

```toml
[voice.providers.assemblyai]
api_key_env = "ASSEMBLYAI_API_KEY"
```

#### `voice.providers.assemblyai.base_url`

**类型**: `string`
**默认值**: `"https://api.assemblyai.com"`
**必填**: 否

API 基础 URL。

```toml
[voice.providers.assemblyai]
base_url = "https://api.assemblyai.com"
```

#### `voice.providers.assemblyai.streaming_base_url`

**类型**: `string`
**默认值**: `"wss://api.assemblyai.com"`
**必填**: 否

流式识别 WebSocket 基础 URL。

```toml
[voice.providers.assemblyai]
streaming_base_url = "wss://api.assemblyai.com"
```

#### `voice.providers.assemblyai.stt_model`

**类型**: `string`
**默认值**: `"best"`
**必填**: 否

STT 模型名称。

```toml
[voice.providers.assemblyai]
stt_model = "nova"
```

---

## ACP 配置

ACP (Agent Connect Protocol) 允许接入外部 Agent 作为工具被主 Agent 调度。

### `acp.startup_timeout_seconds`

**类型**: `u64`
**默认值**: `60`
**必填**: 否

ACP Agent 启动超时（秒）。

```toml
[acp]
startup_timeout_seconds = 120
```

### `acp.agents[]`

**类型**: `array`
**默认值**: `[]`
**必填**: 否

ACP Agent 配置列表。

```toml
[[acp.agents]]
id = "my-agent"
enabled = true
command = "node"
args = ["dist/agent.js", "--port", "8080"]
description = "Custom domain agent"
env = { "API_KEY" = "xxx" }
```

#### `acp.agents[].id`

**类型**: `string`
**必填**: 是

Agent 唯一标识。

```toml
acp.agents.id = "domain-agent"
```

#### `acp.agents[].enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用该 Agent。

```toml
acp.agents.enabled = false
```

#### `acp.agents[].command`

**类型**: `string`
**必填**: 是

启动命令。

```toml
acp.agents.command = "python"
```

#### `acp.agents[].args`

**类型**: `array<string>`
**默认值**: `[]`
**必填**: 否

命令参数。

```toml
acp.agents.args = ["agent.py", "--serve"]
```

#### `acp.agents[].env`

**类型**: `map<string, string>`
**默认值**: `{}`
**必填**: 否

环境变量。

```toml
acp.agents.env = { "OPENAI_API_KEY" = "$env:OPENAI_API_KEY" }
```

#### `acp.agents[].description`

**类型**: `string`
**默认值**: `""`
**必填**: 否

Agent 描述，用于工具调用时让大模型理解这个 Agent 的用途。

```toml
acp.agents.description = "Handles domain-specific knowledge queries"
```

---

## 可观测性配置

可观测性（OpenTelemetry metrics/traces、审计日志、Prometheus 导出）配置。

### `observability.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用可观测性功能。

```toml
[observability]
enabled = true
```

### `observability.service_name`

**类型**: `string`
**默认值**: `"klaw"`
**必填**: 否

服务名称，用于 metrics/traces。

```toml
[observability]
service_name = "klaw-production"
```

### `observability.service_version`

**类型**: `string`
**默认值**: `"0.1.0"`
**必填**: 否

服务版本，用于 metrics/traces。

```toml
[observability]
service_version = "0.11.0"
```

### `observability.metrics`

指标配置。

#### `observability.metrics.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用指标导出。

```toml
[observability.metrics]
enabled = true
```

#### `observability.metrics.export_interval_seconds`

**类型**: `u64`
**默认值**: `15`
**必填**: 否

指标导出间隔（秒）。

```toml
[observability.metrics]
export_interval_seconds = 30
```

### `observability.traces`

链路追踪配置。

#### `observability.traces.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用链路追踪。

```toml
[observability.traces]
enabled = true
```

#### `observability.traces.sample_rate`

**类型**: `f64`
**默认值**: `1.0`
**必填**: 否

采样率 (0.0 - 1.0)。`1.0` 表示全采样。

```toml
[observability.traces]
sample_rate = 0.5
```

### `observability.otlp`

OpenTelemetry OTLP 导出配置。

#### `observability.otlp.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用 OTLP 导出。

```toml
[observability.otlp]
enabled = true
```

#### `observability.otlp.endpoint`

**类型**: `string`
**默认值**: `"http://localhost:4317"`
**必填**: 否

OTLP gRPC 端点。

```toml
[observability.otlp]
endpoint = "http://otel-collector:4317"
```

#### `observability.otlp.headers`

**类型**: `map<string, string>`
**默认值**: `{}`
**必填**: 否

OTLP 请求头。

```toml
[observability.otlp.headers]
Authorization = "Bearer xxx"
```

### `observability.prometheus`

Prometheus 拉取配置。

#### `observability.prometheus.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用 Prometheus 端点暴露。

```toml
[observability.prometheus]
enabled = true
```

#### `observability.prometheus.listen_port`

**类型**: `u16`
**默认值**: `9464`
**必填**: 否

监听端口。

```toml
[observability.prometheus]
listen_port = 9464
```

#### `observability.prometheus.path`

**类型**: `string`
**默认值**: `"/metrics"`
**必填**: 否

metrics 端点路径。

```toml
[observability.prometheus]
path = "/metrics"
```

### `observability.audit`

审计日志配置。

#### `observability.audit.enabled`

**类型**: `boolean`
**默认值**: `false`
**必填**: 否

是否启用审计日志（记录所有 LLM 请求/响应）。

```toml
[observability.audit]
enabled = true
```

#### `observability.audit.output_path`

**类型**: `string` (可选)
**默认值**: `null`
**必填**: 否

审计日志输出路径。`null` 使用默认路径。

```toml
[observability.audit]
output_path = "/var/log/klaw/audit.log"
```

### `observability.local_store`

本地存储配置。

#### `observability.local_store.enabled`

**类型**: `boolean`
**默认值**: `true`
**必填**: 否

是否启用本地可观测性存储。

```toml
[observability.local_store]
enabled = true
```

#### `observability.local_store.retention_days`

**类型**: `u16`
**默认值**: `30`
**必填**: 否

数据保留天数。

```toml
[observability.local_store]
retention_days = 90
```

#### `observability.local_store.flush_interval_seconds`

**类型**: `u64`
**默认值**: `10`
**必填**: 否

刷盘间隔（秒）。

```toml
[observability.local_store]
flush_interval_seconds = 30
```
