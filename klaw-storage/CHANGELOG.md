# Changelog

## 2026-03-14

### Added
- added session routing/model state persistence fields in `sessions` (`active_session_key`, `model_provider`, `model`)
- added `SessionStorage` APIs for session route/model lifecycle: `get_or_create_session_state`, `set_active_session`, `set_model_provider`, `set_model`

### Changed
- changed SQLx/Turso session store initialization to run idempotent schema upgrades for new session state columns

## 2026-03-13

### Added
- added `archive.db` and `archives/` data directory support to `StoragePaths`
- added `DefaultArchiveDb` and `open_default_archive_db()` for archive persistence

### Changed
- added session JSONL history reads so runtimes can rebuild prior conversation turns before the next LLM call
