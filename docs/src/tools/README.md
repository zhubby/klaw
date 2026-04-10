# 工具文档

Klaw 工具系统提供可扩展的 AI 工具抽象，所有工具实现 `Tool` trait：

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn category(&self) -> ToolCategory;
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput>;
}
```

## 工具分类

### 内置工具

基础执行能力，默认启用：

- [Archive](./built-in/archive.md) - 媒体归档管理，导入、检索、导出文件
- [Ask Question](./built-in/ask_question.md) - 创建带选项的人工询问，等待用户回复后继续执行
- [Apply Patch](./built-in/apply_patch.md) - 批量文件编辑（添加、更新、删除、移动）
- [Channel Attachment](./built-in/channel_attachment.md) - 在输出渠道发送附件（图片、文件）
- [Approval](./built-in/approval.md) - 高风险操作的审批记录管理（请求、查询、决议）
- [Geo](./built-in/geo.md) - 通过系统定位服务获取当前环境坐标
- [Local Search](./built-in/local_search.md) - `rg` 优先、`grep` fallback 的代码检索
- [Memory](./built-in/memory.md) - 长期记忆存储与检索
- [Shell](./built-in/shell.md) - 可控的 shell 执行，支持阻断模式和工作区边界控制
- [Voice](./built-in/voice.md) - 语音识别（ASR）和语音合成（TTS）

### Web 工具

外部信息获取：

- [Web Fetch](./web/web_fetch.md) - 单页面内容抓取，支持 SSRF 防护
- [Web Search](./web/web_search.md) - 多 Provider 搜索（Tavily/Brave）

### 高级工具

复杂编排能力：

- [Cron Manager](./advanced/cron_manager.md) - 定时任务管理（创建、列出、删除）
- [Heartbeat 调度器](./advanced/heartbeat.md) - 心跳触发的延迟任务调度
- [Skills Manager](./advanced/skills_manager.md) - Skills 安装、卸载、同步管理
- [Skills Registry](./advanced/skills.md) - Skills 注册表查询
- [Sub Agent](./advanced/sub_agent.md) - 子代理委托执行
- [Terminal Multiplexer](./advanced/terminal_multiplexer.md) - 基于 tmux 的交互式终端会话管理

## 工具配置

在 `~/.klaw/config.toml` 中配置：

```toml
[tools.archive]
enabled = true

[tools.ask_question]
enabled = true
default_expires_minutes = 60

[tools.geo]
enabled = true

[tools.shell]
enabled = true
blocked_patterns = [":(){ :|:& };:"]
unsafe_patterns = ["rm -rf /", "mkfs"]
max_timeout_ms = 120000

[tools.voice]
enabled = true

[tools.web_search]
enabled = true
provider = "tavily"

[tools.memory]
enabled = true
search_limit = 8
use_vector = true

[tools.terminal_multiplexer]
enabled = true
socket_dir = "~/.klaw/tmux-sockets"
```

## 工具上下文

工具执行时可访问：

- `session_key` - 当前会话标识
- `channel` - 来源渠道
- `metadata` - 扩展元数据

## 输出格式

工具输出统一为结构化数据，包含：

- `content_for_model` - 返回给模型的内容
- `content_for_user` - 可直接展示给用户的可读内容（可选）
- `signals` - 结构化信号（停止执行、需要审批、附件、IM 卡片等）
