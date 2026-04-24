# CHANGELOG

## 2026-04-23

### Added

- 长期记忆治理新增 `priority` 元数据校验与默认优先级推导，可显式覆盖 prompt 注入顺序

### Changed

- 长期记忆 prompt 渲染现在优先按显式 `priority` 排序，再回退到 `kind` 优先级
- `SqliteMemoryService::search` 会统一过滤 `long_term` scope 下的 `archived` / `rejected` / `superseded` 记录，避免旧事实继续命中检索
- 低优先级长期记忆现在支持后台自动归档，并按 `kind + topic` 生成 `summary=true` 的摘要索引记录

## 2026-03-31

### Added

- `SqliteMemoryStatsService` 新增 `list_scope_records(scope)`，支持按 scope 返回完整 memory 记录明细，供 GUI detail 弹窗直接消费

## 2026-03-15

### Added

- `SqliteMemoryStatsService` for memory-layer statistics aggregation
- memory stats model types: `MemoryStats` and `ScopeStat`

### Changed

- GUI `Memory` panel can consume `klaw-memory` stats abstraction instead of placeholder content
