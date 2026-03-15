# CHANGELOG

## 2026-03-15

### Added

- initial `klaw-gui` crate with `egui/eframe` workbench shell
- left sidebar navigation for profile/provider/channel/cron/heartbeat/mcp/skill/memory/archive/tool/system-monitor
- center tabbed workspace with open/activate/close behavior and unique-tab-per-menu policy
- typed menu model, UI action reducer, and workbench tab state machine
- placeholder panel renderer abstraction and per-module panel implementations
- crate-level README and architecture documentation
- top menu bar with File/View/Window/Help actions
- bottom status bar with version indicator and theme switch icon
- `egui-phosphor` icon font integration for sidebar menu items and status UI
