# klaw-tool

`klaw-tool` contains the built-in tool implementations exposed to the agent runtime.

## Responsibilities

- define tool interfaces through the shared `Tool` trait
- implement local tools such as `apply_patch`, `shell`, `memory`, `web_fetch`, `web_search`, and `skills_registry`
- implement local tools such as `apply_patch`, `shell`, `approval`, `memory`, `web_fetch`, `web_search`, and `skills_registry`
- keep tool metadata LLM-friendly so planners can infer when and how to call each tool

## Architecture

- `src/lib.rs` exports the registry surface and common tool types
- each tool lives in its own module with request parsing, validation, execution, and local tests
- tools should keep workspace safety checks close to the operation that mutates state

## Current Notes

- the `apply_patch` tool is intentionally patch-oriented and only supports batched file mutations
- the `approval` tool delegates persisted approval lifecycle actions (`request`, `get`, `resolve`) to the `klaw-approval` manager layer
- multi-action tools use action-specific `oneOf` parameter schemas to keep requests explicit and avoid mixing unrelated fields in a single call
- `tools.apply_patch.allow_absolute_paths = true` allows any absolute path outside the workspace
- `tools.apply_patch.allowed_roots = ["/some/path"]` allows specific extra directories while keeping the default workspace boundary elsewhere
- read-only file inspection should be handled by other tools or higher-level runtime capabilities
