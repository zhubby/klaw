use crate::audit::{AuditEvent, AuditLogger};
use crate::config::ObservabilityConfig;
use crate::exporter::{OtlpExporter, PrometheusExporter};
use crate::health::{HealthRegistry, HealthStatus};
use crate::metrics::MetricsRecorder;
use crate::tracing_ext;
use async_trait::async_trait;
use klaw_core::observability::AgentTelemetry;
use prometheus::Registry;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::Span;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ObservabilityError {
    #[error("failed to create OTLP exporter: {0}")]
    OtlpExporter(#[from] crate::exporter::otlp::OtlpExporterError),
    #[error("failed to create Prometheus exporter: {0}")]
    PrometheusExporter(#[from] crate::exporter::prometheus::PrometheusExporterError),
    #[error("observability is disabled")]
    Disabled,
}

pub struct ObservabilityHandle {
    pub metrics: Arc<MetricsRecorder>,
    pub health: Arc<HealthRegistry>,
    pub audit: Option<Arc<AuditLogger>>,
    pub prometheus: Option<PrometheusExporter>,
    pub otlp: Option<OtlpExporter>,
    pub registry: Registry,
}

impl ObservabilityHandle {
    pub fn metrics(&self) -> Arc<MetricsRecorder> {
        Arc::clone(&self.metrics)
    }

    pub fn health(&self) -> Arc<HealthRegistry> {
        Arc::clone(&self.health)
    }

    pub fn audit(&self) -> Option<Arc<AuditLogger>> {
        self.audit.clone()
    }

    pub fn prometheus_exporter(&self) -> Option<&PrometheusExporter> {
        self.prometheus.as_ref()
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn shutdown(&self) {
        if let Some(otlp) = &self.otlp {
            otlp.shutdown();
        }
        tracing_ext::shutdown_tracing();
    }
}

pub fn init_observability(
    config: &ObservabilityConfig,
) -> Result<ObservabilityHandle, ObservabilityError> {
    if !config.enabled {
        return Err(ObservabilityError::Disabled);
    }

    let registry = Registry::new();
    let health = Arc::new(HealthRegistry::new());

    health.register("provider");
    health.register("transport");
    health.register("store");

    let metrics = Arc::new(MetricsRecorder::new(
        &config.service_name,
        Arc::new(registry.clone()),
    ));

    let mut prometheus = None;
    let mut otlp = None;

    if config.prometheus.enabled {
        let mut exporter = PrometheusExporter::with_registry(registry.clone());
        exporter.install(&config.service_name)?;
        prometheus = Some(exporter);
    }

    if config.otlp.enabled {
        let exporter = OtlpExporter::new(
            &config.otlp.endpoint,
            &config.otlp.headers,
            config.traces.sample_rate,
            config.metrics.export_interval_seconds,
        )?;
        otlp = Some(exporter);
    }

    let audit = if config.audit.enabled {
        let output_path = config.audit.output_path.as_ref().map(PathBuf::from);
        Some(Arc::new(AuditLogger::new(output_path, 1000)))
    } else {
        None
    };

    tracing_ext::init_tracing();

    Ok(ObservabilityHandle {
        metrics,
        health,
        audit,
        prometheus,
        otlp,
        registry,
    })
}

pub struct OtelAgentTelemetry {
    metrics: Arc<MetricsRecorder>,
    health: Arc<HealthRegistry>,
    audit: Option<Arc<AuditLogger>>,
    service_name: String,
}

impl OtelAgentTelemetry {
    pub fn new(
        metrics: Arc<MetricsRecorder>,
        health: Arc<HealthRegistry>,
        audit: Option<Arc<AuditLogger>>,
        service_name: impl Into<String>,
    ) -> Self {
        Self {
            metrics,
            health,
            audit,
            service_name: service_name.into(),
        }
    }

    pub fn from_handle(handle: &ObservabilityHandle, service_name: impl Into<String>) -> Self {
        Self::new(
            handle.metrics(),
            handle.health(),
            handle.audit(),
            service_name,
        )
    }

    pub fn create_span(&self, name: &str) -> Span {
        tracing::info_span!("agent_run", service = %self.service_name, span_name = name)
    }
}

fn map_health_status(status: klaw_core::HealthStatus) -> HealthStatus {
    match status {
        klaw_core::HealthStatus::Ready => HealthStatus::Ready,
        klaw_core::HealthStatus::Live => HealthStatus::Live,
        klaw_core::HealthStatus::Degraded => HealthStatus::Degraded,
        klaw_core::HealthStatus::Unavailable => HealthStatus::Unavailable,
    }
}

#[async_trait]
impl AgentTelemetry for OtelAgentTelemetry {
    async fn incr_counter(&self, name: &'static str, labels: &[(&str, &str)], _value: u64) {
        match name {
            crate::metrics::METRIC_INBOUND_CONSUMED_TOTAL => {
                let session_key = labels
                    .iter()
                    .find(|(k, _)| *k == "session_key")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                let provider = labels
                    .iter()
                    .find(|(k, _)| *k == "provider")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                self.metrics.incr_inbound(session_key, provider);
            }
            crate::metrics::METRIC_OUTBOUND_PUBLISHED_TOTAL => {
                let session_key = labels
                    .iter()
                    .find(|(k, _)| *k == "session_key")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                let provider = labels
                    .iter()
                    .find(|(k, _)| *k == "provider")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                self.metrics.incr_outbound(session_key, provider);
            }
            crate::metrics::METRIC_TOOL_SUCCESS_TOTAL => {
                let session_key = labels
                    .iter()
                    .find(|(k, _)| *k == "session_key")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                let tool_name = labels
                    .iter()
                    .find(|(k, _)| *k == "tool_name")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                self.metrics.incr_tool_success(session_key, tool_name);
            }
            crate::metrics::METRIC_TOOL_FAILURE_TOTAL => {
                let session_key = labels
                    .iter()
                    .find(|(k, _)| *k == "session_key")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                let tool_name = labels
                    .iter()
                    .find(|(k, _)| *k == "tool_name")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                let error_code = labels
                    .iter()
                    .find(|(k, _)| *k == "error_code")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                self.metrics
                    .incr_tool_failure(session_key, tool_name, error_code);
            }
            crate::metrics::METRIC_RETRY_TOTAL => {
                let session_key = labels
                    .iter()
                    .find(|(k, _)| *k == "session_key")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                let error_code = labels
                    .iter()
                    .find(|(k, _)| *k == "error_code")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                self.metrics.incr_retry(session_key, error_code);
            }
            crate::metrics::METRIC_DEADLETTER_TOTAL => {
                let session_key = labels
                    .iter()
                    .find(|(k, _)| *k == "session_key")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                self.metrics.incr_deadletter(session_key);
            }
            _ => {
                tracing::debug!(metric_name = name, "unknown counter metric");
            }
        }
    }

    async fn observe_histogram(
        &self,
        name: &'static str,
        labels: &[(&str, &str)],
        duration: Duration,
    ) {
        match name {
            crate::metrics::METRIC_RUN_DURATION_MS => {
                let session_key = labels
                    .iter()
                    .find(|(k, _)| *k == "session_key")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                let stage = labels
                    .iter()
                    .find(|(k, _)| *k == "stage")
                    .map(|(_, v)| *v)
                    .unwrap_or("");
                self.metrics.record_duration(session_key, stage, duration);
            }
            _ => {
                tracing::debug!(metric_name = name, "unknown histogram metric");
            }
        }
    }

    async fn emit_audit_event(
        &self,
        event_name: &'static str,
        trace_id: Uuid,
        payload: serde_json::Value,
    ) {
        if let Some(audit) = &self.audit {
            let session_key = payload
                .get("session_key")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let error_code = payload
                .get("error_code")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);

            let event = AuditEvent::new(event_name, trace_id, session_key, error_code, payload);
            audit.emit(event);
        }
    }

    async fn set_health(&self, component: &'static str, status: klaw_core::HealthStatus) {
        self.health.set_status(component, map_health_status(status));
    }
}
