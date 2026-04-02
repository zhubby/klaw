# Changelog

## 2026-04-02

### Added
- added a persisted `recent_messages_limit` heartbeat-job field (default `12`) so each run can inject a bounded slice of recent session history into `agent.conversation_history`

## 2026-03-21

### Added
- added `HeartbeatManager` for persisted heartbeat create/update/delete/get/list/set-enabled and run-history access
- added `HeartbeatWorker` for due-heartbeat scanning, CAS-style claiming, inbound publication, and run-record persistence
- added session-bound heartbeat metadata helpers and silent-ack filtering helpers

### Changed
- heartbeat is now a first-class persisted domain backed by dedicated heartbeat storage tables instead of config-to-cron reconciliation
