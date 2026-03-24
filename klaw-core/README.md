# klaw-core

`klaw-core` contains the runtime-facing domain model and orchestration primitives for Klaw.

## Responsibilities

- define inbound/outbound/dead-letter message models
- include `InboundMessage.media_references` for media-aware message turns
- expose protocol envelopes, scheduling, reliability, and transport abstractions
- host shared cross-crate models such as `MediaReference`
- provide the main `AgentLoop` runtime
- expose both final-response and streaming-snapshot execution paths for channel-facing runtimes
- route per-message provider/model selection from inbound metadata (`agent.provider_id`, `agent.model`)
- emit model-request, model-attributed tool, and turn-level observability records through `AgentTelemetry`
- support runtime system-prompt hot reload through `AgentLoop::set_system_prompt`
- backfill standard workspace prompt templates under `~/.klaw/workspace` on demand, while creating `BOOTSTRAP.md` only on first initialization
- compose runtime prompts by inlining core workspace docs (`AGENTS.md`, `SOUL.md`, `IDENTITY.md`, `TOOLS.md`) ahead of the runtime sections, while keeping remaining workspace docs and skills lazy-loaded

## Notes

- `MediaReference` and `MediaSourceKind` are shared boundary types for channels, tools, and archive-related flows
- `klaw-core` does not persist media itself; that remains the responsibility of `klaw-archive`
- `AgentLoop` annotates archived attachments into the current user turn so the model can see archive ids, relative archive paths, and the read-only/copy-to-workspace workflow, while explicitly distinguishing current-message attachments from earlier session files
- `AgentLoop` treats `approval_required` tool outcomes as approval handoff states for user-facing messaging, instead of wrapping them as generic tool failures
