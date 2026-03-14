# klaw-storage

`klaw-storage` provides the local path and persistence abstractions used by Klaw services.

## Responsibilities

- manage the `~/.klaw` data directory layout
- provide session and cron persistence stores
- expose generic SQLite access used by higher-level modules such as memory and archive services
- persist session routing/model state used by IM command routing (`active_session_key`, `model_provider`, `model`)

## Data Layout

```text
~/.klaw/
├── klaw.db
├── memory.db
├── archive.db
├── sessions/
└── archives/
```

## Notes

- `DefaultSessionStore` persists session and cron data
- session records support Base Session -> Active Session routing and per-session provider/model persistence
- `DefaultMemoryDb` provides a generic SQL interface for `klaw-memory`
- `DefaultArchiveDb` provides a generic SQL interface for `klaw-archive`
