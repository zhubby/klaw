pub mod audit;
pub mod config;
pub mod exporter;
pub mod health;
pub mod metrics;
pub mod telemetry;
pub mod tracing_ext;

pub use audit::{AuditEvent, AuditLogger};
pub use config::ObservabilityConfig;
pub use health::{HealthRegistry, HealthStatus};
pub use metrics::MetricsRecorder;
pub use telemetry::{init_observability, ObservabilityHandle, OtelAgentTelemetry};

pub use klaw_core::observability::AgentTelemetry;
