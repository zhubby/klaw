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

- [Shell](./built-in/shell.md) - 可控的 shell 执行，支持审批和风险分级
- [Apply Patch](./built-in/apply_patch.md) - 批量文件编辑（添加、更新、删除、移动）
- [本地搜索](./built-in/local_search.md) - 基于 ripgrep 的代码检索
- [记忆](./built-in/memory.md) - 长期记忆存储与检索
- [审批](./built-in/approval.md) - 高风险操作的审批记录管理（请求、查询、决议）

### Web 工具

外部信息获取：

- [Web Fetch](./web/web_fetch.md) - 单页面内容抓取，支持 SSRF 防护
- [Web Search](./web/web_search.md) - 多 Provider 搜索（Tavily/Brave）

### 高级工具

复杂编排能力：

- [Sub Agent](./advanced/sub_agent.md) - 子代理委托执行
- [Skills Registry](./advanced/skills.md) - Skills 生命周期管理

## 工具配置

在 `~/.klaw/config.toml` 中配置：

```toml
[tools.shell]
enabled = true
approval_policy = "on_request"
safe_commands = ["ls", "cat", "echo", "rg", "find"]
max_timeout_ms = 120000

[tools.web_search]
enabled = true
provider = "tavily"

[tools.memory]
enabled = true
search_limit = 8
use_vector = true
```

## 工具上下文

工具执行时可访问：

- `session_key` - 当前会话标识
- `channel` - 来源渠道
- `workspace` - 工作目录
- `metadata` - 扩展元数据

## 输出格式

工具输出统一为结构化 JSON，包含：

- `success` - 执行是否成功
- `result` - 工具结果（给 LLM）
- `for_user` - 可直接展示给用户的内容
- `error` - 错误信息（失败时）
