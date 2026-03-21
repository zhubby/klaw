# Changelog

## 2026-03-21

### Added
- added dedicated heartbeat persistence models (`HeartbeatJob`, `NewHeartbeatJob`, `UpdateHeartbeatJobPatch`, `HeartbeatTaskRun`, `NewHeartbeatTaskRun`, `HeartbeatTaskStatus`) and `HeartbeatStorage` APIs
- added `heartbeat` / `heartbeat_task` tables plus indexes in both SQLx and Turso backends for session-bound heartbeat scheduling and run history

### Fixed
- changed the Turso session store to serialize all access through one shared connection so concurrent runtime writers no longer fail with `concurrent use forbidden`

### Added
- added `llm_audit` persistence models (`LlmAuditRecord`, `NewLlmAuditRecord`, `LlmAuditQuery`, `LlmAuditStatus`, `LlmAuditSortOrder`) and `SessionStorage` APIs for append/list workflows
- added `llm_audit` table + indexes in both SQLx and Turso backends for request/response payload auditing with session/provider/date filtering

### Changed
- storage tests now cover `llm_audit` filtering and requested-time sorting behavior

## 2026-03-20

### Changed
- moved shared data-root file and directory names into `klaw-util`, and updated `StoragePaths` to build its default layout from the centralized helpers

## 2026-03-19

### Added
- added `llm_usage` persistence models (`LlmUsageRecord`, `NewLlmUsageRecord`, `LlmUsageSummary`, `LlmUsageSource`) and `SessionStorage` APIs for append/list/session-sum/turn-sum workflows
- added `llm_usage` table + indexes in both SQLx and Turso backends for request-level token accounting linked to `session_key`

## 2026-03-18

### Added
- added `tmp/` to `StoragePaths` as the dedicated temporary data directory under the Klaw data root

### Changed
- `StoragePaths::ensure_dirs()` now creates the temporary data directory together with the other storage directories

## 2026-03-15

### Added
- added `approvals` persistence model (`ApprovalRecord`, `ApprovalStatus`, `NewApprovalRecord`) and `SessionStorage` APIs for create/get/update/consume lifecycle
- added `approvals` table + indexes in both SQLx and Turso backends, with `session_key` foreign key linkage to `sessions`
- added `consume_latest_approved_shell_command` storage API and backend support to consume approved shell requests by session + command hash
- approvals now persist `command_text` for exact command replay after approval

## 2026-03-14

### Added
- added session routing/model state persistence fields in `sessions` (`active_session_key`, `model_provider`, `model`)
- added `SessionStorage` APIs for session route/model lifecycle: `get_or_create_session_state`, `set_active_session`, `set_model_provider`, `set_model`

### Changed
- changed SQLx/Turso session store initialization to run idempotent schema upgrades for new session state columns

## 2026-03-13

### Added
- added `archive.db` and `archives/` data directory support to `StoragePaths`
- added `DefaultArchiveDb` and `open_default_archive_db()` for archive persistence

### Changed
- added session JSONL history reads so runtimes can rebuild prior conversation turns before the next LLM call
