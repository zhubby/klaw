# Changelog

All notable changes to this module will be documented in this file.

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