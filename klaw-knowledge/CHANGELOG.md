# CHANGELOG

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
