# klaw-approval

`klaw-approval` provides the approval-management service layer for Klaw.

## Capabilities

- Defines the `ApprovalManager` trait for approval lifecycle workflows
- Provides `SqliteApprovalManager` as the default manager backed by workspace storage
- Supports approval creation, lookup, listing, approve/reject resolution, and shell-approval consumption
- Centralizes approval validation and status-transition rules outside UI, CLI, and tool callers

## Architecture

- `manager.rs`: trait, query/input types, and default manager implementation
- `error.rs`: approval-domain error surface

This crate is intentionally narrow and keeps higher-level approval behavior decoupled from direct `klaw-storage` usage.
