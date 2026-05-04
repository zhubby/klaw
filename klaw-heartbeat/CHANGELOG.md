# Changelog

## 2026-05-04

### Fixed
- heartbeat runs now skip before publishing inbound work when the bound session or routed active session is unavailable, logging the skip at debug level instead of executing against a deleted session.

## 2026-04-22

### Fixed
- heartbeat 上下文组装现在会过滤带 heartbeat metadata 的 operational transcript 记录，静默 ack 与 heartbeat prompt 不再回流进后续模型上下文，同时保留真正需要用户看到的 heartbeat assistant 输出

## 2026-04-15

### Changed
- heartbeat worker now drops overdue runs that were missed while the runtime was down and reschedules from the current tick instead of replaying them on restart

## 2026-04-02

### Added
- added a persisted `recent_messages_limit` heartbeat-job field (default `12`) so each run can inject a bounded slice of recent session history into `agent.conversation_history`

### Changed
- heartbeat runs now always append the fixed review prompt at publish time, while persisted heartbeat records only carry the optional custom prompt prepended ahead of that fixed instruction
- `HeartbeatManager` can now auto-sync a one-per-session heartbeat binding for supported channel sessions without clobbering user-edited schedule settings

## 2026-03-21

### Added
- added `HeartbeatManager` for persisted heartbeat create/update/delete/get/list/set-enabled and run-history access
- added `HeartbeatWorker` for due-heartbeat scanning, CAS-style claiming, inbound publication, and run-record persistence
- added session-bound heartbeat metadata helpers and silent-ack filtering helpers

### Changed
- heartbeat is now a first-class persisted domain backed by dedicated heartbeat storage tables instead of config-to-cron reconciliation
