# klaw-memory

`klaw-memory` provides memory CRUD, retrieval (FTS/vector), long-term memory governance, prompt rendering, and statistics for Klaw.

## Capabilities

- Memory upsert/search/get/delete/pin (`SqliteMemoryService`)
- Long-term memory governance (`kind`/`priority`/`status`/`topic` normalization)
- Low-priority long-term memory archiving with grouped summary rollups
- Prompt-safe long-term memory rendering with priority-aware ordering
- Optional embedding provider integration for vector retrieval
- SQLite-backed statistics aggregation (`SqliteMemoryStatsService`)

## Statistics

`SqliteMemoryStatsService` exposes aggregate metrics including total/pinned/embedded counts,
distinct scopes, update recency windows, index availability (FTS/vector), and top scopes.
