# klaw-knowledge

`klaw-knowledge` provides retrieval-first access to external knowledge sources for Klaw, including constrained write-back for Obsidian notes.

## Knowledge vs Memory

- `klaw-memory` stores agent-produced long-term facts and session recall.
- `klaw-knowledge` retrieves external knowledge owned outside the agent, such as an Obsidian vault.
- `klaw-knowledge` can explicitly create new Obsidian notes in the configured vault, but it remains separate from agent memory storage.
- `klaw-knowledge` does not write user knowledge back into `memory.db` or the runtime `Memory` prompt section.

## Capabilities

- Provider-based knowledge retrieval via `KnowledgeProvider`
- Constrained note creation for Obsidian vaults with immediate single-note reindexing
- Obsidian markdown parsing, scored semantic chunking, and indexing helpers
- Obsidian link discovery for explicit wikilinks plus exact name, alias, fuzzy Levenshtein, and unique first-name matches without rewriting vault files
- Structured search result types (`KnowledgeHit`, `KnowledgeEntry`, `ContextBundle`)
- Config-driven Obsidian provider construction for runtime and GUI callers
- Local llama.cpp knowledge model construction that honors per-model manifest GGUF defaults
- Status and incremental sync result types for indexed entries, chunks, and embeddings
- Sync progress events for indexing and embedding phases, including processed counts and current file/chunk labels
- Auto-index watcher support for updating an already indexed Obsidian vault after Markdown file changes
- Runtime snapshot/state types for host-owned Knowledge service readiness reporting
- Retrieval building blocks for hybrid search lanes and RRF fusion
- Turso/libSQL native vector storage for embedded chunks, with `vector_top_k` when a vector index is available and SQL distance ranking before any in-process fallback
