# Changelog

## 2026-03-14

### Changed
- added debug-level tool result logging in `sub_agent`'s delegated tool executor after each tool call (success/failure), with output truncation
- `skills_registry install` now short-circuits with `already_installed: true` when the target skill already exists from the same configured source/template
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
