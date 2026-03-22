# klaw-storage

`klaw-storage` provides the local path and persistence abstractions used by Klaw services.

## Responsibilities

- manage the `~/.klaw` data directory layout
- provide session, cron, and heartbeat persistence stores
- expose generic SQLite access used by higher-level modules such as memory and archive services
- persist session routing/model state used by IM command routing (`active_session_key`, `model_provider`, `model`)
- create and restore snapshot bundles for the managed data root via S3-compatible object storage

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
- session records support Base Session -> Active Session routing and per-session provider/model persistence
- heartbeat records keep session-bound autonomous wakeups separate from isolated cron jobs
- `DefaultMemoryDb` provides a generic SQL interface for `klaw-memory`
- `DefaultArchiveDb` provides a generic SQL interface for `klaw-archive`
- `BackupService` snapshots managed SQLite/filesystem state into `manifest.json` plus `bundle.tar.zst`, uploads them, and restores full snapshots after checksum verification
- retention cleanup keeps only the configured latest snapshot count and refreshes `latest.json` after pruning
- S3 snapshot config accepts either direct credentials (`access_key`, `secret_key`, `session_token`) or environment-variable indirection, and empty device IDs normalize to the local hostname
