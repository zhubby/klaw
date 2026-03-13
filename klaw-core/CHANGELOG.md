# Changelog

## 2026-03-13

### Added
- added shared `MediaReference` and `MediaSourceKind` models for media-aware channel and archive integrations

### Fixed
- fixed agent loop dropping persisted conversation history by restoring prior session turns from inbound metadata before building the LLM request
