# klaw-session

`klaw-session` provides the session-management service layer for Klaw.

## Capabilities

- Defines the `SessionManager` trait for session lifecycle workflows
- Provides `SqliteSessionManager` as the default manager backed by the workspace storage layer
- Supports session listing, lookup, route-state initialization, explicit provider/model override updates, override clearing, and chat history read/write
- Exposes append/list access for persisted `llm_audit` and `tool_audit` diagnostic records
- Normalizes session list pagination through `SessionListQuery`
- Keeps UI, CLI, and runtime callers decoupled from direct `klaw-storage` access

## Architecture

- `manager.rs`: trait, query type, and default manager implementation
- `error.rs`: session-domain error surface

The crate is intentionally narrow: it exposes session management operations while delegating persistence details to `klaw-storage`.
