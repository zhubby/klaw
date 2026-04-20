# File Read Tool 设计与实现

本文档记录 `klaw-tool` 中 `file_read` 工具的设计动机、架构决策、参数模型、核心逻辑、图片支持、可插拔后端与测试覆盖。

## 设计动机

LLM Agent 直接使用 `cat` 读取文件缺乏任何保护机制——一个 100MB 的二进制文件会原样输出到 context，耗尽全部 token 窗口。`file_read` 工具的核心设计思想是**保护性读取**：让 LLM 安全地读取任意大小的文件。

### 三层保护机制

| 层级 | 机制 | 作用 |
|------|------|------|
| 1. 偏移与分页 | `offset` + `limit` 参数 | LLM 不需要一次加载整个大文件，可以分块读取 |
| 2. 双重截断策略 | 行数限制 + 字节数限制 | 超过任一限制自动截断，保护 context |
| 3. 截断续读提示 | 截断后附带提示文本 | 告知 LLM 还有更多内容，提示使用 `offset` 参数继续读取 |

## 代码位置

- 工具实现：`klaw-tool/src/file_read.rs`
- 配置模型：`klaw-config/src/lib.rs`（`tools.file_read`，类型 `FileReadConfig`）
- 运行时注册：`klaw-runtime/src/lib.rs`（`register_configured_tools` 函数）
- GUI 工具面板：`klaw-gui/src/panels/tool.rs`（`ToggleToolKind::FileRead`）

## 参数模型（强约束）

`file_read` 使用强类型请求结构并开启 `deny_unknown_fields`：

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileReadRequest {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}
```

字段说明：

- **`path`**（必填）：文件路径，支持绝对路径与相对路径。相对路径基于 `metadata["workspace"]` 解析。
- **`offset`**（可选，1-indexed）：起始行号。行号与 LLM 看到的行号前缀一致，方便续读。默认为 1。
- **`limit`**（可选）：最大读取行数。默认受 `max_lines` 配置约束（2000 行）。

工具 parameters JSON Schema 设置了 `additionalProperties: false`，拒绝模型传入无效字段。

## 配置模型（`tools.file_read`）

```toml
[tools.file_read]
enabled = true              # 默认开启
max_lines = 2000            # 行数截断上限
max_bytes = 51200            # 字节截断上限（50KB）
auto_resize_images = true   # 自动缩放图片
```

`FileReadConfig` 实现 `ToolEnabled` trait，默认 `enabled = true`，无需额外配置即可使用。

### 为什么需要两个截断限制？

| 限制 | 应对场景 |
|------|---------|
| **2000 行限制** | 防止 LLM context 被大量短行填满（如巨大的 JSON 文件，每行很短但总行数惊人） |
| **50KB 字节限制** | 防止少量超长行消耗大量 token（如 minified JavaScript，一行可能几百 KB） |

两个限制独立生效，**先触发的那个截断**。

## 文本读取核心逻辑

### 流程

1. 从 `metadata["workspace"]` 或绝对路径解析文件路径
2. 检查文件可访问性（`access` 操作）
3. 通过 `ReadOperations` trait 检测 MIME 类型，判断是否图片
4. 若为图片 → 走图片处理流程
5. 若为文本：
   a. 读取文件字节
   b. 二进制检测（前 8KB 中 NUL 字节占比超过 10% 视为二进制）
   c. UTF-8 解码
   d. 按行分割并加上行号前缀（`N: content`）
   e. 双重截断：先检查首行是否超过字节限制，再逐行累加，行数或字节数先触发限制则截断
   f. 截断时附带续读提示（告知使用 `offset: N+1` 继续读取）

### 首行超限处理

如果文件的**第一行就超过 50KB 限制**（如 minified CSS），`file_read` 不会返回半截内容，而是返回提示信息，引导 LLM 使用 shell 工具的 `sed` + `head -c` 来读取。

> 半截的 minified CSS 对 LLM 没有任何帮助，直接提示更好的方案比返回无用内容更有价值。

### 截断元信息

```rust
struct TruncationResult {
    content: String,
    truncated: bool,
    truncated_by: Option<TruncatedBy>,  // "lines" 或 "bytes"
    total_lines: usize,
    total_bytes: usize,
    output_lines: usize,
    output_bytes: usize,
    first_line_exceeds_limit: bool,
}
```

### 输出内容示例

正常读取：

```
1: fn main() {
2:     println!("hello");
3: }
```

截断续读提示：

```
1: fn main() {
2:     println!("hello");
...
2000:     // more code

--- Content truncated (line limit). Showing 2000 of 5000 total lines. Use `offset: 2001` to continue reading. ---
```

## 图片读取

`file_read` 原生支持图片文件读取，这对多模态 LLM 至关重要。

### 支持的图片格式

| 扩展名 | MIME 类型 |
|--------|----------|
| `.png` | `image/png` |
| `.jpg` / `.jpeg` | `image/jpeg` |
| `.gif` | `image/gif` |
| `.bmp` | `image/bmp` |
| `.ico` | `image/x-icon` |
| `.tiff` / `.tif` | `image/tiff` |
| `.webp` | `image/webp` |
| `.avif` | `image/avif` |

### 图片处理流程

1. 通过 `ReadOperations.detect_image_mime_type()` 检测 MIME 类型
2. 读取文件为字节缓冲区
3. 若 `auto_resize_images` 开启（默认），调用 `image` crate 将图片缩放到 2000×2000 像素以内
4. 将（缩放后的）图片 base64 编码为 `data:{mime_type};base64,{data}` data URI
5. 通过 `ToolOutput.media` 字段传递 `LlmMedia` 实例，由 agent loop 注入 LLM 消息

### 设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 默认 `auto_resize_images = true` | 用户 4K 截图（几 MB PNG）自动缩小，避免图片 token 消耗失控 |
| resize 失败优雅降级 | resize 失败不报错，返回 "Image omitted" 文本提示。确保工具永远不会因为图片处理失败让整个 tool call 失败 |

### 输出结构

图片文件成功读取时返回：

```
ToolOutput {
    content_for_model: "Read image file [image/png]\nPath: /path/to/image.png\nImage data included.",
    content_for_user: Some("Image: /path/to/image.png (image/png)"),
    media: vec![LlmMedia {
        mime_type: Some("image/png"),
        url: "data:image/png;base64,iVBOR...",
    }],
    signals: vec![],
}
```

resize 失败时降级返回纯文本：

```
ToolOutput {
    content_for_model: "Read image file [image/png]\n[Image omitted: could not be resized for display. Path: /path/to/image.png]",
    content_for_user: Some("Image file: /path/to/image.png (image/png, resize failed)"),
    media: vec![],  // 无图片数据
    signals: vec![],
}
```

## 媒体内容传递架构

### ToolOutput 扩展

为支持图片返回，`ToolOutput` 结构新增 `media` 字段：

```rust
pub struct ToolOutput {
    pub content_for_model: String,
    pub content_for_user: Option<String>,
    pub media: Vec<klaw_llm::LlmMedia>,  // 新增：工具返回的媒体内容
    pub signals: Vec<ToolSignal>,
}
```

所有现有工具的 `ToolOutput` 构造均添加了 `media: Vec::new()`，不影响现有行为。

### ToolInvocationResult 扩展

`klaw-agent` 中 `ToolInvocationResult` 同步新增 `media` 字段：

```rust
pub struct ToolInvocationResult {
    pub ok: bool,
    pub content_for_model: String,
    pub error_code: Option<String>,
    pub error_details: Option<Value>,
    pub retryable: Option<bool>,
    pub signals: Vec<ToolInvocationSignal>,
    pub media: Vec<klaw_llm::LlmMedia>,  // 新增
}
```

新增 `success_with_media()` 构造方法，`success()` 和 `error()` 默认 `media: Vec::new()`。

### 传递链路

```
FileReadTool.execute()
  → ToolOutput { media: vec![LlmMedia] }
    → RegistryToolExecutor (klaw-core/agent_loop.rs)
      → ToolInvocationResult::success_with_media(content, signals, output.media)
        → apply_tool_calls (klaw-agent/src/lib.rs)
          → LlmMessage { role: "tool", content: ..., media: result.media }
```

LLM 消息中的 `media` 字段会被 OpenAI Chat Completions / Responses API 转换为 `image_url` 内容块。Anthropic 提供商当前不支持工具消息中的图片。

## 可插拔后端（ReadOperations）

```rust
#[async_trait]
pub trait ReadOperations: Send + Sync {
    fn read_file<'a>(&'a self, path: &'a Path)
        -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;
    fn access<'a>(&'a self, path: &'a Path)
        -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
    fn detect_image_mime_type<'a>(&'a self, path: &'a Path)
        -> Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>;
}
```

默认实现 `LocalFsReadOperations` 使用 `tokio::fs` 读取本地文件系统。

未来可扩展的后端：

| 后端 | 场景 |
|------|------|
| SSH 远程读取 | 通过 SSH 连接读取远程服务器上的文件 |
| Docker 容器读取 | 读取容器内文件系统 |
| Git blob 读取 | 直接从 git object store 读取文件内容 |

工具通过 `FileReadTool::with_ops(config, ops)` 注入自定义后端，不修改核心逻辑。

## Binary 文件处理

`file_read` 检测到二进制文件时（前 8KB 中 NUL 字节 > 10%），不尝试解码输出，而是返回引导文本：

```
File appears to be binary (12345 bytes).
Use the shell tool with `xxd`, `hexdump`, or `file` commands to inspect binary files.
Path: /path/to/binary
```

设计原则：专用工具做专用事。对 PDF、Jupyter Notebook 等复杂格式，`file_read` 不提供参数化支持，LLM 应退回到 shell 工具使用专门命令行工具。

## 注册与配置

### 默认启用

`file_read` 工具默认启用，`FileReadConfig` 默认值：

```rust
FileReadConfig {
    enabled: true,
    max_lines: 2000,
    max_bytes: 51200,  // 50KB
    auto_resize_images: true,
}
```

### 运行时注册

在 `klaw-runtime/src/lib.rs` 的 `register_configured_tools` 中：

```rust
if config.tools.file_read.enabled() {
    tools.register(FileReadTool::new(&config.tools.file_read));
}
```

### GUI 工具面板

在 `klaw-gui/src/panels/tool.rs` 中，`file_read` 被添加为 `ToggleToolKind::FileRead`，可以像其他工具一样通过界面开启/关闭。

## 依赖

| 依赖 | 用途 | 来源 |
|------|------|------|
| `base64` | 图片 base64 编码 | workspace 已有 |
| `image` | 图片解码/缩放 | workspace 已有（`features = ["tiff"]`） |
| `klaw-config` | `FileReadConfig` | workspace 内部 |

## 测试覆盖

`klaw-tool/src/file_read.rs` 的 `#[cfg(test)] mod tests` 覆盖：

- `test_truncate_basic` — 完整读取未截断
- `test_truncate_by_lines_limit` — 超过行数限制截断
- `test_truncate_with_offset` — 带 offset 从中间读取
- `test_truncate_offset_beyond_end` — offset 超出文件末尾返回空
- `test_first_line_exceeds_limit` — 首行超过字节限制返回提示
- `test_truncate_by_bytes` — 超过字节限制截断
- `test_detect_image_mime_type` — 文件扩展名到 MIME 类型映射
- `test_is_likely_binary` — 二进制文件检测
- `test_parse_request_valid` — 正常参数解析
- `test_parse_request_minimal` — 只传 path 的最小参数
- `test_parse_request_unknown_field_rejected` — 未知字段被拒绝
- `test_file_read_config_default` — 配置默认值验证

## 整体架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                         LLM Agent                                │
├─────────────────────────────────────────────────────────────────┤
│       Schema: { path: string, offset?: number, limit?: number } │
├─────────────────────────────────────────────────────────────────┤
│                           ↓                                      │
├─────────────────────────────────────────────────────────────────┤
│              ReadOperations（可插拔抽象）                         │
│  ┌─────────────────────┐      ┌─────────────────────────┐      │
│  │  LocalFS (默认)     │      │  SSH / Docker / Git ...  │      │
│  └─────────────────────┘      └─────────────────────────┘      │
├─────────────────────────────────────────────────────────────────┤
│                   ↓  自动检测文件类型                              │
├─────────────────────────────────────────────────────────────────┤
│  文本分支                      │  图片分支                       │
│  • 按行读取 + 行号前缀         │  • 读取 Buffer                │
│  • 双重截断（2000行/50KB）     │  • base64 编码为 data URI      │
│  • 先触发者生效               │  • 自动缩放到 2000×2000        │
│  • 记录截断元信息             │  • resize 失败优雅降级          │
│  • 二进制文件返回 shell 提示  │  • 通过 ToolOutput.media 传递   │
├─────────────────────────────────────────────────────────────────┤
│                   ↓  返回结果                                    │
├─────────────────────────────────────────────────────────────────┤
│  • 文本：带截断提示的内容（告知续读方式）                         │
│  • 图片：[文字描述] + [LlmMedia data URI]                       │
│  • 截断元信息：truncated_by、total_lines、output_lines 等        │
├─────────────────────────────────────────────────────────────────┤
│               ToolOutput.media → LlmMessage.media               │
│               → OpenAI ImageUrl / Responses InputImage           │
└─────────────────────────────────────────────────────────────────┘
```

## 取舍分析

### 得到

| 收益 | 说明 |
|------|------|
| **安全的文件探索** | LLM 不会因为读了大文件而耗尽 context。`offset/limit` 配合续读提示让增量探索工作流流畅 |
| **文本 + 图片一体化** | 同一个工具处理两种最常见格式，LLM 不需要学习不同工具。自动检测 MIME 类型，对多模态友好 |
| **可插拔适配** | `ReadOperations` trait 支持本地、远程、容器等多种环境 |
| **媒体传递链路** | `ToolOutput.media` → `ToolInvocationResult.media` → `LlmMessage.media` 的完整链路，为未来其他工具返回图片铺路 |

### 放弃

| 代价 | 说明 |
|------|------|
| **截断可能丢失上下文** | LLM 可能需要多次 read 调用才能获取完整信息。但相比一次性耗尽 context，这个开销值得 |
| **图片缩放可能丢失细节** | 自动缩放到 2000×2000 以内，高分辨率细节可能丢失。但对大多数场景已经足够 |
| **非文本格式不原生支持** | PDF、Jupyter Notebook 等不被 file_read 原生支持。设计原则是专用工具做专用事，LLM 应退回到 shell 处理 |