# CHANGELOG

## 2026-03-15

### Added

- initial `klaw-gui` crate with `egui/eframe` workbench shell
- left sidebar navigation for profile/provider/channel/cron/heartbeat/mcp/skill/memory/archive/tool/system-monitor
- new `Configuration` workbench module with `config.toml` editor
- TOML syntax highlighting in configuration editor (section/key/string/number/bool/comment)
- configuration actions: `Save` (validate before persist), `Reset`, `Migrate`, `Reload`
- configuration action: `Validate` (run parse + schema checks without writing file)
- unsaved-changes confirmation before reset/migrate
- global toast notifications via `egui-notify` for configuration operation feedback (success/failure/validation)
- center tabbed workspace with open/activate/close behavior and unique-tab-per-menu policy
- typed menu model, UI action reducer, and workbench tab state machine
- placeholder panel renderer abstraction and per-module panel implementations
- crate-level README and architecture documentation
- top menu bar with File/View/Window/Help actions
- bottom status bar with version indicator and theme switch icon
- `egui-phosphor` icon font integration for sidebar menu items and status UI
- GUI state persistence and restore on startup via `~/.klaw/gui_state.json` (tabs, active tab, theme mode, fullscreen, about visibility)
- load system CJK fonts via `fontdb` as fallback in `egui` font chain, reducing Chinese glyph missing issues
- provider panel now loads providers from `config.toml`, shows active/default/auth details, and supports `Set Active`
- provider add/edit flow via `egui::Window` form with config persistence and validation feedback
- channel panel now loads/writes `channels.dingtalk` and `disable_session_commands_for`, with `egui::Window` add/edit form
- mcp panel now loads/writes global settings and `mcp.servers`, with `egui::Window` add/edit form
- skill panel upgraded to `Skill Registry`, with config-bound registry list and `egui::Window` add/edit form
- cron panel now integrates storage DB operations: list jobs/runs, add/edit via window, and enable/disable/delete
- archive panel now reads `archive.db` through storage DB interface with filters and detail view
- refactored GUI cron/archive to call `klaw-cron` and `klaw-archive` abstractions instead of direct storage operations
- memory panel now shows real memory-layer statistics through `klaw-memory` abstraction
- persisted app window size in UI state and restore on startup (non-fullscreen mode)
- tool panel now renders config-backed tool cards, supports per-tool edit windows, and persists `tools.*` fields (enabled toggles and tool-specific settings) to `config.toml`
- system monitor panel now shows real-time CPU and memory cards with usage percent and absolute memory usage
- top File menu now includes `Force Persist Layout` to flush layout persistence immediately
- heartbeat panel now supports managing `heartbeat.defaults` and `heartbeat.sessions` (add/edit/delete/reload/save)
- sidebar now includes `Session`, `Approval`, and `Skill` menus; `Provider` menu title renamed to `Model Provider`
- status bar now includes runtime provider override dropdown (from `model_providers`) for dynamic runtime provider switching
- system monitor now shows four real-time cards (CPU/memory/data-dir disk usage/app uptime) and detailed system information in English
