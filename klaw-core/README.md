# klaw-core

`klaw-core` contains the runtime-facing domain model and orchestration primitives for Klaw.

## Responsibilities

- define inbound/outbound/dead-letter message models
- expose protocol envelopes, scheduling, reliability, and transport abstractions
- host shared cross-crate models such as `MediaReference`
- provide the main `AgentLoop` runtime
- route per-message provider/model selection from inbound metadata (`agent.provider_id`, `agent.model`)

## Notes

- `MediaReference` and `MediaSourceKind` are shared boundary types for channels, tools, and archive-related flows
- `klaw-core` does not persist media itself; that remains the responsibility of `klaw-archive`
