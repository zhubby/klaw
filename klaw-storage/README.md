# klaw-storage

`klaw-storage` provides the local path and persistence abstractions used by Klaw services.

## Responsibilities

- manage the `~/.klaw` data directory layout
- provide session, cron, and heartbeat persistence stores
- expose generic SQLite access used by higher-level modules such as memory and archive services
- persist session routing/model state used by IM command routing (`active_session_key`, `model_provider`, `model`)
- sync and restore versioned manifests plus deduplicated blobs for the managed data root via S3-compatible object storage

## Data Layout

```text
~/.klaw/
├── klaw.db
├── memory.db
├── archive.db
├── config.toml
├── settings.json
├── gui_state.json
├── tmp/
├── sessions/
├── archives/
├── skills/
├── skills-registry/
└── workspace/
```

## Notes

- `DefaultSessionStore` persists session, cron, and heartbeat data
- the default Turso-backed session store serializes access through one shared connection to avoid driver-level concurrent-use failures
- `tmp/` is the dedicated temporary data directory under the Klaw data root
- session records support Base Session -> Active Session routing
- `model_provider` / `model` now represent explicit per-session overrides only; global defaults are resolved at runtime and are no longer copied into every session row
- heartbeat records keep session-bound autonomous wakeups separate from isolated cron jobs
- `DefaultMemoryDb` provides a generic SQL interface for `klaw-memory`
- `DefaultArchiveDb` provides a generic SQL interface for `klaw-archive`
- `BackupService` stages managed SQLite/filesystem state into versioned manifests plus content-addressed blobs, uploads only missing blobs, and restores historical manifests after checksum verification
- `BackupService` keeps `latest.json` as the current-manifest ref while preserving `manifests/<id>.json` history for restore and GC
- `BackupService` can emit progress updates for remote reconciliation, manifest preparation, blob upload, manifest publish, and retention cleanup so callers can surface live sync state
- retention cleanup keeps only the configured latest manifest count, refreshes `latest.json`, and removes unreferenced blobs after pruning
- S3 snapshot config accepts either direct credentials (`access_key`, `secret_key`, `session_token`) or environment-variable indirection, and empty device IDs normalize to the local hostname
- custom S3 endpoints such as R2 must provide explicit credentials or credential env names instead of relying on AWS shared-profile discovery
