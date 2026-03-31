# klaw-cron

`klaw-cron` provides cron scheduling execution and cron data management abstractions.

## Capabilities

- Parse and validate cron/every schedules (`ScheduleSpec`)
- Execute due jobs and publish inbound messages (`CronWorker`)
- Execute a single cron job immediately for manual triggers (`CronWorker::run_job_now`)
- Manage cron jobs/runs through a higher-level service (`SqliteCronManager`)

## Main APIs

- `CronWorker` / `CronWorkerConfig`: runtime tick and continuous processing
- `SqliteCronManager`: list jobs, list runs, create/update/delete jobs, enable/disable jobs
- `ScheduleSpec`: schedule parsing and next-run calculation

## Storage Integration

`SqliteCronManager` opens default storage handles internally and centralizes cron data operations, so callers (such as GUI) do not need to query storage tables directly.

`CronWorker` consults persisted session routing state only to refresh channel delivery metadata when possible. Each scheduled run now publishes with a fresh execution `session_key`, while preserving the configured payload key in `cron.original_session_key` and the per-run execution key in `cron.resolved_session_key`. This keeps cron turns isolated from prior cron history without breaking channel delivery via `channel.base_session_key` / `channel.delivery_session_key`.
