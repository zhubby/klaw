# klaw-llm

`klaw-llm` 提供统一的 LLM provider 抽象和实现，负责请求编排、协议转换与响应解析。

## 能力

- 统一 `LlmProvider` trait，向上层暴露单轮 `chat` 接口。
- OpenAI-compatible provider：
  - `chat_completions` wire API。
  - `responses` wire API（含 function call / function_call_output 映射）。
- Anthropic provider（Messages API）。

## 架构

- `src/lib.rs`：跨 provider 的核心类型（消息、工具定义、响应、错误、调用选项）。
- `src/providers/openai_compatible.rs`：OpenAI-compatible 协议适配与解析。
- `src/providers/anthropic.rs`：Anthropic 协议适配与解析。

## 设计原则

- 上层只依赖统一领域对象，不感知下游 wire API 差异。
- 在 provider 边界完成字段映射与协议兼容。
- 工具调用统一归一到 `ToolCall`，便于 agent loop 编排。
