# klaw-knowledge

`klaw-knowledge` provides read-only retrieval over external knowledge sources for Klaw.

## Knowledge vs Memory

- `klaw-memory` stores agent-produced long-term facts and session recall.
- `klaw-knowledge` retrieves external knowledge owned outside the agent, such as an Obsidian vault.
- `klaw-knowledge` does not write user knowledge back into `memory.db` or the runtime `Memory` prompt section.

## Capabilities

- Provider-based knowledge retrieval via `KnowledgeProvider`
- Obsidian markdown parsing, chunking, and indexing helpers
- Structured search result types (`KnowledgeHit`, `KnowledgeEntry`, `ContextBundle`)
- Config-driven Obsidian provider construction for runtime and GUI callers
- Status and incremental sync result types for indexed entries, chunks, and embeddings
- Retrieval building blocks for hybrid search lanes and RRF fusion
