# Changelog

## 2026-03-25

### Fixed
- startup-oriented remote manifest lookups can now read `latest.json` plus the current manifest directly, avoiding full history listing when callers only need remote-update detection
- remote retention cleanup no longer reloads the same manifest set twice while building its prune plan

## 2026-03-23

### Changed
- replaced bundle-based S3 snapshots with a versioned manifest plus content-addressed blob store; `latest.json` now points at the current manifest while remote history lives under `manifests/<id>.json`
- `BackupService` now reconciles local files against the remote manifest baseline before publishing a new manifest, uploads only missing blobs, and restores historical manifests directly from blob objects
- remote retention cleanup now prunes unreferenced blobs in addition to expired manifests, and legacy `bundle.tar.zst` layouts are rejected explicitly

### Changed
- custom S3 endpoints such as R2 now require explicit credentials or credential env names up front, avoiding fallback to missing AWS shared-profile files during sync startup and backup
- `BackupService` now exposes progress callbacks for snapshot preparation, upload, and retention cleanup so GUI clients can render live backup progress
- session route initialization no longer persists global default provider/model into every session row; `model_provider` / `model` now represent explicit session overrides only

### Added
- added `clear_model_routing_override` storage APIs in both SQLx and Turso backends so runtimes can normalize legacy session route state back to the global default

## 2026-03-22

### Added
- added `BackupService`, snapshot manifest models, and S3-compatible snapshot store support for managed data-root backup and restore
- added database snapshot export, tar+zstd bundle generation, remote snapshot listing, and restore verification tests

## 2026-03-22

### Changed
- remote snapshot retention cleanup now refreshes `latest.json` after pruning so the latest pointer stays consistent when old snapshots are removed outside a fresh backup upload
- S3 snapshot configuration now supports direct credential values in addition to env references, and empty device IDs now normalize from the local hostname

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
