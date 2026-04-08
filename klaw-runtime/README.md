# klaw-runtime

`klaw-runtime` is Klaw's host-layer composition crate.

## Responsibilities

- build and own the shared `RuntimeBundle`
- expose the channel-facing `SharedChannelRuntime`
- wire runtime submission helpers for one-shot and streaming flows
- host runtime-only IM command handling and session routing policy
- integrate background services, webhook processing, and gateway lifecycle glue

## Notes

- This crate exists to keep `klaw-cli` focused on process startup and command parsing.
- It intentionally depends on multiple workspace crates because it is the runtime composition root.
- Lower-level crates such as `klaw-core`, `klaw-agent`, `klaw-channel`, and `klaw-gateway` should remain narrowly scoped and should not absorb this host-specific glue.