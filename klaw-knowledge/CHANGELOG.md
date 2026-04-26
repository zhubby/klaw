# CHANGELOG

## 2026-04-26

### Added

- 新增 Obsidian vault auto-index watcher，支持监听 Markdown 新增、修改、删除、移动并更新 knowledge 索引
- 新增单文件索引、删除和已有索引增量补偿接口，供 runtime 自动索引复用
- 新增 Turso/libSQL 原生向量列与向量索引初始化，knowledge chunk embeddings 会按模型维度重建为 `F32_BLOB`
- 新增 semantic lane 的数据库内向量查询路径，优先使用 `vector_top_k`，索引不可用时使用 SQL 距离排序后再回退到 Rust 余弦计算
- 新增 Obsidian 链接发现，支持精确名称、alias、Levenshtein 0.92 模糊匹配和 People 首名唯一匹配，并只写入索引图边

### Changed

- Obsidian provider 打开时不再支持启动即全量索引，首次同步保持为显式调用
- Search 结果 metadata 补全改为只按 fused hit ids 批量读取，避免每次搜索全表读取 `knowledge_entries`
- Obsidian markdown chunking 改为 `engraph` 风格的断点评分，优先标题、代码围栏、主题分隔符和空行，并避免切开代码块

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
