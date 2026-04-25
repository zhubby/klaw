# Changelog

## 2026-04-25

### Fixed
- `knowledge` tool registration no longer reindexes the configured vault; indexing remains an explicit knowledge sync/startup responsibility outside the tool registry gate

### Changed
- `knowledge` tool startup now reuses the shared configured Obsidian provider builder used by GUI/runtime knowledge flows

## 2026-04-20

### Changed
- `local_search` now returns structured JSON in `content_for_model`, including `matches`, `total_matches`, `total_matches_known`, `returned_matches`, and explicit limit truncation metadata; once a search stops early at `limit`, it marks the total as unknown instead of scanning the entire tree just to count the remainder
- `local_search` fallback discovery now respects repository ignore rules through the `ignore` crate instead of only skipping hard-coded directories, while explicit file targets still bypass parent-directory ignore rules

## 2026-04-14

### Fixed
- `apply_patch` 现在会在请求写入未授权目录时创建审批记录并返回 `approval_required` 结构化错误/信号，而不是直接以 `ExecutionFailed` 中断 agent loop；批准后可通过 `apply_patch.approval_id` 或最近一次匹配审批重试同一补丁请求

### Fixed
- `cron_manager` 现在会拒绝来自 `cron:*` 执行会话的 `create(message=...)` 调用，避免已触发的 cron 任务再次把建定时任务指令投回模型后自我复制出大量重复 cron

## 2026-04-13

### Changed
- `cron_manager.create` 现在收口为基于 `source_session_key` 的交互式会话绑定模型：调用方必须传当前对话 session，tool 会自动生成 cron payload 并固化 `cron.source_session_key` / `cron.base_session_key`

### Removed
- `cron_manager.create` / `update` 不再接受手工路由用的 `payload`、`payload_json`、`channel`、`chat_id`、`session_key` 参数，避免模型创建出无法投递的定时任务

### Fixed
- `cron_manager` 现在会从当前对话 session 反查 base session，覆盖 terminal active child、DingTalk/Telegram 当前会话和 websocket 会话的 cron 绑定场景

## 2026-04-07

### Fixed
- `ask_question` no longer exposes the unsupported `allow_multiple` parameter in its public schema, and now rejects callers that still send it as an unknown field so the tool contract stays strictly single-select

### Changed
- `ask_question` now describes its preferred use cases in model-facing metadata more explicitly, including when to use it, when not to use it, and how to present a recommended single-select option

### Added
- added an `ask_question` tool that persists a pending single-select question, emits a `question_single_select` IM card, and stops the current turn until the user answers

### Changed
- `ask_question` now reuses shared session storage so pending card answers survive channel round-trips and can be resumed from the original conversation session

## 2026-04-06

### Fixed
- `shell` approval signals now carry `command_preview` in the propagated `approval_required` payload, so Telegram and DingTalk approval cards can show the exact pending command instead of only the approval id

## 2026-04-02

### Changed
- `heartbeat_manager` is now scoped to the current conversation heartbeat only, exposing just `get` and `update` instead of generic CRUD/list operations
- `heartbeat_manager.update` now resolves the base-session heartbeat from the current active session and only edits the persisted custom prompt, while the fixed system heartbeat prompt stays runtime-managed

## 2026-03-31

### Fixed
- `cron_manager` test coverage now asserts that `update`, `delete`, and `set_enabled` surface errors for missing cron ids, protecting the tool layer from regressing back to false-success responses

## 2026-03-30

### Removed
- deleted the empty typo placeholder file `src/event_subcribe.rs`

## 2026-03-28

### Added
- expanded `channel_attachment` so it can send either an existing `archive_id` or a policy-checked local file path back into the active chat, and wired it into `tools.channel_attachment.enabled`
- `ToolOutput` now supports success-path structured signals, allowing non-error tools to ask runtime for side effects such as channel attachment delivery

## 2026-03-27

### Added
- 新增 `geo` 工具：在 macOS 上通过系统 `CoreLocation` 请求当前设备坐标，并在权限被拒、服务关闭或定位超时时返回明确错误

## 2026-03-27

### Fixed
- `cron_manager` 现在会在创建 / 更新 `cron` 类型任务时按传入的 `timezone` 重算 `next_run_at_ms`，并对无效 IANA 时区返回参数校验错误

## 2026-03-26

### Changed
- test fixtures and delegated audit flows now account for `AgentExecutionOutput.tool_audits`, keeping `sub_agent` diagnostics aligned with runtime tool-audit persistence

## 2026-03-26

### Changed
- `sub_agent` no longer requires planner-provided `context.session`; it now binds child execution to the current tool session, generates a unique delegated child session key per run, and keeps optional `context` as supplemental metadata only

### Fixed
- `shell`、`local_search` 与 `terminal_multiplexer` 在按命令名启动 `sh`/`rg`/`grep`/`tmux` 时现在会统一注入共享增强后的 PATH，改善 macOS GUI 启动下的外部命令发现能力
- `sub_agent` now re-surfaces delegated `approval_required` / `stop` signals to the parent agent instead of swallowing them at the tool boundary
- `sub_agent` now forwards delegated LLM audit payloads to the runtime audit sink so parent-session observability can include child agent model requests

## 2026-03-25

### Changed
- changed `cron_manager` and `heartbeat_manager` timezone defaults to use the detected system timezone when callers omit `timezone`

## 2026-03-24

### Added
- added `archive.list_session_attachments` to list archived files from the current session across prior turns
- added `voice` tool with `stt` (archived audio -> transcript text) and `tts` (text -> archived generated audio) actions
- added `skills_registry.add`, `skills_registry.sync`, and `skills_registry.delete` so registry sources can be managed directly from the tool without editing config first
- added `ToolSignal::stop_current_turn`, a shared `stop` signal constructor for tools that need to terminate the current agent turn early

### Changed
- clarified the `archive` tool metadata so models prefer `get` when an exact `archive_id` is already present and use `list_current_attachments` only for current-message attachments
- changed `skills_registry` from browse-only to full registry lifecycle management while keeping `list` / `show` / `search` for synced catalogs
- redesigned `terminal_multiplexer` as a tmux-only interactive session orchestrator with a private socket, structured session metadata, pane monitor commands, and `wait_for_text` prompt synchronization
- bounded `terminal_multiplexer` auto-observation within a single user turn and emit a stop signal with captured pane state when the observation budget is exhausted, preventing open-ended tmux watch loops from consuming the whole agent run

## 2026-03-21

### Added
- added `heartbeat_manager` for persisted session-bound heartbeat create/update/delete/get/list/list-runs and enable/disable operations

### Changed
- `local_search` 现在优先使用 `rg`，并在系统缺少 ripgrep 时回退到兼容 BSD/macOS 的 `grep`，同时保留 `include_pattern` 与默认目录排除行为
- `shell` 现在同时支持 `blocked_patterns` 与 `unsafe_patterns`：前者直接拒绝，后者触发审批，未命中任一模式的命令将直接执行

### Fixed
- `cron_manager` 现在支持注入共享 `session_store`，避免 runtime 内对同一个 `klaw.db` 重新打开独立连接并触发 SQLite `database is locked`
- `cron_manager` message shortcut 现在能从 Telegram 子会话推断正确的 `chat_id` 与 `cron.base_session_key`，避免定时任务绑定到错误会话

## 2026-03-20

### Changed
- updated sub-agent execution to match the new optional agent-streaming parameter without changing current tool behavior
- `shell` and `apply_patch` now resolve the fallback data workspace through `klaw-util`, removing another local copy of the default `~/.klaw/workspace` path logic

## 2026-03-19

### Changed
- `cron_manager` message shortcut now records `cron.base_session_key` for supported channel sessions so runtime cron delivery can resolve the current active session without changing persisted payload compatibility
- `shell` 与 `apply_patch` 在未配置 workspace 与 `storage.root_dir` 时，默认回退工作区统一为 `~/.klaw/workspace`，不再使用 `~/.klaw/data/workspace`

## 2026-03-18

### Changed
- split the old mixed `skills_registry` tool into two tools: read-only `skills_registry` and installed-skill `skills_manager`
- moved registry install/write actions out of `skills_registry`; `skills_manager` now owns `install_from_registry`, `uninstall`, `list_installed`, `show_installed`, and `load_all`
- updated tool metadata and schemas so registry browsing and installed-skill lifecycle are no longer mixed in one interface

## 2026-03-17

### Changed
- `shell` 与 `apply_patch` 的 workspace 回退链调整为：`metadata.workspace` -> `tools.*.workspace` -> `(<storage.root_dir 或 ~/.klaw>)/workspace`
- `shell` 与 `apply_patch` 在使用数据目录 workspace 作为回退时会自动创建该目录并 canonicalize

## 2026-03-16

### Changed
- made `cron_manager` more tolerant of planner-generated inputs: `create` can now infer `schedule_kind` from `schedule_expr`, accept 5-field cron, normalize `every 24h`, and translate daily time shorthand like `8:00` into canonical cron form
- `cron_manager` now accepts payloads as either JSON objects or JSON strings that decode to objects, and tolerates common string booleans such as `"true"` / `"false"` for `enabled`
- expanded `cron_manager` schema descriptions and validation errors with concrete accepted examples so retries converge faster after invalid input
- `cron_manager` payload validation now enforces the full `InboundMessage` shape at create/update time instead of deferring schema errors to runtime execution
- `cron_manager` now supports a `message` shortcut that builds a valid cron inbound payload from the current tool session context, so models no longer need to spell out the full payload structure for common in-chat scheduling flows
- `cron_manager` `message` shortcut now defaults to isolated cron session keys like `cron:<job_id>` instead of reusing the current interactive session, avoiding silent conversation-history growth across scheduled runs

## 2026-03-15

### Added
- added `approval` tool with persisted lifecycle actions: `request`, `get`, `resolve` (approve/reject), backed by `SessionStorage` approval records
- added session-backed shell approval requests: mutating shell commands now persist pending approval records (with `approval_id`) when a session store is available
- added metadata-based approval replay path for shell commands via `shell.approval_id` with one-time consume semantics against approved records
- added auto-consume behavior for approved shell requests by `(session_key, command_hash)` so retries can pass without explicitly carrying `shell.approval_id`
- approval request persistence now stores full command text to support post-approval immediate execution flows

### Changed
- routed approval lifecycle and shell approval consumption through the new `klaw-approval` manager layer instead of direct storage calls
- `ShellTool` now supports store injection (`with_store`, `with_config_and_store`) while preserving legacy `shell.approved=true` fallback behavior
- `sub_agent` 调用链适配新的 `AgentExecutionInput.user_media` 字段，确保子代理执行仍显式使用空媒体上下文
- `shell` 测试配置同步新增 `tools.shell.enabled` 字段，确保与配置模型一致

## 2026-03-14

### Changed
- added debug-level tool result logging in `sub_agent`'s delegated tool executor after each tool call (success/failure), with output truncation
- `skills_registry install` now indexes managed skills through `skills-registry-manifest.json` and reads installed content from local registry mirrors instead of copying into `~/.klaw/skills`
- `skills_registry uninstall` now supports mixed-mode removal and returns `removed_managed` / `removed_local` flags
- added a 15-second timeout guard around `skills_registry install` download path to avoid long-running install hangs under unstable network conditions
- redesigned `skills_registry` actions for clearer semantics: `install`, `uninstall`, `list_installed`, `show`, `load_all`
- replaced old `download/update/delete/list/get` action names in the `skills_registry` tool schema and runtime dispatch
- expanded `skills_registry` tool metadata and parameter descriptions to be more planner-friendly for LLM tool selection and argument generation
- tightened multi-action tool request schemas to action-specific `oneOf` branches (aligned with `apply_patch` style), so a single request cannot mix unrelated action arguments:
  - `skills_registry`
  - `cron_manager`
  - `terminal_multiplexer`
  - `memory`

### Added
- added `skills_registry` `search` action to query local registry mirrors (`~/.klaw/skills-registry`) by keyword against skill name and extracted `SKILL.md` description
- added action-level search controls: `query` (required) and `limit` (optional, range `1..=100`)

## 2026-03-13

### Changed
- renamed the file mutation tool from `fs` to `apply_patch`
- refactored the `apply_patch` tool to expose only batched file mutations
- tightened the `apply_patch` request schema and tool description around multi-file edit workflows
- added `tools.apply_patch` config to control absolute path access and extra allowed roots

### Fixed
- validated all `apply_patch` operations before applying changes so invalid later steps do not partially mutate earlier files
