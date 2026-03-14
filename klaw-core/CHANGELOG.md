# Changelog

## 2026-03-14

### Changed
- added debug-level tool result logging in `AgentLoop` executor after each tool call (success/failure), with output truncation to prevent oversized log lines
- changed `AgentLoop` to support runtime provider registry routing, selecting provider/model from inbound metadata (`agent.provider_id` / `agent.model`) per message

## 2026-03-13

### Added
- added shared `MediaReference` and `MediaSourceKind` models for media-aware channel and archive integrations

### Fixed
- fixed agent loop dropping persisted conversation history by restoring prior session turns from inbound metadata before building the LLM request
