# Changelog

## 2026-03-21

### Added
- provider responses and failures now carry structured audit payloads with serialized request/response bodies and provider ids for downstream persistence

### Changed
- OpenAI-compatible and Anthropic providers now capture request/response audit metadata at the provider boundary without serializing auth headers

## 2026-03-20

### Changed
- added provider-side streaming support to `LlmProvider` via `chat_stream`, plus `LlmStreamEvent` deltas for content and reasoning
- OpenAI-compatible provider now honors `OpenAiCompatibleConfig.stream` and uses native SSE streaming for both `chat_completions` and `responses`
- moved the default local tokenizer directory resolution (`~/.klaw/tokenizers`) into `klaw-util`

## 2026-03-19

### Added
- added `LlmUsage` and optional `LlmResponse.usage` so providers can return normalized token usage metadata
- added local token estimation fallback using the `tokenizers` crate; when provider APIs omit `usage`, Klaw now estimates token usage locally and marks it as `estimated_local`

### Changed
- OpenAI-compatible response parsing now extracts prompt/input, completion/output, cached, and reasoning token counts from both `chat_completions` and `responses`
- Anthropic message parsing now captures input/output token usage and response id when the API returns them
- `LlmResponse` now also records `usage_source`, distinguishing `provider_reported` from `estimated_local`

## 2026-03-15

### Changed
- `LlmMessage` 新增 `media` 字段，支持在单条用户消息中携带媒体 URL
- `OpenAiCompatibleProvider` 现在会将媒体映射到多模态请求块：
  - `chat_completions`: `content` 支持 `text` + `image_url` 组合
  - `responses`: `content` 支持 `input_text` + `input_image` 组合
- 新增媒体请求块构建单元测试，覆盖 `responses` 输入序列化

## 2026-03-13

### Added
- 增加 `OpenAiWireApi`，支持 `chat_completions` 与 `responses` 双协议切换。
- `ChatOptions` 新增 Responses API 参数承载字段（如 `previous_response_id`、`parallel_tool_calls`、`tool_choice`、`text`、`reasoning` 等）。
- 新增 Responses API 请求/响应映射与解析单元测试。

### Changed
- `OpenAiCompatibleProvider` 从固定 `/chat/completions` 改为按 `wire_api` 选择端点。
- OpenAI-compatible 实现新增 Responses API 输入构建（message/function_call/function_call_output）与输出解析（message/reasoning/function_call）。
