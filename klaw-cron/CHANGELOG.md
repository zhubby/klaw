# CHANGELOG

## 2026-04-25

### Changed

- Cron storage wiring now imports the shared `DatabaseExecutor` abstraction instead of the former `MemoryDb` trait name

## 2026-04-14

### Changed

- 测试用 `FakeStorage` 已补齐新的游标分页历史接口，保持 `SessionStorage` trait 升级后的覆盖完整性

## 2026-04-09

### Fixed

- `CronWorker` 现在会把 execution session 的 `channel.base_session_key` / `channel.delivery_session_key` 连同回复所需的 channel metadata 一起持久化到 session 索引，允许后续 `/approve` 之类的交互命令安全识别并认领 cron 触发的隔离执行 session

## 2026-03-31

### Fixed

- `CronWorker` 现在会为每次任务触发生成独立的 execution `session_key`，并显式注入空的 `agent.conversation_history`，让 cron turn 像 webhook 一样按轮隔离，避免长时间运行后把上下文无限叠加到同一个 session

## 2026-03-30

### Fixed

- `CronWorker` 现在会把 `channel.base_session_key` 与解析后的 `channel.delivery_session_key` 一并写入已发布的入站 metadata，允许后台 channel dispatcher 在 DingTalk `session_webhook` 失效后重新解析最新 active session 并做单次补发

### Changed

- `CronWorker` 新增 `MissedRunPolicy`；默认 `Skip` 会在服务恢复后直接跳到当前时间之后的下一次触发，显式启用 `CatchUp` 时才会逐次补偿停机期间错过的执行点

## 2026-03-29

### Fixed

- `CronWorker` no longer rewrites channel cron deliveries onto the chat's current `active_session_key`; scheduled runs now keep their own stored `session_key` so cron turns do not merge into normal agent-loop history, while channel delivery metadata is still refreshed from the active route when available

## 2026-03-27

### Fixed

- `CronWorker` 和 `SqliteCronManager` 现在会在推进 / 重算 `cron` 任务的 `next_run_at_ms` 时使用任务自身的 IANA `timezone`，不再固定按 UTC 解释 cron 表达式

## 2026-03-26

### Fixed

- `CronWorker` 现在会在跟随 active session 路由后刷新持久化的会话回复元数据，避免 DingTalk cron 继续使用创建任务时保存的旧 `session_webhook`

## 2026-03-21

### Changed

- `CronWorker` 现在也会为 `telegram` cron payload 解析 base session 并跟随持久化的 `active_session_key` 路由到当前激活会话

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
