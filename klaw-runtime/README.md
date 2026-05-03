# klaw-runtime

`klaw-runtime` is Klaw's host-layer composition crate.

## Responsibilities

- build and own the shared `RuntimeBundle`
- expose the channel-facing `SharedChannelRuntime`
- wire runtime submission helpers for one-shot and streaming flows
- host runtime-only IM command handling and session routing policy
- integrate background services, webhook processing, and gateway lifecycle glue
- map gateway WebSocket v1 turn metadata into structured `item/*` and `turn/*` protocol notifications while preserving legacy `session.stream.*` frames
- own the shared Knowledge service so GUI search, Knowledge tool calls, and index/vector sync reuse one provider/model runtime instead of reopening it per request
- clear the shared Knowledge service during runtime shutdown so local model resources are released before process exit

## Notes

- This crate exists to keep `klaw-cli` focused on process startup and command parsing.
- It intentionally depends on multiple workspace crates because it is the runtime composition root.
- Lower-level crates such as `klaw-core`, `klaw-agent`, `klaw-channel`, and `klaw-gateway` should remain narrowly scoped and should not absorb this host-specific glue.
- `/approve` 恢复审批时，runtime 现在会优先从触发审批的 `tool_audit` 重放原始 tool call，并把真实工具结果作为结构化 assistant/tool 历史接回 agent；shell 与其它接入审批的工具都不再依赖 prompt 式 follow-up 或 runtime 侧强制重试。