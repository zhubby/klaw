# CHANGELOG

## 2026-03-21

### Added

- GUI 新增独立 `Gateway` 一级 workbench 面板，支持查看 gateway 运行状态、启停与重启
- GUI 新增独立 `Webhook` 一级 workbench 面板，支持按来源、事件类型、session、状态和时间范围筛选 webhook 事件，并查看 payload / metadata / 错误详情
- GUI 新增 `LLM` 一级 workbench 面板，支持按 session/provider/日期范围过滤请求响应审计记录、按时间列升降序排序，并通过右键菜单打开详情
- GUI 新增 `Analyze Dashboard` 一级 workbench 面板，用于展示本地工具调用分析数据，包括成功率、失败分布、Top 工具和时间窗趋势

### Changed

- `klaw gui` 现在会根据 `gateway.enabled` 在启动时自动拉起内置 gateway，并把运行态信息暴露给 GUI 面板
- `LLM` 审计详情窗口现在以内置可交互 JSON tree 渲染 request/response body，并在 JSON 解析失败时回退到只读原始文本
- `Observability` 面板继续作为纯配置页，并新增本地分析存储的开关、保留天数和刷新间隔配置
- `Session` 聊天弹窗现在会根据当前主题切换消息卡片与角色标题配色，浅色模式下用户消息为淡粉背景、助手消息为淡蓝背景，深色模式标题色也调整为更协调的粉蓝系

## 2026-03-20

### Added

- provider/channel form serialization now carries the new streaming config fields, currently defaulting them to `false` until dedicated UI controls are added
- skills registry panel right-click context menu now includes `Delete` option with confirmation dialog
- skills registry panel context menu items now show icons (Sync, Edit, Copy Name, Delete)
- delete option in skills registry context menu uses red warning color for visibility
- `cleanup_registry` API in `klaw-skill` to remove registry-related entries from installed skills manifest

### Changed

- workbench tabs now stay on a single row with horizontal scrolling when they overflow, and the tab strip hides its scrollbar
- activating a workbench tab now moves it to the first position in the tab strip so the selected tab stays leftmost
- GUI default path resolution for `settings.json`, `gui_state.json`, data root, and workspace markdowns now comes from `klaw-util` instead of duplicating `~/.klaw/...` joins across panels and persistence modules
- channel panel now displays per-instance type and runtime status, supports deleting channel instances, and sends a generic `SyncChannels` runtime event after save/reload so running channels update without restarting `klaw gui`
- channel panel now supports both `dingtalk` and `telegram` instances, with separate add/edit forms and shared runtime status rendering
- bottom status bar runtime provider dropdown now sends a live runtime command, so new routes and `/new` immediately use the selected provider override without editing `config.toml`

### Added

- documented the native macOS app packaging flow that wraps the existing GUI entrypoint into `Klaw.app` and a distributable `.dmg`
- archive panel right-click menu now shows `Preview` for supported records and opens an in-app preview window for UTF-8 text, images, and macOS Quick Look-backed document/media thumbnails such as PDF

## 2026-03-19

### Changed

- session panel now shows aggregated input/output/total token counts per session alongside the indexed session list
- provider panel now supports editing and displaying optional `tokenizer_path` for local token estimation fallback

## 2026-03-18

### Added

- GUI sidebar now includes `System` and `Setting` menus; `Setting` is a placeholder workbench panel for future settings work
- GUI `System` panel now shows `~/.klaw/tmp` usage through `klaw-storage::StoragePaths`, with refresh and trash-icon cleanup actions
- GUI now includes a dedicated `Logs` workbench panel that streams process logs in real time, with level filters (`trace/debug/info/warn/error/unknown`), keyword search, pause/auto-scroll controls, clear, export-to-file, and bounded in-memory retention
- GUI startup now installs a `tray-icon` status item using `assets/icons/logo.iconset`, so Klaw shows an icon in the system tray / macOS menu bar for the full app lifetime
- tray status item menu now provides `Open Klaw`, `Setting`, `About`, and `Quit Klaw`; `Setting` currently shows a placeholder notification, while the other actions focus/open the main window, show the existing About dialog, and quit the app
- profile panel now manages workspace markdown docs from `~/.klaw/workspace` using tool-style cards and a popup editor with fixed-height markdown-highlighted text area plus `Save` / `Cancel` / `Reset`

### Changed

- `klaw gui` tracing initialization now fans out logs to both the primary sink and the GUI log channel using a non-blocking writer path, so dropped GUI log events never block runtime logging
- installed-skill management naming is now consistently `Skills Manager` across the sidebar title, panel file/module names, and Rust type/field names to avoid confusion with `Skills Registry`
- GUI 工具配置面板现在独立展示 `skills_registry` 与 `skills_manager` 两个开关
- GUI 技能面板改为通过拆分后的 `SkillsRegistry` / `SkillsManager` 接口读取 registry catalog 与 installed skills

## 2026-03-17

### Changed

- unified GUI timestamp display format to `YYYY/MM/DD HH:MM:SS` across session/approval/archive/cron/skill/memory panels, and formatted system boot time in system monitor with the same style

## 2026-03-16

### Changed

- cron panel now supports `Run Now` from both the jobs table and the task-runs header, routed through the live GUI runtime so manual execution immediately creates a run record and enqueues the inbound work
- cron form now validates `payload_json` against the full `InboundMessage` schema before saving, so missing required fields like `channel` are caught in the GUI
- macOS GUI startup now sets the app icon from `assets/icons/logo.icns`
- system monitor summary cards now render as one row with 4 equal-width cards; CPU/Memory progress bars are width-limited, and data-directory disk usage shows only size (no progress bar)
- system monitor layout now uses `StripBuilder`: four summary cards scale with panel width at fixed inter-card spacing, and `System Information` is rendered in a fixed-height scrollable section
- session panel now lists indexed sessions in a table via `klaw-session` manager abstractions instead of a placeholder view
- approval panel now lists approvals in a table and routes approve/reject/consume actions through `klaw-approval`
- skill panel now manages installed skills via `klaw-skill`, including list/detail, registry sync, and uninstall flows
- skills registry sync entry now lives on the `Skills Registry` list actions instead of the installed `Skills Manager` panel
- skill panel now includes an install window with registry selection and scrollable install/uninstall actions per registry skill
- skill panel now adds `Install Local` flow: pick local `SKILL.md` via `egui-file-dialog`, validate skill name format, and copy the entire local skill directory into `~/.klaw/skills`
- GUI skill actions now trigger a runtime skills-prompt reload command so newly changed skills can apply to subsequent requests without restarting the GUI runtime
- GUI fullscreen persistence now syncs from runtime viewport state each frame, so exiting fullscreen via system window controls is correctly persisted for next launch

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
- skill panel upgraded to `Skills Registry`, with config-bound registry list and `egui::Window` add/edit form
- cron panel now integrates storage DB operations: list jobs/runs, add/edit via window, and enable/disable/delete
- archive panel now reads `archive.db` through storage DB interface with filters and detail view
- refactored GUI cron/archive to call `klaw-cron` and `klaw-archive` abstractions instead of direct storage operations
- memory panel now shows real memory-layer statistics through `klaw-memory` abstraction
- persisted app window size in UI state and restore on startup (non-fullscreen mode)
- tool panel now renders config-backed tool cards, supports per-tool edit windows, and persists `tools.*` fields (enabled toggles and tool-specific settings) to `config.toml`
- system monitor panel now shows real-time CPU and memory cards with usage percent and absolute memory usage
- top File menu now includes `Force Persist Layout` to flush layout persistence immediately
- heartbeat panel now supports managing `heartbeat.defaults` and `heartbeat.sessions` (add/edit/delete/reload/save)
- sidebar now includes `Session`, `Approval`, and `Skills Manager` menus; `Provider` menu title renamed to `Model Provider`
- status bar now includes runtime provider override dropdown (from `model_providers`) for dynamic runtime provider switching
- system monitor now shows four real-time cards (CPU/memory/data-dir disk usage/app uptime) and detailed system information in English
