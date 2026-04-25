# Changelog

## 2026-04-25

### Changed
- Archive storage wiring now uses the shared `DatabaseExecutor` abstraction instead of the former `MemoryDb` trait name

## 2026-03-20

### Changed
- removed the `klaw-core` dependency from `klaw-archive`; callers now convert their local media source enums into `ArchiveSourceKind` at the integration boundary

## 2026-03-13

### Added
- introduced the new `klaw-archive` crate for media file archiving
- added SQLite-backed archive service, media sniffing, and file persistence helpers
- added query/get/download abstractions for future tool and channel integrations
