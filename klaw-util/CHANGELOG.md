# Changelog

## 2026-03-26

### Added
- added shared command-PATH augmentation helpers so macOS GUI launches can reuse one standard binary search path policy across crates

### Changed
- split `lib.rs` into focused `paths`, `environment`, and `command_path` modules while keeping the public `klaw-util` API re-exported from the crate root

## 2026-03-25

### Added
- added `system_timezone_name()` with an `iana-time-zone` fallback-to-UTC resolver so runtime defaults can reuse the host system timezone label

## 2026-03-20

### Added
- added the new `klaw-util` crate with shared data-directory constants and default path helpers for config, workspace, skills, tokenizers, GUI state, and storage files
