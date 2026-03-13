# klaw-storage

`klaw-storage` provides the local path and persistence abstractions used by Klaw services.

## Responsibilities

- manage the `~/.klaw` data directory layout
- provide session and cron persistence stores
- expose generic SQLite access used by higher-level modules such as memory and archive services

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
- `DefaultMemoryDb` provides a generic SQL interface for `klaw-memory`
- `DefaultArchiveDb` provides a generic SQL interface for `klaw-archive`
