# klaw-memory

`klaw-memory` provides memory CRUD, retrieval (FTS/vector), and statistics for Klaw.

## Capabilities

- Memory upsert/search/get/delete/pin (`SqliteMemoryService`)
- Optional embedding provider integration for vector retrieval
- SQLite-backed statistics aggregation (`SqliteMemoryStatsService`)

## Statistics

`SqliteMemoryStatsService` exposes aggregate metrics including total/pinned/embedded counts,
distinct scopes, update recency windows, index availability (FTS/vector), and top scopes.
