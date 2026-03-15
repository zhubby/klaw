# Changelog

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
