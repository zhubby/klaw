# klaw-heartbeat

`klaw-heartbeat` owns session-bound autonomous heartbeat behavior for Klaw.

## Responsibilities

- manage persisted heartbeat job definitions through `HeartbeatManager`
- schedule and publish due heartbeat turns through `HeartbeatWorker`
- preserve heartbeat-specific metadata and silent-ack semantics
- keep heartbeat distinct from isolated cron jobs, while reusing the shared inbound/runtime pipeline

## Notes

- heartbeat jobs are bound to a target `session_key`
- runtime delivery resolves the current `active_session_key` when present, so heartbeat follows the active conversation branch
- heartbeat jobs can persist a per-job recent-message window, allowing each run to inject bounded session history from the resolved conversation branch instead of replaying the full transcript
- heartbeat output is considered silent only when it carries heartbeat metadata and exactly matches the configured silent-ack token
- persistence lives in `klaw-storage` heartbeat tables rather than being projected into cron rows
