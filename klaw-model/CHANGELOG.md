# CHANGELOG

## 2026-04-25

### Added

- Initial `klaw-model` crate for local model asset management
- File-based manifest storage under `~/.klaw/models`
- Explicit Hugging Face artifact download support with progress callbacks
- `llama.cpp` embedding/rerank/chat runtime traits and command backend skeleton
- `ModelService` facade for GUI and knowledge integration
- Hugging Face repository tree listing for full snapshot downloads from `repo_id` plus revision
- Hugging Face revision SHA tracking so upgrades can skip downloads when the local snapshot is current

### Changed

- Switched the default `llama.cpp` backend to Rust bindings via `llama-cpp-2`
- Kept the command backend as a non-default fallback path
- Updated knowledge-side local model construction to use the Rust binding backend by default
- Added local orchestrator generation and query-expansion parsing modeled after `engraph`
- Local model downloads now store files under `snapshots/{model_id}` and support cooperative cancellation with per-file progress
