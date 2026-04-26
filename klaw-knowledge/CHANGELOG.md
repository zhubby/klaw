# CHANGELOG

## 2026-04-26

### Added

- 新增 Obsidian vault auto-index watcher，支持监听 Markdown 新增、修改、删除、移动并更新 knowledge 索引
- 新增单文件索引、删除和已有索引增量补偿接口，供 runtime 自动索引复用

### Changed

- Obsidian provider 打开时不再支持启动即全量索引，首次同步保持为显式调用

## 2026-04-25

### Added

- Configured Obsidian provider helpers for GUI/runtime callers, including status snapshots and incremental index/vector sync results
- Missing-vector synchronization for already indexed chunks when an embedding model is configured after initial indexing
- Initial `klaw-knowledge` crate for read-only external knowledge retrieval
- Knowledge domain types, Obsidian parsing/chunking helpers, context bundle assembly, and RRF fusion utilities
- Knowledge sync progress events with stage, processed count, total count, and current note/chunk name for GUI feedback
- Shared Knowledge runtime snapshot/state types for runtime-owned loading, ready, syncing, and error reporting

### Changed

- Local llama.cpp model resolution now honors each installed model manifest's default GGUF file
- Knowledge storage access now depends on the shared `DatabaseExecutor` abstraction instead of the misleading `MemoryDb` name

### Fixed

- Obsidian temporal search no longer panics when queries contain non-ASCII text
