# klaw-gui

`klaw-gui` is the desktop workbench UI crate for Klaw, built with `egui/eframe`.

## Capabilities

- Workbench shell with left navigation and center tab workspace
- Left navigation groups sidebar menus by domain and sorts items alphabetically within each group
- Workbench sidebar now includes `System` and `Settings`
- Workbench sidebar now includes dedicated `Gateway` and `Webhook` panels
- Workbench sidebar now includes a dedicated `Voice` panel for voice config editing and microphone-to-STT testing
- Top menu bar (File/View/Window/Help)
  - File menu includes `Force Persist Layout` to immediately flush layout state to disk
- Bottom status bar with version and theme-mode switcher
  - Runtime provider override dropdown on the right (select from `model_providers` without editing config; applies immediately to the running runtime's default provider for new routes and `/new`)
- System tray / macOS menu bar icon loaded from embedded PNG assets at runtime
  - tray menu includes `Open Klaw`, `Settings`, `About`, and `Quit Klaw`
  - `Settings` opens the in-app settings workbench
- UI state persistence across restart (`~/.klaw/gui_state.json`)
  - includes tabs/theme/fullscreen and window size
- macOS app icon is loaded from embedded image bytes at startup, so both `.app` bundles and standalone binaries keep the custom icon
- System CJK font fallback via `fontdb` to avoid Chinese text missing-glyph rendering
- Strongly typed menu model for workspace modules
- Sidebar group headings for `WORKSPACE`, `AI & CAPABILITY`, `RUNTIME & ACCESS`, `AUTOMATION & OPERATIONS`, `DATA & HISTORY`, and `OBSERVABILITY`
- Single-tab-per-menu behavior (click to open or activate)
- Workbench panel renderers for:
  - profile (workspace markdown doc cards + editor window + runtime system prompt preview)
  - configuration
  - model provider (config-bound list + add/edit window)
  - channel (config-bound list + add/edit window)
  - voice (config-bound voice settings + microphone transcription test)
  - cron (db-bound list + add/edit window)
  - heartbeat (db-backed heartbeat list + add/edit/delete/run-now)
  - gateway (runtime-backed gateway status, enable/disable, restart, and base address display)
  - webhook (db-backed webhook event list, filters, detail inspection, and `gateway.webhook` config editing)
  - mcp (config-bound list + add/edit window)
  - skill (installed skill management with list/detail/remove/sync actions)
  - skills registry (config-bound list + add/edit window)
  - memory
  - archive (db-bound query + detail view)
  - tool
  - analyze dashboard
  - system (tmp directory usage and cleanup)
  - setting (general/network plus versioned manifest sync, retention, and restore)
- system-monitor (real-time CPU/memory/data-dir/uptime cards in a 2x2 equal-width layout, plus detailed system information)
- logs panel (live tracing stream in-process with level filters, keyword search, pause stream, auto-scroll, clear, export, and bounded in-memory buffer)
- analyze dashboard panel (local observability-backed tool and model analytics with time-range switching, provider/model filters, token composition, error breakdown, tool success breakdown, and trend sampling)
- GUI timestamp rendering uses the host system timezone for human-readable datetime display
- Configuration panel features:
  - load and edit `config.toml` raw text
  - TOML syntax highlighting (section/key/value/comment)
  - `Validate`, `Save` (validate before persist), `Reset`, `Migrate`, `Reload`
  - dirty-state warning before reset/migrate overwrite
  - global toast notifications for operation feedback
- Profile Prompt panel features:
  - read markdown files directly under `~/.klaw/workspace`
  - show workspace docs as cards with file summary, modified time, and path
  - render a read-only runtime system prompt preview that loads asynchronously and fills the remaining panel height
  - create a new workspace-root file from a popup with `file name` and `body`
  - edit a document in a fixed-height markdown-highlighted popup editor
  - save, cancel, or reset in the editor footer
- Provider panel features:
  - provider/channel editors already preserve the new streaming config fields in the config model, though the current GUI still leaves them at their default `false` values
  - read providers from `config.toml` (`model_provider` + `model_providers`)
  - set active provider directly and clear any temporary runtime override so the running runtime immediately follows the saved global default again
  - add/edit provider via `egui::Window` form and persist back to config
- Channel panel features:
  - read/write channel config from `config.toml` for `channels.dingtalk` and `channels.telegram`
  - add/edit current dingtalk and telegram channels via `egui::Window`
  - show per-instance `type / id / enabled / status`
  - delete channel instances from the table
  - edit and save `channels.disable_session_commands_for`
  - request a live GUI runtime `SyncChannels` after channel saves/reloads so running channel instances update without restarting the app
- Voice panel features:
  - read/write `voice.enabled`, default language/voice, and provider-specific Deepgram/AssemblyAI/ElevenLabs fields
  - show configured key source per provider (`api_key` vs `api_key_env`) without exposing secret values in the summary view
  - capture microphone audio from the system default input device, encode it as WAV, and send it to the configured STT provider for a full-chain transcription test
  - surface recording/transcribing progress plus transcript, language, confidence, device, and audio format metadata in the panel
- MCP panel features:
  - read/write `mcp.enabled`, `mcp.startup_timeout_seconds`, `mcp.servers`
  - add/edit MCP servers via `egui::Window`
- Skills Registry panel features:
  - read/write `skills.sync_timeout` and registries
  - add/edit registries via `egui::Window`
  - sync a registry's installed skills directly from the registry list actions
  - request a runtime skills-prompt reload after registry config/save and sync actions
- Skills Manager panel features:
  - read installed skills from `klaw-skill` merged store view
  - inspect source metadata and `SKILL.md` content in a detail window
  - open an install window with registry selection and a scrollable registry skill table
  - install local skills by selecting a local `SKILL.md` with `egui-file-dialog`, validating name format, and copying the full source directory to `~/.klaw/skills/<name>`
  - install/uninstall registry-managed skills through the installed-skills manager flow
  - uninstall local skills and registry-managed skills
  - request a runtime skills-prompt reload after install/uninstall actions
- Memory panel features:
  - read memory-layer aggregate stats via `klaw-memory` stats abstraction
  - open a `Config` dialog from the toolbar to edit `memory.embedding.enabled/provider/model`
  - populate the provider picker from configured `model_providers` and default the model field from the selected provider's `default_model`
  - show total/pinned/embedded/scope/recency/index metrics and top scopes
- System panel features:
  - resolve `~/.klaw/tmp` through `klaw-storage::StoragePaths`
  - calculate the temporary directory size on demand
  - clear all temporary files and folders while keeping the `tmp/` root directory
- MCP panel features:
  - manage global MCP settings through a `Config` popup instead of inline controls
  - render configured servers in a selectable `TableBuilder` list with right-click `Detail` / `Edit` / `Config` / `Delete` actions
  - poll runtime MCP status asynchronously from a manager snapshot so GUI refreshes do not block the egui thread or retrigger MCP sync
  - show per-server runtime state and discovered tool counts directly in the table
  - open a detail popup that renders the cached MCP `tools/list` response for the selected server
- Settings panel features:
  - persist sync settings in `settings.json`, including S3 endpoint/region/bucket/prefix, backup scope, retention, schedule, hostname-based device ID, and both direct or env-backed credentials
  - trigger manual manifest sync runs against the remote blob store
  - show a live progress bar plus stage/detail text while manual sync is reconciling, uploading blobs, publishing manifests, and pruning remote history
  - trigger manual retention cleanup against remote manifests
  - list remote manifests and manually restore a selected manifest version
  - surface startup remote-update detection via a lightweight latest-manifest check, while full remote manifest history loads only on manual refresh or backup flows
  - validate custom S3 endpoint credentials up front so R2-style endpoints do not rely on AWS shared-profile discovery
- Session panel features:
  - read indexed sessions via `klaw-session` manager abstraction
  - render session metadata in a read-only table with limit/offset controls
- Approval panel features:
  - read approvals via `klaw-approval` manager abstraction
  - resolve `approve` / `reject` and trigger `consume` from a table view
- Cron panel features:
  - read/manage cron jobs and task runs via `klaw-cron` manager abstraction
  - add/edit cron jobs via `egui::Window`
  - manually trigger `Run Now` from the jobs table or runs section through the live GUI runtime
- Heartbeat panel features:
  - read/manage persisted heartbeat jobs and run history via `klaw-heartbeat`
  - add/edit heartbeat jobs via `egui::Window`
  - keep form-only heartbeat defaults locally in the GUI instead of writing back to config
  - manually trigger `Run Now` through the live GUI runtime
- Archive panel features:
  - query archives via `klaw-archive` service abstraction with filters
  - inspect archive record details and metadata in a detail window
  - preview supported archived files from the table context menu
  - render UTF-8 text directly, show images inline, and use macOS Quick Look thumbnails for PDF and other common document/media previews when available

## Architecture

- `app/`: `eframe::App` implementation and update loop
- `domain/`: core domain enums (menu identity)
- `state/`: UI action model + workbench tab state reducer
- `ui/`: shell/sidebar/workbench composition
- `panels/`: module-specific workbench panels
  - includes `logs` panel backed by a non-blocking runtime log chunk bridge
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

## macOS Packaging

The repository-level macOS packaging flow wraps the existing GUI-capable `klaw` binary into a native app bundle:

```bash
make build-macos-app
make package-macos-dmg
```

The app bundle still ships `assets/icons/logo.icns` for Finder/Dock bundle metadata, while runtime window/tray icon loading uses embedded assets so distributed binaries do not depend on the source tree layout. Packaged artifacts are emitted to `dist/macos/`.
