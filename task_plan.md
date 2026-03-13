# Task Plan: Add klaw-archive

## Goal
Implement a new `klaw-archive` crate for media archiving with file persistence, SQLite indexing, media sniffing, and shared interfaces for future channel/tool integrations.

## Phases
- [x] Phase 1: Explore current storage, channel, and workspace structure
- [x] Phase 2: Add storage/archive crate foundations
- [x] Phase 3: Add media abstractions and integration hooks
- [x] Phase 4: Add tests, docs, and verification

## Key Questions
1. How should archive DB access fit into `klaw-storage` without adding domain leakage?
2. Where should shared media reference types live to avoid crate cycles?

## Decisions Made
- Use `klaw-storage` only for paths and generic archive DB access; keep archive schema and behavior inside `klaw-archive`.
- Put shared media reference types in `klaw-core` and map them into `klaw-archive` types.
- Deduplicate by content hash for physical files while preserving one archive record per ingest event.

## Errors Encountered
- `klaw-memory/README.md` is currently missing despite repo guidelines; new/modified crates will include README and CHANGELOG files.

## Status
**Completed** - `klaw-archive`, storage/archive DB support, media references, tests, and docs are all implemented and verified.
