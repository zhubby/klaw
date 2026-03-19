# CHANGELOG

## 2026-03-19

### Changed

- `CronWorker` now resolves channel cron deliveries against persisted active-session routing when it can infer a base chat session, and annotates published payloads with `cron.original_session_key` / `cron.resolved_session_key`

## 2026-03-16

### Added

- `CronWorker::run_job_now` for ad hoc execution of a single cron job without waiting for the next poll cycle or mutating its scheduled next-run timestamp

## 2026-03-15

### Added

- `SqliteCronManager` as cron data management abstraction (list/create/update/delete/enable/runs)
- `CronListQuery` for paginated cron job listing

### Changed

- exposed cron domain types from `klaw-cron` for upstream callers (`CronJob`, `CronTaskRun`, `CronScheduleKind`, `NewCronJob`, `UpdateCronJobPatch`)
