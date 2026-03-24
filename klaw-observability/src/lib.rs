pub mod audit;
pub mod config;
pub mod exporter;
pub mod health;
pub mod local_store;
pub mod metrics;
pub mod telemetry;
pub mod tracing_ext;

pub use audit::{AuditEvent, AuditLogger};
pub use config::{LocalStoreConfig, ObservabilityConfig};
pub use health::{HealthRegistry, HealthStatus};
pub use local_store::{
    LocalMetricsStore, LocalMetricsStoreError, ModelDashboardSnapshot, ModelErrorBreakdownRow,
    ModelLatencyPercentilesRow, ModelStatsQuery, ModelStatsRow, ModelSummaryRow,
    ModelTimeseriesPoint, ModelTokenCompositionRow, ModelToolBreakdownRow, SqliteLocalMetricsStore,
    ToolDashboardSnapshot, ToolErrorBreakdownRow, ToolMetricEvent, ToolSampleBucket,
    ToolStatsQuery, ToolStatsRow, ToolSummaryRow, ToolTimeRange, ToolTimeseriesPoint,
    TurnEfficiencyRow,
};
pub use metrics::MetricsRecorder;
pub use telemetry::{ObservabilityHandle, OtelAgentTelemetry, init_observability};

pub use klaw_core::observability::{
    AgentTelemetry, ModelRequestRecord, ModelRequestStatus, ModelToolOutcomeRecord,
    ToolOutcomeStatus, TurnOutcomeRecord,
};
