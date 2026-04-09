# klaw-gui

`klaw-gui` is the desktop workbench UI crate for Klaw, built with `egui/eframe`.

## Capabilities

- Workbench shell with left navigation and center tab workspace
- Left navigation groups sidebar menus by domain and sorts items alphabetically within each group
- Workbench sidebar now includes `System`, `Settings`, and `Terminal`
- Workbench sidebar now includes dedicated `Gateway` and `Webhook` panels
- Workbench sidebar now includes a dedicated `Voice` panel for voice config editing and split STT/TTS testing
- Top menu bar (File/View/Window/Help)
  - File menu includes `Force Persist Layout` to immediately flush layout state to disk
- Bottom status bar with version and theme-mode dropdown
  - Runtime provider override dropdown on the right (select from `model_providers` without editing config; applies immediately to the running runtime's default provider for new routes and `/new`)
- `About Klaw` dialog now centers the title and shows the embedded app icon, version, build-time git commit sha, and repository link
- System tray / macOS menu bar icon loaded from embedded PNG assets at runtime
  - macOS menu bar icon now uses left click to show and activate the main window
  - right click opens a compact menu with `About` and `Quit Klaw`
- UI state persistence across restart (`~/.klaw/gui_state.json`)
  - includes tabs/theme mode/light-dark theme presets/fullscreen and window size
- macOS app icon is loaded from embedded image bytes at startup, so both `.app` bundles and standalone binaries keep the custom icon
- macOS window close requests now hide the app to the menu bar instead of quitting; tray `Quit Klaw` remains the explicit full-exit path
- macOS `Launch at startup` now provisions a user `LaunchAgent` from the packaged `.app` bundle and re-syncs stale login-item state on startup
- Shared `klaw-ui-kit::install_fonts()` setup, including embedded LXGW WenKai fonts, Phosphor icon font registration, and desktop system CJK fallback via `fontdb`
- Strongly typed menu model for workspace modules
- Sidebar group headings for `WORKSPACE`, `AI & CAPABILITY`, `RUNTIME & ACCESS`, `AUTOMATION & OPERATIONS`, `DATA & HISTORY`, and `OBSERVABILITY`
- Single-tab-per-menu behavior (click to open or activate)
- Workbench panel renderers for:
  - profile (workspace markdown doc cards + editor window + runtime system prompt preview)
  - configuration
  - terminal (embedded `egui_term` PTY view with start/restart/stop controls, default workspace working directory, and tab-close cleanup)
  - model provider (config-bound list + add/edit window)
  - channel (config-bound list + add/edit window)
  - voice (config-bound voice settings + split STT/TTS test workspace)
  - cron (db-bound list + add/edit window)
  - heartbeat (db-backed heartbeat list + add/edit/delete/run-now)
- gateway (runtime-backed gateway status, disk-config reload sync, start, restart, base address display, independent Tailscale host status, background gateway/tailscale actions, explicit Tailscale mode apply flow, Tailscale-only refresh/apply guards when the local service is unavailable, and `auth.token` random-secret generation from the config dialog)
  - webhook (db-backed webhook event list, filters, detail inspection, `gateway.webhook.events` / `gateway.webhook.agents` config editing, local `hooks/prompts/*.md` template management with shared markdown editor highlighting plus CommonMark preview, and generated `/webhook/agents` trick URLs based on current gateway/tailscale runtime state)
  - mcp (config-bound list + add/edit window)
  - skill (installed skill management with list/detail/remove/sync actions)
  - skills registry (config-bound list + add/edit window)
  - memory
  - archive (db-bound query + detail view)
  - tool (remaining-height sortable table with right-click `Edit` / `Inspect` / `Logs`, runtime schema parameter summary, inspect popup sourced from live tool definitions, and per-tool audit history/detail viewer)
  - analyze dashboard
  - system (tmp directory usage and cleanup)
  - setting (general/network plus versioned manifest sync, retention, and restore)
- system-monitor (real-time CPU/memory/data-dir/uptime cards in a 2x2 equal-width layout, plus detailed system information)
- logs panel (live tracing stream in-process with level filters, keyword search, pause stream, auto-scroll, clear, export, and bounded in-memory buffer)
- analyze dashboard panel (local observability-backed tool and model analytics with time-range switching, provider/model filters, token composition, error breakdown, tool success breakdown, and smoothed trend curves)
- GUI timestamp rendering uses the host system timezone for human-readable datetime display
- Configuration panel features:
  - load and edit `config.toml` raw text
  - TOML syntax highlighting (section/key/value/comment)
  - `Validate`, `Save` (validate before persist), `Reset`, `Migrate`, `Reload`
  - dirty-state warning before reset/migrate overwrite
  - global toast notifications for operation feedback
- Profile Prompt panel features:
  - read markdown files directly under `~/.klaw/workspace`
  - show workspace docs in a table with file summary, modified time, and path
  - render a read-only runtime system prompt preview that loads asynchronously and fills the remaining panel height
  - create a new workspace-root file from a popup with `file name` and `body`
  - edit and create workspace markdown files with a shared markdown-highlighted `TextEdit` layouter
  - save, cancel, reset-to-original, or reset-to-default in the editor footer
  - expose row context actions for preview, edit, guarded reset-to-default, and delete
- Provider panel features:
  - provider/channel editors already preserve the new streaming config fields in the config model, though the current GUI still leaves them at their default `false` values
  - read providers from `config.toml` (`model_provider` + `model_providers`)
  - render providers in a scrollable table that supports both horizontal and vertical overflow
  - show `Config default` and `Runtime active` provider summaries separately so runtime overrides do not masquerade as config changes
  - distinguish config-default and runtime-active providers in the `ID` column with separate badges
  - set active provider directly and sync the running runtime provider registry/default route so the app follows the saved global default without restart
  - add/edit provider via `egui::Window` form, persist back to config, and immediately sync live runtime provider state
  - expose icon-based row actions for edit, set-active, copy-id, and guarded delete
- Bottom status bar provider switcher features:
  - read available providers/default models from the live runtime snapshot instead of raw config polling
  - keep the selected runtime override in sync with provider sync operations so the dropdown never offers providers the running runtime cannot actually use
- Channel panel features:
  - read/write channel config from `config.toml` for `channels.dingtalk` and `channels.telegram`
  - add/edit current dingtalk and telegram channels via `egui::Window`
  - show per-instance `type / id / enabled / status` with color-coded runtime state icons, without exposing auth secrets in the list, and with proxy reduced to an on/off indicator
  - delete channel instances from the table
  - edit and save `channels.disable_session_commands_for`
  - request a live GUI runtime `SyncChannels` after channel saves/reloads so running channel instances update without restarting the app
- Voice panel features:
  - read/write `voice.enabled`, default language/voice, and provider-specific Deepgram/AssemblyAI/ElevenLabs fields
  - show configured key source per provider (`api_key` vs `api_key_env`) without exposing secret values in the summary view
  - switch between `STT Test` and `TTS Test` tabs inside the same panel
  - capture microphone audio from the system default input device, encode it as WAV, and send it to the configured STT provider for a full-chain transcription test
  - show explicit icon-based STT controls plus a prominent red-dot recording indicator while recording is active
  - synthesize typed text through the configured TTS provider, save generated audio into the host tmp directory, and show the resulting output path and metadata
  - play and stop generated TTS audio directly inside the GUI after synthesis completes
  - surface recording/transcribing/synthesizing progress plus transcript, device, audio format, tmp output path, and playback status in the panel
- MCP panel features:
  - read/write `mcp.startup_timeout_seconds` and `mcp.servers`
  - add/edit MCP servers via `egui::Window`
- Skills Registry panel features:
  - read/write `skills.sync_timeout` and registries
  - manage `skills.sync_timeout` through a `Config` popup instead of inline controls
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
  - show total/pinned/embedded/scope/recency/index metrics and a parent-width `Top Scopes` table with horizontal and vertical scrolling when needed
- System panel features:
  - resolve `~/.klaw/tmp` through `klaw-storage::StoragePaths`
  - calculate the temporary directory size on demand
  - clear all temporary files and folders while keeping the `tmp/` root directory
- MCP panel features:
  - manage global MCP settings through a `Config` popup instead of inline controls
  - render configured servers in a selectable `TableBuilder` list with right-click `Detail` / `Edit` / `Config` / `Delete` actions
  - poll runtime MCP status asynchronously from a manager snapshot so GUI refreshes do not block the egui thread or retrigger MCP sync
  - show per-server runtime state and discovered tool counts directly in the table
  - open a detail popup that renders the cached MCP `tools/list` response through a shared CommonMark viewer
- Settings panel features:
  - configure GUI theme presets in `General`, with `Default`/`Latte`/`Crab` for light mode and `Default`/`Frappé`/`Macchiato`/`Mocha` for dark mode
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
  - support SQL-backed `channel` dropdown filtering and `Updated At` ascending/descending sorting
  - open the chat history window directly by double-clicking a session row
- Approval panel features:
  - read approvals via `klaw-approval` manager abstraction
  - resolve `approve` / `reject` and trigger `consume` from a table view
- Cron panel features:
  - read/manage cron jobs and task runs via `klaw-cron` manager abstraction
  - add/edit cron jobs via `egui::Window`
  - manually trigger `Run Now` from the jobs table or runs section through the live GUI runtime, with the request polled asynchronously so the egui thread stays responsive while the runtime drains the triggered turn
- Heartbeat panel features:
  - read/manage persisted heartbeat jobs and run history via `klaw-heartbeat`
  - add/edit heartbeat jobs via `egui::Window`
  - view and edit the per-job inherited recent-message window used for bounded heartbeat context
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
  - includes `terminal` panel backed by `egui_term` and a lazily started local PTY session
- `widgets/`: shared reusable UI widgets
  - includes shared markdown helpers for code-style `TextEdit` layouters and CommonMark rich rendering
- `theme.rs`: centralized visual theme setup
  - system-follow mode selection plus configurable light/dark theme presets, including the custom `Crab` light palette derived from the Klaw gateway logo
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
