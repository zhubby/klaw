# Changelog

## 2026-03-21

### Added
- added `HeartbeatManager` for persisted heartbeat create/update/delete/get/list/set-enabled and run-history access
- added `HeartbeatWorker` for due-heartbeat scanning, CAS-style claiming, inbound publication, and run-record persistence
- added session-bound heartbeat metadata helpers and silent-ack filtering helpers

### Changed
- heartbeat is now a first-class persisted domain backed by dedicated heartbeat storage tables instead of config-to-cron reconciliation
