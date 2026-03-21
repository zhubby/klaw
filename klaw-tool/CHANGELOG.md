# Changelog

## 2026-03-21

### Changed
- `local_search` 现在优先使用 `rg`，并在系统缺少 ripgrep 时回退到兼容 BSD/macOS 的 `grep`，同时保留 `include_pattern` 与默认目录排除行为

### Fixed
- `cron_manager` 现在支持注入共享 `session_store`，避免 runtime 内对同一个 `klaw.db` 重新打开独立连接并触发 SQLite `database is locked`

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
