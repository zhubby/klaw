# klaw-tool

`klaw-tool` contains the built-in tool implementations exposed to the agent runtime.

## Responsibilities

- define tool interfaces through the shared `Tool` trait
- implement local tools such as `apply_patch`, `shell`, `approval`, `archive`, `voice`, `memory`, `web_fetch`, `web_search`, `cron_manager`, `heartbeat_manager`, `skills_registry`, and `skills_manager`
- keep tool metadata LLM-friendly so planners can infer when and how to call each tool

## Architecture

- `src/lib.rs` exports the registry surface and common tool types
- each tool lives in its own module with request parsing, validation, execution, and local tests
- tools should keep workspace safety checks close to the operation that mutates state

## Current Notes

- the `apply_patch` tool is intentionally patch-oriented and only supports batched file mutations
- the `approval` tool delegates persisted approval lifecycle actions (`request`, `get`, `resolve`) to the `klaw-approval` manager layer
- tools can emit structured signals such as `approval_required` and `stop`; `stop` asks the agent loop to end the current turn immediately without another model round
- the `shell` tool now supports two rule lists: `blocked_patterns` reject immediately, while `unsafe_patterns` require approval; commands that match neither execute directly
- the `cron_manager` tool accepts planner-friendly schedule inputs such as 5-field cron (`0 8 * * *`), `every 24h`, and daily time shorthand (`8:00`), then normalizes them before persistence
- the `cron_manager` tool also accepts either a JSON object or a JSON string for payloads, and tolerates common boolean strings like `"true"` / `"false"` for `enabled`
- the `cron_manager` and `heartbeat_manager` tools now default `timezone` to the detected system timezone when callers omit it
- the `cron_manager` tool supports a high-level `message` shortcut for scheduled prompts in the current conversation, auto-filling channel/chat/session defaults from tool context unless explicitly overridden
- the `message` shortcut now defaults to an isolated cron session key like `cron:<job_id>` so scheduled runs do not silently accumulate the current chat's conversation history
- the `heartbeat_manager` tool manages session-bound heartbeat jobs directly from storage, with session/channel/chat defaults inferred from current tool context when possible
- sub-agent execution currently opts out of live streaming and still consumes the final aggregated agent output
- `skills_registry` now manages registry sources end to end: `add` persists `[skills.<source>]` into config, `sync` refreshes local mirrors, `delete` removes config + local clone + manifest state, and `list/show/search` browse synced registry skills
- `skills_manager` owns installed-skill lifecycle actions, including `install_from_registry`
- `local_search` uses `rg` first and falls back to BSD-compatible `grep` when ripgrep is not installed, while still honoring `include_pattern` and the default `.git` / `node_modules` exclusions
- multi-action tools use action-specific `oneOf` parameter schemas to keep requests explicit and avoid mixing unrelated fields in a single call
- the `archive` tool distinguishes between current-message attachments (`list_current_attachments`) and session-wide archived attachments (`list_session_attachments`), and now explicitly prefers `get` when the model already has an `archive_id`
- the `voice` tool exposes both `stt` and `tts`: `stt` reads archived audio by `archive_id` and returns transcript text, while `tts` synthesizes text, archives the generated audio, and returns the new archive record
- `tools.apply_patch.allow_absolute_paths = true` allows any absolute path outside the workspace
- `tools.apply_patch.allowed_roots = ["/some/path"]` allows specific extra directories while keeping the default workspace boundary elsewhere
- when `metadata.workspace` and `tools.*.workspace` are both unset, `shell` and `apply_patch` default to `(<storage.root_dir or ~/.klaw>)/workspace`
- read-only file inspection should be handled by other tools or higher-level runtime capabilities
- the `terminal_multiplexer` tool is tmux-only and always uses an isolated private socket under `${KLAW_TMUX_SOCKET_DIR:-${TMPDIR:-/tmp}/klaw-tmux-sockets}`, so listing or terminating sessions never touches the user's personal tmux server
- `terminal_multiplexer` returns structured session metadata and monitor commands, and now supports `wait_for_text` to synchronize interactive CLIs before sending the next command
- `terminal_multiplexer` now enforces a bounded auto-observation budget per user turn; once repeated `capture` / `wait_for_text` calls hit the limit, it emits a stop signal with the latest pane output so the model must summarize state back to the user before continuing
