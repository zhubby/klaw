# CHANGELOG

## 2026-03-31

### Added

- `SqliteMemoryStatsService` 新增 `list_scope_records(scope)`，支持按 scope 返回完整 memory 记录明细，供 GUI detail 弹窗直接消费

## 2026-03-15

### Added

- `SqliteMemoryStatsService` for memory-layer statistics aggregation
- memory stats model types: `MemoryStats` and `ScopeStat`

### Changed

- GUI `Memory` panel can consume `klaw-memory` stats abstraction instead of placeholder content
