# Changelog

All notable changes to this module will be documented in this file.

## 2026-03-22

### Added
- model-level local analysis storage for provider/model request metrics, token composition, model-attributed tool outcomes, and turn-efficiency outcomes
- public model analysis query types (`ModelStatsQuery`, `ModelStatsRow`, `ModelTimeseriesPoint`, `ModelDashboardSnapshot`) for GUI aggregation
- estimated per-model cost calculation in local analysis snapshots using a built-in static price table

### Changed
- `OtelAgentTelemetry` now records model requests, model-attributed tool success/failure, and turn outcomes into the shared local analysis store alongside existing tool metrics

## 2026-03-21

### Added
- local SQLite-backed analysis store for tool outcomes, with event rows and minute rollups for GUI queries
- public tool analysis query types (`ToolStatsQuery`, `ToolStatsRow`, `ToolTimeseriesPoint`, `ToolDashboardSnapshot`)
- `ObservabilityHandle.local_store` integration so runtime telemetry and GUI can share one local analysis source

### Changed
- `init_observability` now initializes the optional local analysis store and accepts an optional data-root override for colocating observability data with the rest of Klaw state

## [0.1.0] - 2025-03-19

### Added
- Initial implementation of observability module
- Metrics with OTLP and Prometheus exporters
- Distributed tracing with probability sampling
- Structured audit event logging
- Health registry for component status management
- Configuration support via `ObservabilityConfig`
- `AgentTelemetry` trait implementation via `OtelAgentTelemetry`
- Integration with `AgentLoop` for telemetry injection
- Integration with `klaw-gateway` for health and metrics endpoints
  - `/health/live` - Liveness probe
  - `/health/ready` - Readiness probe
  - `/health/status` - JSON health status
  - `/metrics` - Prometheus metrics endpoint
