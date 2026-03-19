use opentelemetry::global;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct TracingConfig {
    pub service_name: String,
    pub sample_rate: f64,
    pub enabled: bool,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            service_name: "klaw".to_string(),
            sample_rate: 0.1,
            enabled: true,
        }
    }
}

pub fn init_tracing() {
    global::set_text_map_propagator(TraceContextPropagator::new());
}

#[must_use]
pub struct TracingGuard {
    _service_name: String,
}

impl TracingGuard {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            _service_name: service_name.into(),
        }
    }
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        shutdown_tracing();
    }
}

pub fn shutdown_tracing() {
    let _ = global::shutdown_tracer_provider();
}

pub fn record_span_duration(span: &tracing::Span, duration: Duration) {
    span.record("duration_ms", duration.as_millis() as u64);
}

#[macro_export]
macro_rules! create_span {
    ($name:expr) => {
        tracing::info_span!($name)
    };
    ($name:expr, $($field:tt)*) => {
        tracing::info_span!($name, $($field)*)
    };
}
