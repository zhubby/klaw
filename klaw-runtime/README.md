# klaw-runtime

`klaw-runtime` is Klaw's host-layer composition crate.

## Responsibilities

- build and own the shared `RuntimeBundle`
- expose the channel-facing `SharedChannelRuntime`
- wire runtime submission helpers for one-shot and streaming flows
- host runtime-only IM command handling and session routing policy
- integrate background services, webhook processing, and gateway lifecycle glue
- map gateway WebSocket v1 turn metadata into structured `item/*` and `turn/*` protocol notifications for both streaming and non-streaming turns
- own the shared Knowledge service so GUI search, Knowledge tool calls, and index/vector sync reuse one provider/model runtime instead of reopening it per request
- clear the shared Knowledge service during runtime shutdown so local model resources are released before process exit
- provide Gateway lifecycle state helpers so GUI command handlers can recover from timed-out start/restart/mode operations without leaving the Gateway snapshot stuck in `transitioning`

## Notes

- This crate exists to keep `klaw-cli` focused on process startup and command parsing.
- It intentionally depends on multiple workspace crates because it is the runtime composition root.
- Lower-level crates such as `klaw-core`, `klaw-agent`, `klaw-channel`, and `klaw-gateway` should remain narrowly scoped and should not absorb this host-specific glue.
- `/approve` 恢复审批时，runtime 现在会优先从触发审批的 `tool_audit` 重放原始 tool call，并把真实工具结果作为结构化 assistant/tool 历史接回 agent；shell 与其它接入审批的工具都不再依赖 prompt 式 follow-up 或 runtime 侧强制重试。
- Assistant chat history keeps user-visible metadata such as cards, but runtime-only LLM audit, usage, and model conversation-history payloads stay in their dedicated stores instead of being duplicated into session JSONL files.
- WebUI-facing assistant history enriches and persists interaction metadata before JSONL write, so approval/question cards and `channel_attachment` resource previews survive page reloads through `thread/history`.
