# Changelog

## 2026-03-15

### Added
- added `approval` tool with persisted lifecycle actions: `request`, `get`, `resolve` (approve/reject), backed by `SessionStorage` approval records
- added session-backed shell approval requests: mutating shell commands now persist pending approval records (with `approval_id`) when a session store is available
- added metadata-based approval replay path for shell commands via `shell.approval_id` with one-time consume semantics against approved records
- added auto-consume behavior for approved shell requests by `(session_key, command_hash)` so retries can pass without explicitly carrying `shell.approval_id`
- approval request persistence now stores full command text to support post-approval immediate execution flows

### Changed
- `ShellTool` now supports store injection (`with_store`, `with_config_and_store`) while preserving legacy `shell.approved=true` fallback behavior
- `sub_agent` 调用链适配新的 `AgentExecutionInput.user_media` 字段，确保子代理执行仍显式使用空媒体上下文

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
