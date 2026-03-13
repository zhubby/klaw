# Changelog

## 2026-03-13

### Changed
- `build_provider_from_config` 传递并启用 `wire_api` 到 OpenAI-compatible provider，`responses` 配置现在会实际生效。
- agent 默认调用参数补齐新的 `ChatOptions` 可选字段，保持兼容同时可扩展 Responses API 能力。
- `build_provider_from_config` 现在会优先使用根级 `model` 配置作为默认模型，允许覆盖 provider 的 `default_model`。
