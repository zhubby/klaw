# klaw-llm

`klaw-llm` 提供统一的 LLM provider 抽象和实现，负责请求编排、协议转换与响应解析。

## 能力

- 统一 `LlmProvider` trait，向上层暴露单轮 `chat` 接口，并支持可选的 `chat_stream` 增量事件输出。
- `LlmMessage` 支持 `media` 字段（URL/data URL），用于多模态用户输入。
- OpenAI-compatible provider：
  - `chat_completions` wire API。
  - `responses` wire API（含 function call / function_call_output 映射）。
  - 多模态内容映射（`text + image_url` / `input_text + input_image`）。
  - 可选 native SSE streaming，向上层发出文本/推理增量事件。
- Anthropic provider（Messages API）。
- 当 provider 未返回 `usage` 时，支持本地 token 估算回退：
  - 优先读取配置的 `tokenizer.json`（`tokenizers` crate）
  - 若无可用 tokenizer 文件，则退回启发式估算，保证 token 统计始终可展示

## 架构

- `src/lib.rs`：跨 provider 的核心类型（消息、工具定义、响应、错误、调用选项）。
- `src/providers/openai_compatible.rs`：OpenAI-compatible 协议适配与解析。
- `src/providers/anthropic.rs`：Anthropic 协议适配与解析。

## 设计原则

- 上层只依赖统一领域对象，不感知下游 wire API 差异。
- 在 provider 边界完成字段映射与协议兼容。
- 工具调用统一归一到 `ToolCall`，便于 agent loop 编排。
