# Changelog

## 2026-03-13

### Added
- 增加 `OpenAiWireApi`，支持 `chat_completions` 与 `responses` 双协议切换。
- `ChatOptions` 新增 Responses API 参数承载字段（如 `previous_response_id`、`parallel_tool_calls`、`tool_choice`、`text`、`reasoning` 等）。
- 新增 Responses API 请求/响应映射与解析单元测试。

### Changed
- `OpenAiCompatibleProvider` 从固定 `/chat/completions` 改为按 `wire_api` 选择端点。
- OpenAI-compatible 实现新增 Responses API 输入构建（message/function_call/function_call_output）与输出解析（message/reasoning/function_call）。
