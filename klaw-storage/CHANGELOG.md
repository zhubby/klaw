# Changelog

## 2026-03-13

### Added
- added `archive.db` and `archives/` data directory support to `StoragePaths`
- added `DefaultArchiveDb` and `open_default_archive_db()` for archive persistence

### Changed
- added session JSONL history reads so runtimes can rebuild prior conversation turns before the next LLM call
