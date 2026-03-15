# klaw-gui

`klaw-gui` is the desktop workbench UI crate for Klaw, built with `egui/eframe`.

## Capabilities

- Workbench shell with left navigation and center tab workspace
- Top menu bar (File/View/Window/Help)
- Bottom status bar with version and theme-mode switcher
- UI state persistence across restart (`~/.klaw/gui_state.json`)
- System CJK font fallback via `fontdb` to avoid Chinese text missing-glyph rendering
- Strongly typed menu model for workspace modules
- Single-tab-per-menu behavior (click to open or activate)
- Placeholder panel renderers for:
  - profile
  - configuration
  - provider (config-bound list + add/edit window)
  - channel (config-bound list + add/edit window)
  - cron (db-bound list + add/edit window)
  - heartbeat
  - mcp (config-bound list + add/edit window)
  - skill registry (config-bound list + add/edit window)
  - memory
  - archive (db-bound query + detail view)
  - tool
  - system-monitor
- Configuration panel features:
  - load and edit `config.toml` raw text
  - TOML syntax highlighting (section/key/value/comment)
  - `Validate`, `Save` (validate before persist), `Reset`, `Migrate`, `Reload`
  - dirty-state warning before reset/migrate overwrite
  - global toast notifications for operation feedback
- Provider panel features:
  - read providers from `config.toml` (`model_provider` + `model_providers`)
  - set active provider directly
  - add/edit provider via `egui::Window` form and persist back to config
- Channel panel features:
  - read/write `channels.dingtalk` list from `config.toml`
  - add/edit dingtalk channels via `egui::Window`
  - edit and save `channels.disable_session_commands_for`
- MCP panel features:
  - read/write `mcp.enabled`, `mcp.startup_timeout_seconds`, `mcp.servers`
  - add/edit MCP servers via `egui::Window`
- Skill Registry panel features:
  - read/write `skills.sync_timeout` and registries
  - add/edit registries via `egui::Window`
- Cron panel features:
  - read/manage cron jobs and task runs via `klaw-cron` manager abstraction
  - add/edit cron jobs via `egui::Window`
- Archive panel features:
  - query archives via `klaw-archive` service abstraction with filters
  - inspect archive record details and metadata in a detail window

## Architecture

- `app/`: `eframe::App` implementation and update loop
- `domain/`: core domain enums (menu identity)
- `state/`: UI action model + workbench tab state reducer
- `ui/`: shell/sidebar/workbench composition
- `panels/`: module-specific placeholder panels
- `widgets/`: shared reusable UI widgets
- `theme.rs`: centralized theme setup
  - system-follow default
  - light/dark/system cycling
- `state/persistence.rs`: local UI state load/save with schema versioning and atomic writes

## Running

Use the CLI entrypoint:

```bash
klaw gui
```
