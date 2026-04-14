# Changelog

## 2026-04-14

### Changed
- `run_agent_execution` no longer emits the placeholder `Current turn stopped. No further tool calls were made.` text for `ask_question` stops that also carry an IM card; the turn still stops for the asynchronous user choice, but callers now receive an empty visible reply plus the card metadata/signals so the resumed follow-up can continue cleanly

## 2026-04-11

### Changed
- When approaching the `max_tool_iterations` limit (≥3 iterations allowed), the agent now injects a final-iteration system prompt asking the model to summarize progress and respond directly instead of calling more tools

## 2026-03-26

### Added
- `run_agent_execution` now returns per-tool `tool_audits` with arguments, full tool results, signals, timing, and tool-call sequencing for downstream persistence

### Fixed
- `build_provider_from_config` 现在始终使用当前 provider 自己的 `default_model` 构建实例，不再读取根级 `config.model` 作为跨 provider 的全局覆盖，避免切换 provider 后默认模型仍停留在旧值

## 2026-03-24

### Changed
- `run_agent_execution` now forwards per-request `agent.tool_choice` metadata into provider `ChatOptions`, allowing callers to require tool use for specific turns such as bootstrap/session initialization
- `run_agent_execution` now short-circuits on tool `stop` signals, skips further model/tool iterations, and returns a fixed stopped-turn message while preserving tool signals and request metrics

## 2026-03-22

### Changed
- `run_agent_execution` now preserves the provider-authored assistant message when a tool loop short-circuits on `approval_required`, keeping user-facing approval prompts stable while still returning tool signals and request metrics

## 2026-03-21

### Changed
- `run_agent_execution` now carries provider-generated request/response audit payloads alongside per-request usage records so callers can persist audited LLM traffic by tool-loop iteration

## 2026-03-20

### Changed
- `run_agent_execution` now optionally emits live snapshot events while a provider is streaming, and clears intermediate draft output when a tool loop continues into another model turn

### Fixed
- `run_agent_execution` now stops the current tool loop immediately after a tool emits `approval_required`, preventing a single user turn from creating multiple approval records before the channel renders the approval prompt

## 2026-03-19

### Changed
- `run_agent_execution` now collects per-request `LlmUsage` records across tool-loop iterations and returns them in `AgentExecutionOutput.request_usages`
- `run_agent_execution` now enforces `token_budget` against accumulated provider-reported `total_tokens`; `0` still means unlimited

## 2026-03-17

### Changed
- `run_agent_execution` now emits a `debug` log before each `provider.chat` call with the outbound request payload shape (`messages`, `tools`, selected model override, and `ChatOptions`) to make model request inspection easier during troubleshooting

## 2026-03-15

### Changed
- `AgentExecutionInput` 新增 `user_media` 字段，允许在当前用户轮次同时传入文本与媒体
- `run_agent_execution` 组装 `LlmMessage` 时会携带 `user_media`，并保持历史/system/tool 消息媒体为空

## 2026-03-14

### Changed
- 在触发 `ToolLoopExhausted` 时增加 `warn` 日志，区分 `max_tool_calls` 与 `max_tool_iterations` 两种上限命中场景，并输出当前计数与阈值。
- `run_agent_execution` 支持 `max_tool_iterations=0` 与 `max_tool_calls=0` 表示不设限。

## 2026-03-13

### Changed
- `build_provider_from_config` 传递并启用 `wire_api` 到 OpenAI-compatible provider，`responses` 配置现在会实际生效。
- agent 默认调用参数补齐新的 `ChatOptions` 可选字段，保持兼容同时可扩展 Responses API 能力。
- `build_provider_from_config` 现在会优先使用根级 `model` 配置作为默认模型，允许覆盖 provider 的 `default_model`。
