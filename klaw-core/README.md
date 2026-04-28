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
- keep `AgentLoop` focused on envelope/runtime orchestration while delegating the inner model/tool turn execution to `klaw-agent::run_agent_execution`
- emit model-request, model-attributed tool, and turn-level observability records through `AgentTelemetry`
- surface per-turn `tool_audits` and `llm_audits` in runtime outcomes for persistence/UI inspection
- support runtime system-prompt hot reload through `AgentLoop::set_system_prompt`
- backfill standard workspace prompt templates under `~/.klaw/workspace` on demand, while creating `BOOTSTRAP.md` only on first initialization
- compose runtime prompts by inlining core workspace docs (`AGENTS.md`, `SOUL.md`, `IDENTITY.md`, `TOOLS.md`) ahead of the runtime sections, while keeping remaining workspace docs and skills lazy-loaded
- inject the RTK prompt extension when `rtk` is available, while leaving command approval and blocked-command enforcement in the shell tool

## Notes

- `MediaReference` and `MediaSourceKind` are shared boundary types for channels, tools, and archive-related flows
- `klaw-core` does not persist media itself; that remains the responsibility of `klaw-archive`
- `AgentRunState` is now an honest outer lifecycle (`Received` -> `Validating` -> `Executing` -> `Publishing` -> terminal state) rather than a synthetic mirror of each inner tool-loop step
- `AgentLoop` annotates archived attachments into the current user turn so the model can see archive ids, relative archive paths, and the read-only/copy-to-workspace workflow, while explicitly distinguishing current-message attachments from earlier session files
- when archived files should be sent back into chat, prompt guidance now steers the model toward the `channel_attachment` tool rather than text-only claims that a file was sent
- `AgentLoop` treats `approval_required` tool outcomes as approval handoff states for user-facing messaging, instead of wrapping them as generic tool failures
- `AgentLoop` treats tool `stop` signals as successful turn short-circuits and forwards `turn.stopped` / `turn.stop_signal` metadata to outbound messages
- outbound metadata now also carries `turn.disposition` for approval/stopped short-circuits so callers can distinguish normal final messages from execution handoff states
- `AgentLoop` can also honor per-turn execution limit overrides from inbound metadata (`agent.max_tool_iterations`, `agent.max_tool_calls`, `agent.token_budget`) when a runtime needs to allow a narrowly bounded retry loop without relaxing the global runtime limits
