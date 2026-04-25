# klaw-model

`klaw-model` manages local model assets for Klaw and exposes local `llama.cpp` runtime interfaces.

The default local inference backend is the Rust binding from `utilityai/llama-cpp-rs`
via the `llama-cpp-2` crate. A command backend is still kept as a fallback/debug path,
but it is no longer the default.

## Responsibilities

- Manage model storage under `~/.klaw/models`
- Download explicit Hugging Face artifacts into the local model store
- Persist installed model manifests and scan installed models
- Prevent deleting models that are still bound by config
- Expose local embedding, rerank, chat, and orchestrator runtime traits

## Layout

- `catalog.rs`: Hugging Face model identifiers and normalization
- `storage.rs`: model directory layout, manifest scanning, deletion
- `download.rs`: Hugging Face artifact download with progress callbacks
- `manifest.rs`: JSON manifest persistence helpers
- `llama_cpp.rs`: local runtime traits, Rust binding backend, and command fallback
- `service.rs`: high-level facade for GUI and knowledge consumers

## Runtime Backend

`klaw-model` now defaults to a Rust binding backend:

- Global shared `llama.cpp` backend initialized once per process
- GGUF model loading via `llama-cpp-2`
- Per-request `LlamaContext` creation for embedding, rerank, chat, and orchestrator generation
- Query/document prompt formatting for embedding model families such as Qwen and embeddinggemma
- Query-expansion orchestration with JSON parsing and heuristic fallback inspired by `engraph`

### Build Requirements

Because the default backend builds `llama.cpp` through Rust bindings, the local toolchain
must provide:

- `cmake`
- `clang` / Apple Clang

Without those tools, `klaw-model` cannot compile the default inference backend.

## Current Scope

The first version focuses on local model assets and runtime interfaces only.

- Remote provider routing remains in `klaw-llm`
- Desktop GUI is the only management surface for now
- `klaw-knowledge` consumes model IDs and local runtime traits rather than raw file paths
