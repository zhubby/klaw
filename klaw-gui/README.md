# klaw-gui

`klaw-gui` is the desktop workbench UI crate for Klaw, built with `egui/eframe`.

## Capabilities

- Workbench shell with left navigation and center tab workspace
- Strongly typed menu model for workspace modules
- Single-tab-per-menu behavior (click to open or activate)
- Placeholder panel renderers for:
  - profile
  - provider
  - channel
  - cron
  - heartbeat
  - mcp
  - skill
  - memory
  - archive
  - tool
  - system-monitor

## Architecture

- `app/`: `eframe::App` implementation and update loop
- `domain/`: core domain enums (menu identity)
- `state/`: UI action model + workbench tab state reducer
- `ui/`: shell/sidebar/workbench composition
- `panels/`: module-specific placeholder panels
- `widgets/`: shared reusable UI widgets
- `theme.rs`: centralized theme setup

## Running

Use the CLI entrypoint:

```bash
klaw gui
```
