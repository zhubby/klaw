# Web Fetch Tool 设计与实现

本文档记录 `klaw-tool` 中 `web_fetch` 工具的目标、配置、参数约束、安全策略与测试覆盖。

## 目标

- 提供统一的 `web_fetch` 工具，抓取单个明确 URL 内容。
- 支持 `markdown`/`text` 提取模式，返回可读正文。
- 具备基础 SSRF 防护、重定向控制和内存缓存。

## 代码位置

- 工具实现：`klaw-tool/src/web_fetch.rs`
- 配置结构：`klaw-config/src/lib.rs`
- 运行时注册：`klaw-cli/src/commands/runtime.rs`

## 配置模型

位于 `tools.web_fetch`：

```toml
[tools.web_fetch]
enabled = false
max_chars = 50000
timeout_seconds = 15
cache_ttl_minutes = 10
max_redirects = 3
readability = true
ssrf_allowlist = []
```

字段说明：

- `enabled`：是否启用工具。
- `max_chars`：默认返回最大字符数。
- `timeout_seconds`：单次请求超时。
- `cache_ttl_minutes`：内存缓存 TTL（分钟）。
- `max_redirects`：最大重定向跳数。
- `readability`：HTML 提取时是否优先可读文本模式。
- `ssrf_allowlist`：允许访问的私网网段（CIDR，例：`172.22.0.0/16`）。

## Tool Metadata（面向 LLM）

`web_fetch` 参数设计：

- `url`：必填，必须是 `http`/`https`。
- `extract_mode`：可选，`markdown` 或 `text`，默认 `markdown`。
- `max_chars`：可选，覆盖默认返回长度。

适用场景：

- 已知目标页面 URL，需要抓取正文内容。
- 与 `web_search` 配合：先搜，再针对候选链接做精读。

## 安全与可靠性策略

- Scheme 限制：仅允许 `http` 和 `https`。
- SSRF 防护：
  - 直接 IP 或 DNS 解析结果命中私网/回环/链路本地地址时默认阻断。
  - 命中 `ssrf_allowlist` 才放行。
- 重定向控制：手动跟随并限制 `max_redirects`，检测循环跳转。
- 缓存：以 `url + extract_mode + max_chars` 为 key，按 TTL 过期。

## 输出格式

工具输出为 JSON 文本（pretty-print），核心字段：

- `url`：最终访问 URL（重定向后）。
- `content_type`：响应内容类型。
- `extract_mode`：实际提取模式（`markdown`/`text`/`json`）。
- `content`：提取后的文本内容。
- `truncated`：是否被截断。
- `original_length`：原始响应体长度。

## 测试覆盖

当前测试覆盖：

- 参数校验（`url`、`extract_mode`、`max_chars`）。
- HTML 文本提取和 UTF-8 截断边界。
- 私网地址识别与 allowlist 匹配。
- SSRF 阻断和不支持 scheme 的错误路径。

建议后续补充：

- 基于 mock server 的重定向链路与循环跳转测试。
- 缓存过期行为（TTL 命中/失效）测试。
- 不同 `content-type`（HTML/JSON/纯文本）的端到端快照测试。
