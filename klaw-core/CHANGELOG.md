# Changelog

## 2026-04-14

### Changed
- `AgentLoop` now preserves `ask_question` IM-card stop metadata while allowing the underlying agent execution to surface an empty visible reply instead of the generic stopped-turn placeholder, so websocket/webui channels can show the card without the misleading extra assistant text

## 2026-04-11

### Changed
- `ToolLoopExhausted` errors now return an informative response to the user instead of `final_response: None`, explaining that the tool call limit was reached and suggesting alternatives

## 2026-04-07

### Added
- expanded `AgentLoop` coverage for generic IM question cards so tools that emit both `im_card` and `stop` signals now keep the card metadata and `turn.stop_signal` metadata together in the outbound response

## 2026-04-06

### Changed
- refactored the agent execution boundary so `AgentLoop` now builds a typed `AgentExecutionContext` and delegates inner turn execution details to `klaw-agent`, instead of pushing system prompt, tool choice, provider/model routing, and attachment context through only a loose metadata map
- simplified `AgentRunState` into an honest outer lifecycle (`Received` -> `Validating` -> `Executing` -> `Publishing`) so runtime state no longer pretends to mirror each inner tool-loop step
- `AgentLoop` now records `turn.disposition` metadata for approval and stopped short-circuits alongside the existing `approval.*` and `turn.stop_*` fields
- `AgentLoop` now honors optional per-turn execution limit overrides from inbound metadata (`agent.max_tool_iterations`, `agent.max_tool_calls`, `agent.token_budget`), so runtimes can selectively constrain retry-heavy follow-up turns without changing global defaults

## 2026-04-03

### Removed
- removed the unused `ErrorClass` enum from `reliability.rs` because runtime error handling already flows through `ErrorCode` and normalized retry buckets

### Changed
- strengthened the runtime system prompt with an explicit truthfulness rule that forbids claiming files were read, commands were run, searches were performed, or other tool-backed actions were completed unless they were actually verified by a tool result in the current turn or directly provided by the user
- translated remaining `klaw-core` Rust doc comments to English and added clarifying comments around runtime error normalization

## 2026-04-02

### Changed
- tightened heartbeat guidance in the workspace `AGENTS.md` prompt template so models treat `HEARTBEAT_OK` as the default silent ack token, honor job-specific override tokens, and rely on session context plus heartbeat metadata instead of a removed sidecar file
- clarified in the workspace `AGENTS.md` prompt template that session-bound heartbeat jobs should be managed with `heartbeat_manager`, while explicit wall-clock jobs belong to `cron_manager`

### Removed
- removed the `HEARTBEAT.md` workspace prompt template so runtime prompt setup no longer auto-creates or references a heartbeat-only markdown sidecar

## 2026-03-30

### Fixed
- `AgentLoop` now normalizes persisted/outbound LLM audit `provider` values to the resolved provider id instead of the provider implementation name, so downstream runtime storage and GUI panels can map audit rows back to configured provider display names

## 2026-03-28

### Changed
- runtime prompt guidance now explicitly tells the model to use `channel_attachment` when an archived file or image should be sent back into chat, instead of only describing the file in plain text
- current-message attachment guidance now points models at `channel_attachment` for returning archived attachments to the user

## 2026-03-26

### Added
- `ProcessOutcome` now carries structured `tool_audits` alongside `llm_audits`, allowing runtimes to persist full tool execution diagnostics per turn

### Changed
- `AgentLoop` 现在将默认 provider、provider registry 与 provider default model 收敛到一份可热替换的 `ProviderRuntimeSnapshot`，避免 runtime/GUI/命令层继续各自缓存一份 provider 真相源

## 2026-03-24

### Changed
- moved runtime system prompt assembly helpers into `prompt.rs`, including the shared workspace-docs prompt block and a new `build_runtime_system_prompt` entrypoint
- changed workspace prompt template backfill so `BOOTSTRAP.md` is created only during the first workspace initialization and is not recreated after later deletion
- changed runtime system prompt assembly to inline workspace `AGENTS.md` / `SOUL.md` / `IDENTITY.md` / `TOOLS.md` content ahead of the existing runtime sections, added a leading workspace path/role descriptor, and limited on-demand doc guidance to `USER.md`, `HEARTBEAT.md`, and `BOOTSTRAP.md`
- aligned prompt templates with the new runtime prompt model by treating `USER.md` as on-demand context, updating `SOUL.md` continuity wording, and removing raw credentials from `TOOLS.md`
- updated current-message attachment guidance so turns with archived files explicitly steer the model toward `archive.get` for known ids and `list_session_attachments` for earlier files from the same session
- updated current-message attachment guidance so archived audio / voice attachments also steer the model toward `voice.stt`
- `AgentLoop` now forwards tool `stop` signals as successful stopped-turn metadata (`turn.stopped`, `turn.stop_signal`) instead of treating the turn as a loop failure

### Removed
- removed the unused deprecated `load_or_create_system_prompt*` compatibility shims from `klaw-core`

## 2026-03-22

### Added
- `AgentTelemetry` now exposes model-request, model-attributed tool-outcome, and turn-outcome recording APIs

### Changed
- `AgentLoop` now emits provider/model-level observability records for successful and failed model requests, model-attributed tool outcomes, and per-turn completion/degraded/budget/tool-loop outcomes

## 2026-03-21

### Added
- `AgentLoop` now propagates provider request/response audit payloads into runtime outcomes and outbound metadata under `llm.audit.records` for downstream persistence/UI inspection
- `AgentTelemetry` 新增 `record_tool_outcome` 接口，并引入 `ToolOutcomeStatus`，用于把工具成功/失败、耗时和错误码写入本地分析存储

### Changed
- `AgentLoop` 在工具成功和失败路径都会上报结构化工具结果，供 GUI 分析面板统计成功率、失败分布和趋势
- `AgentLoop` now preserves `approval_required` tool messages without wrapping them in `tool ... failed: execution failed`, so approval prompts are surfaced as approval states instead of user-facing failures

## 2026-03-20

### Changed
- `AgentLoop` now exposes a streaming processing path that forwards agent snapshot events while preserving the existing final outbound envelope shape
- moved default `~/.klaw/workspace` path derivation into the new `klaw-util` crate and re-exported `WORKSPACE_DIR_NAME` from there instead of owning the constant in `klaw-core`

## 2026-03-19

### Added
- `AgentLoop` now propagates request-level LLM token usage into outbound metadata under `llm.usage.records`, including provider/model/wire_api and token counters for downstream persistence
- added `ErrorCode::BudgetExceeded` and mapped agent token-budget breaches to an explicit runtime failure path
- `AgentLoop` now annotates archived inbound attachments into the current user message and tool metadata, exposing `archive_id` / `storage_rel_path` plus read-only/copy-to-workspace guidance to the model

### Fixed
- fixed `InMemoryTransport::publish` so published messages are also consumable from the in-memory queue, restoring cron/manual runtime flows that publish inbound work before draining the agent loop

## 2026-03-17

### Added
- added workspace prompt template bootstrap APIs in `prompt.rs` that initialize `~/.klaw/workspace` with built-in `AGENTS.md`/`BOOTSTRAP.md`/`HEARTBEAT.md`/`IDENTITY.md`/`SOUL.md`/`TOOLS.md`/`USER.md` files (create-only, no overwrite)
- added runtime prompt composition helpers for OpenClaw-style skills lazy loading:
  - `format_skills_for_prompt` (shortlist only)
  - `skills_lazy_load_instructions`
  - `compose_runtime_prompt`

### Changed
- changed `load_or_create_system_prompt*` behavior to a compatibility shim that only ensures workspace prompt templates and no longer reads/writes `SYSTEM.md`

### Removed
- removed `SYSTEM.md` default-prompt constants from `klaw-core` public exports

## 2026-03-16

### Changed
- `AgentLoop` system prompt is now hot-reloadable at runtime via interior locking and a `set_system_prompt` API

## 2026-03-14

### Changed
- added debug-level tool result logging in `AgentLoop` executor after each tool call (success/failure), with output truncation to prevent oversized log lines
- changed `AgentLoop` to support runtime provider registry routing, selecting provider/model from inbound metadata (`agent.provider_id` / `agent.model`) per message

## 2026-03-15

### Changed
- `InboundMessage` 新增 `media_references` 字段（`serde(default)`），用于跨 channel/runtime/agent 透传媒体引用
- `AgentLoop` 在构建当前用户消息时会从 `media_references` 提取可用媒体 URL，传给 LLM 执行层

## 2026-03-13

### Added
- added shared `MediaReference` and `MediaSourceKind` models for media-aware channel and archive integrations

### Fixed
- fixed agent loop dropping persisted conversation history by restoring prior session turns from inbound metadata before building the LLM request
