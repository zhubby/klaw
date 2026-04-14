# CHANGELOG

## 2026-04-14

### Added

- added session-manager history paging wrappers so callers can request the latest N chat records or fetch older history before a given `message_id`

## 2026-03-30

### Added
- added session-manager support for distinct session-channel option loading so GUI callers can populate SQL-backed channel filters

### Changed
- `SessionListQuery` now carries optional `channel` filtering plus `updated_at` sort order so callers can request database-side filtering and ordering instead of post-processing in memory

## 2026-03-26

### Added
- added session-manager wrappers for `tool_audit` append/list/filter operations so runtime and GUI callers can inspect persisted tool execution diagnostics
- added `set_delivery_metadata` session-manager wrappers so runtimes can persist refreshable channel reply metadata on active sessions

## 2026-03-25

### Added

- added `append_webhook_agent`, `update_webhook_agent_status`, and `list_webhook_agents` session-manager wrappers plus `WebhookAgent*` re-exports for GUI/runtime callers

## 2026-03-23

### Changed

- session-manager routing APIs now treat provider/model as explicit session overrides instead of copied defaults, and expose override clearing for runtime route normalization

## 2026-03-21

### Added

- added session-manager wrappers for `llm_audit` append/list operations so GUI/runtime callers can query audited provider requests through `klaw-session`

## 2026-03-19

### Added

- added session-manager wrappers for request-level LLM usage append/list/session-sum/turn-sum operations

## 2026-03-16

### Added

- initial session manager crate with `SessionManager` trait, `SessionListQuery`, and `SqliteSessionManager`

### Changed

- expanded `SessionManager` to cover session state routing, chat history persistence, and provider/model updates so runtime and CLI can stop calling storage directly
