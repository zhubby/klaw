# klaw-archive

`klaw-archive` provides media archive persistence for Klaw.

## Responsibilities

- Persist uploaded or generated media files under the Klaw data directory
- Index archived files in `archive.db`
- Detect common media types from file signatures
- Expose a backend-agnostic archive service trait for future tools and channel integrations
- Keep archive domain types independent from `klaw-core`; callers map their own source enums into `ArchiveSourceKind`

## Storage Layout

```text
~/.klaw/
├── archive.db
└── archives/
    └── YYYY-MM-DD/
        └── <uuid>.<ext>
```

## Main API

- `ArchiveService`: ingest, query, load details, and download archived media
- `SqliteArchiveService`: SQLite-backed implementation using `klaw-storage`
- `ArchiveRecord`, `ArchiveIngestInput`, `ArchiveQuery`, `ArchiveBlob`

## Notes

- Physical files are deduplicated by content hash
- Each ingest still creates a separate archive record for auditability
- Media type detection trusts file signatures first and only falls back to the original extension
