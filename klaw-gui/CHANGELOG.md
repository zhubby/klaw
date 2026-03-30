# CHANGELOG

## 2026-03-30

### Changed

- `Session` 面板顶部新增 `channel` 下拉筛选，并把筛选条件下沉到 session SQL 查询层，而不是在 GUI 内存列表中二次过滤

### Fixed

- `Session` 面板的 `Updated At` 列现在支持升序/降序切换，并直接驱动底层 SQL `ORDER BY`
- `Session` 面板记录现在支持双击直接打开 `View Chat`，无需再进入右键菜单

## 2026-03-29

### Added

- `Gateway` 配置弹窗中的 `auth.token` 现在带有 `Generate` 按钮，可直接填充 `sk_...` 风格随机密钥
- `Settings > General` 的浅色主题预设新增 `Crab`，采用 Klaw logo 的奶白、蟹壳橙红、金色点缀与深棕描边配色

### Changed

- `Channel` 面板列表移除了 `Auth` 列；`Status` 改为按运行态显示带颜色的图标+文字，`Proxy` 列收敛为仅显示是否开启
- `Memory` 面板的 `Top Scopes` 改为贴合父容器宽度的表格，并在内容超出时同时提供横向与纵向滚动
- `Webhook` 面板不再允许编辑 webhook path；`events` / `agents` 路径固定显示为 `/webhook/events` 与 `/webhook/agents`
- `Webhook` 面板的 `Inspect Prompt` 右键菜单新增 `Edit`，并复用内置 Markdown 编辑器直接修改现有 prompt 模板

### Fixed

- `Tool` 面板现在会显示 `channel_attachment`，并支持编辑其 `enabled`、本地附件 allowlist 与 `max_bytes` 配置，不再出现 runtime 已注册但 GUI 面板缺失该工具的情况
- `Archive` 面板主列表现在在内容超出可视宽度时提供横向滚动，并继续保留纵向滚动，避免右侧列被截断后无法浏览
- `Archive` 面板记录列表现在支持双击直接打开可预览条目的预览窗口，同时保留现有单击选中与右键菜单操作
- `Gateway` 面板切换到 Tailscale Funnel 时不再错误地强制要求先启用 `gateway.auth`；未配置认证时改为仅显示公网暴露警告
- `Gateway` 状态轮询的 runtime 响应超时从 `200ms` 调整为 `1s`，避免本机 Tailscale 状态探测稍慢时反复报 `timed out waiting for gateway status response`
- `LLM` 面板表格滚动容器不再设置最大宽度，列表区域现在会直接贴合父容器可用宽度
- `LLM` 面板表格列定义现在与实际渲染列一致，`Session` 列改为占用剩余宽度并对长文本做截断悬浮提示，避免长 `session_key` 把右侧列挤压错位

## 2026-03-28

### Changed

- macOS 顶部状态栏图标现在改为左键直接显示并激活主窗口、右键仅弹出 `About` / `Quit Klaw` 菜单；窗口关闭动作同时改为隐藏到状态栏而非直接退出

### Fixed

- macOS `Settings > General > Launch at startup` now manages a real user `LaunchAgent`, only enables from the packaged `Klaw.app` bundle, and re-syncs stale login-item state on GUI startup
- `Channel`、`Voice`、`Tool`、`Skills Registry`、`Skills Manager`、`Model Provider`、`MCP` 与 `Profile Prompt` 面板标题下方不再显示冗余的 `Config` / `Path` / `Workspace Path` / `Skill Root` 状态行

## 2026-03-27

### Added

- `Tool` 面板新增 `geo` 工具配置入口，可直接切换 `tools.geo.enabled`

## 2026-03-27

### Changed

- `Tool Logs` 主窗口改为固定宽高并按 app 视口自动收缩，日志详情从内嵌右侧栏调整为按记录右键打开的固定尺寸 `Summary` 弹窗

### Fixed

- `Skills Manager` 安装窗口在 registry skill 的 `id` 为空时，现在会按 `name` 再回退到 registry 名选择稳定标识，避免 root-level `SKILL.md` 安装时把空 skill 名写进配置
- `Skills Registry` 面板顶部的 `Config`、`Reload`、`Add Skills Registry` 按钮现在放在同一行，避免工具栏被拆成两行后显得松散
- `Cron` 面板在创建 / 编辑任务时，重算 `Next Run At` 现在会把表单里的 `timezone` 一并传入调度计算，避免任务列表继续按 UTC 预览 cron 触发时间
- `Tool Logs` 的 JSON tree 与长文本详情现在都包裹在固定高度的内部滚动区中，展开节点或查看大结果时不再把窗口高度继续撑大
- `Tool Logs` 记录列表改为整行选中并恢复右键菜单交互，`Time` 列同时支持升降序切换，主窗口固定尺寸更新为当前 app 视口的 `2/3`

## 2026-03-26

### Added
- `Tool` 面板右键菜单新增 `Logs` 入口，可按工具查看最近结构化 `tool_audit` 历史并展开单次调用的完整参数、结果、signals 与 metadata
- `Settings > General` 新增 Light/Dark 主题预设下拉框，支持在保留默认 egui 主题的基础上为 light 模式选择 `Latte`，为 dark 模式选择 `Frappé`、`Macchiato`、`Mocha`

### Changed

- GUI 底部状态栏的 provider 下拉现在读取 live runtime provider snapshot，而不是直接轮询 `config.toml`，因此只会展示当前 runtime 真正可切换的 provider/default model
- `Provider` 面板现在会并排显示 `Config default` 与 `Runtime active`，表格中的 provider 标记也会区分配置默认与运行时当前 provider，避免把状态栏临时 override 误读成配置已切换
- GUI 底部状态栏的主题切换从循环点击改为显式 `Theme Mode` 下拉框，并会结合已保存的 Light/Dark 预设应用实际配色且在重启后恢复
- `MCP` 面板的全局设置弹窗移除了 `enabled` 开关，仅保留 `startup_timeout_seconds`；运行时现在默认总是持有可热重载的 MCP manager
- `Tool` 面板从卡片网格改为占满剩余高度的可滚动表格，并按工具名首字母排序展示每个 tool 的状态、描述和运行时 schema 参数数量
- `Skills Registry` 面板将 `skills.sync_timeout` 的输入与 `Save Timeout` 动作收进独立 `Config` 弹窗，主工具栏原位置改为 `Config` 按钮，和其他配置面板保持一致

### Fixed

- `Tool` 面板保存后现在会立即触发 runtime tool sync，行为与 `Provider` / `Channel` / `MCP` 面板保持一致，避免 tool 开关修改后还需要重启 GUI runtime 才能生效
- `Tool` 面板列表现在支持右键菜单 `Edit` / `Inspect`；`Inspect` 弹窗改为展示运行时 tool trait 实际暴露的 `description()` 与 `parameters()` schema，并提取参数描述而不是复用 GUI 配置字段
- `Voice` 面板的 STT/TTS 测试不再额外依赖 `voice.enabled=true`，只要当前选中的 provider 密钥配置完整即可直接验证实际语音链路
- `Provider` 面板新增/编辑/删除 provider 以及切换 active provider 后，现在都会统一触发 runtime provider sync，避免 GUI 配置态、底部切换和实际运行路由继续分叉
- Archive 面板调用系统 `qlmanage` 生成 Quick Look 预览时现在也会复用共享增强后的 PATH，避免 GUI 启动环境遗漏标准外部命令搜索目录

### Changed

- `Voice` 面板移除了旧的 `voice.enabled` 开关，并在摘要区明确标注该字段已废弃；当前面板只维护实际生效的 provider 与默认语音参数

## 2026-03-25

### Added

- `Gateway` 面板现在会独立显示本机 Tailscale 主机状态，包括连接状态、版本、backend state、DNS 名称与 tailnet URL，即使 gateway 尚未运行也能先做环境排查
- `Voice` 面板新增 `TTS Test` 页签，支持输入文本走当前 TTS provider 生成语音，落盘到系统 tmp 目录，并在 GUI 内直接播放/停止生成音频
- `Webhook` 面板新增 prompt 模板管理：支持在 `hooks/prompts` 下创建 Markdown 模板、固定高度列表检查全部模板、右键 `View` / `Trick` / 红色 `Delete`，以及基于当前 gateway/tailscale 运行态生成 `/webhook/agents` 请求地址

### Changed

- `Webhook` 面板配置编辑与摘要展示现支持 `gateway.webhook.events` / `gateway.webhook.agents` 双 endpoint；`Gateway` 面板不再重复暴露 webhook 配置入口，避免与独立 `Webhook` 菜单重叠
- `Webhook` 面板的请求列表现改为严格 `Events / Agents` 双模式切换；`agents` 视图直接读取独立 `webhook_agents` 数据，并以 `hook id` 语义显示查询项、列表列和详情字段
- `Voice` 面板的原 `Microphone Test` 区块改为 `STT Test / TTS Test` 双页签布局；STT 页签保留现有麦克风转写链路，但按钮改为带图标样式，并在录音中显示明显红点状态
- `Gateway` 面板中的 Tailscale 模式切换改为持久化待应用选择，并通过显式 `Apply` 按钮提交，避免下拉选择在下一帧渲染时回退
- `Gateway` 面板不再持久显示 `Last Error` 行，错误改为只通过 toast 等即时反馈通道展示，避免长错误文本破坏状态区布局
- `Gateway` 面板中的 Tailscale `Apply` 失败后会保留用户选中的目标模式，便于修正环境问题后直接重试，而不会被立即锁回已生效配置
- `Gateway` 面板首次状态加载失败时不再永久停留在 `Loading...`，而是显示错误信息与 `Retry` 按钮，便于继续排查 runtime 卡点
- `Gateway` 面板的 gateway / tailscale 操作改为后台请求，不再直接阻塞 GUI 渲染线程；Tailscale 区域也新增独立 `Refresh Tailscale` 按钮，并在本机 Tailscale 服务不可用时禁用 `Apply`

### Fixed

- `Gateway` 面板的 `Reload` 现在会在重新加载磁盘配置后同步刷新运行时 gateway 状态摘要，避免 `Configured` / `Auth` / `Tailscale` 继续停留在旧快照
- `Profile Prompt` 面板 `Workspace Markdown Files` 表格右键菜单新增橙色 `Reset` 操作，仅对内置模板文件显示，并在确认后将目标文件重置为默认模板并同步已打开的编辑/预览状态
- `Provider`、`Memory`、`Gateway`、`Channel`、`MCP`、`Tool`、`Voice`、`Webhook`、`Skills Registry` 与 `Skills Manager` 面板现在都会基于磁盘最新配置做局部更新，避免 stale snapshot 在后续保存时把已落盘的 provider 或其他配置覆盖掉
- `Model Provider` 面板表格现在在内容超出宽度时提供横向滚动、在可视高度不足时提供纵向滚动，避免长列内容被截断后难以浏览
- `Model Provider` 面板 `ID` 列中的 active provider 现在在文字尾部显示绿色勾选图标，右键菜单也补充了带图标的 `Edit` / `Set Active` / `Copy ID` / 红色 `Delete`
- `Model Provider` 面板新增删除确认流程，并阻止删除当前 active provider 或正被 memory embedding 使用的 provider，避免写出无效配置
- GUI 统一时间格式现在按系统本地时区展示 `*_at_ms` / Unix 时间戳，而不再直接按 UTC 语义渲染
- `Cron` 与 `Heartbeat` 新建表单的默认 timezone 改为系统探测值，不再硬编码 `UTC`
- `Profile Prompt` 面板恢复旧的持久化 tab 状态时，现在会把历史标题 `Profile` 自动规范为 `Profile Prompt`
- `Profile Prompt` 面板的 `System Prompt Preview` 改为后台加载，避免首次打开或手动刷新时阻塞 GUI 渲染线程
- `Profile Prompt` 面板的 `Workspace Markdown Files` 与 `System Prompt Preview` 区块现在会按分配高度填充，窗口变高时不再留下异常空隙
- `Profile Prompt` 面板 `Workspace Markdown Files` 列表中的 `Modified` 时间现在复用 GUI 统一时间格式，显示为可读日期而不是 Unix 秒时间戳
- GUI startup sync checks now read only the latest remote manifest needed for update detection instead of loading the full remote manifest history
- GUI startup no longer runs remote retention cleanup when sync is enabled but automatic backup is disabled

### Added

- `Gateway` 面板新增 `Start` 按钮，用于按当前磁盘配置启动 gateway；当服务已运行或正在过渡时按钮会自动禁用

- `Profile Prompt` 面板新增只读 `System Prompt Preview` 区块，使用 markdown 高亮渲染当前 runtime system prompt，并以固定剩余高度显示、内容过长时在框内滚动

## 2026-03-24

### Changed

- GUI 左侧栏现在按 `WORKSPACE`、`AI & CAPABILITY`、`RUNTIME & ACCESS`、`AUTOMATION & OPERATIONS`、`DATA & HISTORY`、`OBSERVABILITY` 分组显示，并在组内按菜单标题首字母排序
- GUI 侧栏菜单文案将 `Setting` 统一为 `Settings`，内部持久化 key `setting` 保持不变

### Added

- GUI 新增 `Voice` 一级 workbench 面板，支持编辑 `config.toml` 中的 voice 配置并执行系统麦克风录音转写测试
- Tool 面板新增 `voice` 开关，支持启停新的 `voice` tool

## 2026-03-23

### Added

- `Profile` 面板新增 `Create File` 弹窗，可直接在 workspace 根目录输入文件名与正文创建新文件

### Changed

- `Profile` 面板创建 workspace 文件时现在会校验文件名仅落在 workspace 根目录下，并阻止覆盖已存在文件
- GUI `Profile` 菜单与标签文案现统一显示为 `Profile Prompt`，内部 `profile` key 保持不变

## 2026-03-23

### Added

- `MCP` panel now shows live runtime server state and tool counts in a selectable table
- `MCP` panel server rows now expose icon-based context actions including a red `Delete`
- `MCP` panel now exposes a `Detail` popup that renders the cached `tools/list` response as markdown-friendly content
- `Memory` panel now exposes a `Config` dialog for editing `memory.embedding.enabled/provider/model` directly from the refresh toolbar

### Changed

- `Setting > Sync` now uses versioned manifest history plus content-addressed blob sync terminology throughout the UI, including last-manifest tracking, remote-manifest lists, and restore messaging
- startup checks, auto sync, and retention cleanup now operate on remote manifests instead of bundle snapshots while preserving the shared sync runtime state between the shell supervisor and settings panel
- `MCP` global settings moved behind a `Config` dialog instead of rendering inline
- `MCP` runtime status refresh now reads a manager snapshot instead of triggering a full sync, avoiding long-lived GUI spinners while keeping polling off the GUI thread
- `Memory` panel provider selection now reads available providers from `config.toml` and fills the embedding model from the selected provider's default model
- GUI app and tray icons now load from embedded image assets at runtime, so both packaged `.app` bundles and standalone macOS binaries keep the custom icon without relying on source-tree file paths
- `Setting > Sync` now validates custom S3 endpoint credentials before startup checks or manual actions run, so R2 users no longer fall through to missing AWS shared-profile files
- `Setting > Sync` manual backup now renders a live progress bar with stage and item detail while snapshot preparation, upload, and retention cleanup run in the background
- Provider 面板切换全局 active provider 后，现在会主动清除临时 runtime override，让运行中的默认 provider 立即回到配置值，避免 GUI 配置态和 runtime 路由继续分叉

## 2026-03-22

### Added

- `Analyze Dashboard` now includes a `Models` view backed by observability local-store data, with provider/model filters, token composition, model/tool success breakdowns, turn-efficiency summaries, and multi-series trend charts

### Changed

- `Analyze Dashboard` now loads both tool analytics and provider/model analytics from the shared local observability store with one refresh path

## 2026-03-22

### Changed

- `Analyze Dashboard` Success Rate Trend now uses egui_plot line chart instead of progress bar list, showing success rate percentage and call volume over time with interactive legend

### Added

- `Setting` 面板现在提供可落盘的 S3 sync 配置，包括 endpoint/region/bucket/prefix、凭证环境变量名、设备 ID、保留策略和自动备份间隔
- `Setting` 面板新增 `Run Backup Now`、远端快照列表和手动恢复确认流程，直接调用 `klaw-storage` 的 snapshot backup/restore 服务

### Changed

- GUI `settings.json` schema 升级到 v2，sync 默认备份范围调整为 session/memory/archive/config/gui settings，并补充最近一次快照状态字段

## 2026-03-21

### Added

- GUI 新增独立 `Gateway` 一级 workbench 面板，支持查看 gateway 运行状态、启停与重启
- GUI 新增独立 `Webhook` 一级 workbench 面板，支持按来源、事件类型、session、状态和时间范围筛选 webhook 事件，并查看 payload / metadata / 错误详情
- GUI 新增 `LLM` 一级 workbench 面板，支持按 session/provider/日期范围过滤请求响应审计记录、按时间列升降序排序，并通过右键菜单打开详情
- GUI 新增 `Analyze Dashboard` 一级 workbench 面板，用于展示本地工具调用分析数据，包括成功率、失败分布、Top 工具和时间窗趋势

### Changed

- `klaw gui` 现在会根据 `gateway.enabled` 在启动时自动拉起内置 gateway，并把运行态信息暴露给 GUI 面板
- `Gateway` 面板现在只显示单个完整服务地址，不再单独列出 WebSocket / Health / Metrics 链接；`Webhook` 面板新增 `gateway.webhook` 配置摘要与弹窗编辑入口
- `LLM` 审计详情窗口现在以内置可交互 JSON tree 渲染 request/response body，并在 JSON 解析失败时回退到只读原始文本
- `Observability` 面板继续作为纯配置页，并新增本地分析存储的开关、保留天数和刷新间隔配置
- `Session` 聊天弹窗现在会根据当前主题切换消息卡片与角色标题配色，浅色模式下用户消息为淡粉背景、助手消息为淡蓝背景，深色模式标题色也调整为更协调的粉蓝系
- `Tool` 配置面板移除了 shell 的 `safe_commands` 与 `approval_policy` 输入项，并为 shell 拆分出 `blocked_patterns` 与 `unsafe_patterns` 两组规则，分别用于直接拒绝和审批
- heartbeat 面板改为直接管理持久化 heartbeat jobs 与 run history，不再编辑 `config.heartbeat.*`；面板新增 `Run Now` 并通过 GUI runtime 立即触发执行

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

## 2026-03-22

### Changed

- sync settings now remove the `MCP` backup option from the GUI, strip legacy `mcp` entries from persisted sync scope, and keep `Skills` plus `User Workspace` in the default snapshot scope
- sync runtime state is now shared between the settings panel and the global shell supervisor, so in-progress task labels, last snapshot metadata, and remote snapshot lists stay aligned
- startup remote snapshot checks now populate the Sync panel state and surface a newer-remote warning directly inside the snapshot actions area
- sync settings now expose a manual `Run Retention Cleanup` action and the shell runs one retention cleanup pass after startup when sync is enabled
- sync settings now default `device_id` from the system hostname and support both direct S3 credential values and env-backed credential references in `settings.json` and the GUI form

## 2026-03-18

### Added

- GUI sidebar now includes `System` and `Setting` menus; `Setting` is a placeholder workbench panel for future settings work
- GUI `System` panel now shows `~/.klaw/tmp` usage through `klaw-storage::StoragePaths`, with refresh and trash-icon cleanup actions
- GUI now includes a dedicated `Logs` workbench panel that streams process logs in real time, with level filters (`trace/debug/info/warn/error/unknown`), keyword search, pause/auto-scroll controls, clear, export-to-file, and bounded in-memory retention
- GUI startup now installs a `tray-icon` status item using `assets/icons`, so Klaw shows an icon in the system tray / macOS menu bar for the full app lifetime
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
