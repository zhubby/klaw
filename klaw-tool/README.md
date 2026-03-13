# klaw-tool

`klaw-tool` contains the built-in tool implementations exposed to the agent runtime.

## Responsibilities

- define tool interfaces through the shared `Tool` trait
- implement local tools such as `fs`, `shell`, `memory`, `web_fetch`, and `web_search`
- keep tool metadata LLM-friendly so planners can infer when and how to call each tool

## Architecture

- `src/lib.rs` exports the registry surface and common tool types
- each tool lives in its own module with request parsing, validation, execution, and local tests
- tools should keep workspace safety checks close to the operation that mutates state

## Current Notes

- the `apply_patch` tool is intentionally patch-oriented and only supports batched file mutations
- read-only file inspection should be handled by other tools or higher-level runtime capabilities
