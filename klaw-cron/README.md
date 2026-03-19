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

`CronWorker` also consults persisted session routing state when possible. For channel-aware cron payloads, the stored payload `session_key` is treated as the original trigger context, while the actual published target may be rewritten to the channel's current active session. The worker records both values in payload metadata as `cron.original_session_key` and `cron.resolved_session_key`.
