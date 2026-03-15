# CHANGELOG

## 2026-03-15

### Added

- `SqliteCronManager` as cron data management abstraction (list/create/update/delete/enable/runs)
- `CronListQuery` for paginated cron job listing

### Changed

- exposed cron domain types from `klaw-cron` for upstream callers (`CronJob`, `CronTaskRun`, `CronScheduleKind`, `NewCronJob`, `UpdateCronJobPatch`)
