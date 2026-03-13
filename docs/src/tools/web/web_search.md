# Web Search Tool 设计与实现

本文档记录 `klaw-tool` 中 `web_search` 工具的设计目标、配置模型、Provider 抽象方式、调用参数约束、测试策略与后续扩展建议。

## 目标

- 提供统一的 `web_search` 工具接口，供模型在需要外部信息时调用。
- 支持多搜索服务 Provider（当前为 `tavily` 与 `brave`）。
- 通过配置切换 Provider，并支持 Provider 专有参数。
- 对大模型友好：`description` 与 `parameters` 要足够清晰，降低误调用和错误参数率。

## 代码位置

- 工具实现：`klaw-tool/src/web_search.rs`
- 配置结构：`klaw-config/src/lib.rs`
- 运行时注册：`klaw-cli/src/commands/runtime.rs`

## 整体架构

`WebSearchTool` 对外是单一工具名 `web_search`，内部通过 trait 分发到具体 Provider：

- `WebSearchProvider` trait：统一 `name()` 与 `search()` 接口
- `TavilyProvider`：对接 Tavily API
- `BraveProvider`：对接 Brave Search API

`WebSearchTool::new(config)` 根据 `tools.web_search.provider` 构造对应 Provider 实例。

## 配置模型

位于 `tools.web_search`，当前结构如下：

```toml
[tools.web_search]
enabled = true
provider = "tavily" # 可选: tavily / brave

[tools.web_search.tavily]
base_url = "https://api.tavily.com"
env_key = "TAVILY_API_KEY"
search_depth = "basic"
topic = "general"
include_answer = true
include_raw_content = false
include_images = false
project_id = "my-project-id"

[tools.web_search.brave]
base_url = "https://api.search.brave.com"
env_key = "BRAVE_SEARCH_API_KEY"
country = "US"
search_lang = "en"
ui_lang = "en-US"
safesearch = "moderate"
freshness = "pw"
```

说明：

- 配置不复用：`tavily` 和 `brave` 各自独立读取自身配置。
- 启用时校验：
  - `provider` 必须为 `tavily` 或 `brave`
  - 对应 Provider 必须配置 `api_key` 或 `env_key`（`env_key` 对应变量值在运行时解析）

## Tool Metadata（面向 LLM）

`web_search` 的 metadata 设计重点：

- `description` 明确 “什么时候应该调用”：当问题依赖外部或时效信息时。
- `parameters` 明确字段语义和边界：
  - `query`：必填，要求具体
  - `max_results`：默认 5，范围 1..20，超出会被钳制
  - `additionalProperties = false`，避免无效字段污染
  - 提供 examples，帮助模型形成正确调用模式

这类元数据是工具可用性的第一入口，直接影响工具触发率和参数质量。

## Provider 实现差异

### Tavily

- Endpoint: `POST /search`
- Header:
  - `Authorization: Bearer <api_key>`
  - `Content-Type: application/json`
  - 可选 `X-Project-ID`
- Body 支持专有参数：
  - `search_depth`、`topic`、`include_answer`、`include_raw_content`、`include_images`

参考：<https://docs.tavily.com/documentation/api-reference/introduction>

### Brave

- Endpoint: `GET /res/v1/web/search`
- Header:
  - `X-Subscription-Token: <api_key>`
  - `Accept: application/json`
  - `Accept-Encoding: gzip`
- Query 支持专有参数：
  - `country`、`search_lang`、`ui_lang`、`safesearch`、`freshness`

参考：<https://api-dashboard.search.brave.com/documentation/quickstart>

## 统一输出模型

内部统一为 `SearchResultItem`：

- `title`
- `url`
- `snippet`

然后格式化为工具输出文本，确保不同 Provider 的结果对模型是稳定结构。

## 错误处理策略

- 参数校验错误：`ToolError::InvalidArgs`
- 请求与响应错误：`ToolError::ExecutionFailed`
- 包括：
  - 缺少 `query`
  - 非法 `max_results`
  - API 请求失败或返回非 2xx
  - JSON 反序列化失败

## 测试策略

当前测试重点覆盖：

- 参数校验（必填与范围）
- Provider 路由（`tavily`/`brave`）
- 输出格式

建议后续补充：

- Mock HTTP 的请求/响应回归测试
- Provider 专有参数映射测试
- 认证 header 与 query/body 组装断言

## 扩展新 Provider 的步骤

1. 在 `klaw-config` 添加独立配置结构与默认值。
2. 在 `validate` 增加 provider 分支校验。
3. 在 `web_search.rs` 新增 `XxxProvider` 并实现 `WebSearchProvider` trait。
4. 在 `WebSearchTool::new` 增加 provider 构造分支。
5. 增加参数映射与错误处理测试。
6. 更新本文档与 `SUMMARY.md`。
