# Changelog

## 2026-03-21

### Added
- `AgentTelemetry` 新增 `record_tool_outcome` 接口，并引入 `ToolOutcomeStatus`，用于把工具成功/失败、耗时和错误码写入本地分析存储

### Changed
- `AgentLoop` 在工具成功和失败路径都会上报结构化工具结果，供 GUI 分析面板统计成功率、失败分布和趋势

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
