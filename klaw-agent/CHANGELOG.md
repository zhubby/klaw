# Changelog

## 2026-03-14

### Changed
- 在触发 `ToolLoopExhausted` 时增加 `warn` 日志，区分 `max_tool_calls` 与 `max_tool_iterations` 两种上限命中场景，并输出当前计数与阈值。
- `run_agent_execution` 支持 `max_tool_iterations=0` 与 `max_tool_calls=0` 表示不设限。

## 2026-03-13

### Changed
- `build_provider_from_config` 传递并启用 `wire_api` 到 OpenAI-compatible provider，`responses` 配置现在会实际生效。
- agent 默认调用参数补齐新的 `ChatOptions` 可选字段，保持兼容同时可扩展 Responses API 能力。
- `build_provider_from_config` 现在会优先使用根级 `model` 配置作为默认模型，允许覆盖 provider 的 `default_model`。
