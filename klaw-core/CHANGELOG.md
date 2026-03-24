# Changelog

## 2026-03-24

### Changed
- moved runtime system prompt assembly helpers into `prompt.rs`, including the shared workspace-docs prompt block and a new `build_runtime_system_prompt` entrypoint
- changed workspace prompt template backfill so `BOOTSTRAP.md` is created only during the first workspace initialization and is not recreated after later deletion

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
