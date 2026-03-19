use opentelemetry::{
    global,
    metrics::{Counter, Gauge, Histogram, Meter},
    KeyValue,
};
use prometheus::Registry;
use std::sync::Arc;
use std::time::Duration;

pub const METRIC_INBOUND_CONSUMED_TOTAL: &str = "agent_inbound_consumed_total";
pub const METRIC_OUTBOUND_PUBLISHED_TOTAL: &str = "agent_outbound_published_total";
pub const METRIC_RUN_DURATION_MS: &str = "agent_run_duration_ms";
pub const METRIC_TOOL_SUCCESS_TOTAL: &str = "agent_tool_success_total";
pub const METRIC_TOOL_FAILURE_TOTAL: &str = "agent_tool_failure_total";
pub const METRIC_RETRY_TOTAL: &str = "agent_retry_total";
pub const METRIC_DEADLETTER_TOTAL: &str = "agent_deadletter_total";
pub const METRIC_SESSION_QUEUE_DEPTH: &str = "agent_session_queue_depth";

pub const LABEL_SESSION_KEY: &str = "session_key";
pub const LABEL_PROVIDER: &str = "provider";
pub const LABEL_TOOL_NAME: &str = "tool_name";
pub const LABEL_ERROR_CODE: &str = "error_code";
pub const LABEL_STAGE: &str = "stage";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricName {
    InboundConsumedTotal,
    OutboundPublishedTotal,
    RunDurationMs,
    ToolSuccessTotal,
    ToolFailureTotal,
    RetryTotal,
    DeadLetterTotal,
    SessionQueueDepth,
}

impl MetricName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InboundConsumedTotal => METRIC_INBOUND_CONSUMED_TOTAL,
            Self::OutboundPublishedTotal => METRIC_OUTBOUND_PUBLISHED_TOTAL,
            Self::RunDurationMs => METRIC_RUN_DURATION_MS,
            Self::ToolSuccessTotal => METRIC_TOOL_SUCCESS_TOTAL,
            Self::ToolFailureTotal => METRIC_TOOL_FAILURE_TOTAL,
            Self::RetryTotal => METRIC_RETRY_TOTAL,
            Self::DeadLetterTotal => METRIC_DEADLETTER_TOTAL,
            Self::SessionQueueDepth => METRIC_SESSION_QUEUE_DEPTH,
        }
    }
}

pub struct MetricsRecorder {
    meter: Meter,
    registry: Arc<Registry>,
    inbound_counter: Counter<u64>,
    outbound_counter: Counter<u64>,
    duration_histogram: Histogram<f64>,
    tool_success_counter: Counter<u64>,
    tool_failure_counter: Counter<u64>,
    retry_counter: Counter<u64>,
    deadletter_counter: Counter<u64>,
    queue_depth_gauge: Gauge<i64>,
}

impl MetricsRecorder {
    pub fn new(service_name: impl Into<String>, registry: Arc<Registry>) -> Self {
        let service_name: String = service_name.into();
        let service_name_static: &'static str = Box::leak(service_name.into_boxed_str());
        let meter = global::meter(service_name_static);

        let inbound_counter = meter
            .u64_counter(METRIC_INBOUND_CONSUMED_TOTAL)
            .with_description("Total number of inbound messages consumed")
            .build();
        let outbound_counter = meter
            .u64_counter(METRIC_OUTBOUND_PUBLISHED_TOTAL)
            .with_description("Total number of outbound messages published")
            .build();
        let duration_histogram = meter
            .f64_histogram(METRIC_RUN_DURATION_MS)
            .with_description("Duration of agent run in milliseconds")
            .build();
        let tool_success_counter = meter
            .u64_counter(METRIC_TOOL_SUCCESS_TOTAL)
            .with_description("Total number of successful tool invocations")
            .build();
        let tool_failure_counter = meter
            .u64_counter(METRIC_TOOL_FAILURE_TOTAL)
            .with_description("Total number of failed tool invocations")
            .build();
        let retry_counter = meter
            .u64_counter(METRIC_RETRY_TOTAL)
            .with_description("Total number of retry attempts")
            .build();
        let deadletter_counter = meter
            .u64_counter(METRIC_DEADLETTER_TOTAL)
            .with_description("Total number of messages sent to dead letter queue")
            .build();
        let queue_depth_gauge = meter
            .i64_gauge(METRIC_SESSION_QUEUE_DEPTH)
            .with_description("Current session queue depth")
            .build();

        Self {
            meter,
            registry,
            inbound_counter,
            outbound_counter,
            duration_histogram,
            tool_success_counter,
            tool_failure_counter,
            retry_counter,
            deadletter_counter,
            queue_depth_gauge,
        }
    }

    pub fn registry(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }

    pub fn meter(&self) -> &Meter {
        &self.meter
    }

    pub fn incr_inbound(&self, session_key: &str, provider: &str) {
        let labels = [
            KeyValue::new(LABEL_SESSION_KEY, session_key.to_string()),
            KeyValue::new(LABEL_PROVIDER, provider.to_string()),
        ];
        self.inbound_counter.add(1, &labels);
    }

    pub fn incr_outbound(&self, session_key: &str, provider: &str) {
        let labels = [
            KeyValue::new(LABEL_SESSION_KEY, session_key.to_string()),
            KeyValue::new(LABEL_PROVIDER, provider.to_string()),
        ];
        self.outbound_counter.add(1, &labels);
    }

    pub fn record_duration(&self, session_key: &str, stage: &str, duration: Duration) {
        let labels = [
            KeyValue::new(LABEL_SESSION_KEY, session_key.to_string()),
            KeyValue::new(LABEL_STAGE, stage.to_string()),
        ];
        self.duration_histogram
            .record(duration.as_secs_f64() * 1000.0, &labels);
    }

    pub fn incr_tool_success(&self, session_key: &str, tool_name: &str) {
        let labels = [
            KeyValue::new(LABEL_SESSION_KEY, session_key.to_string()),
            KeyValue::new(LABEL_TOOL_NAME, tool_name.to_string()),
        ];
        self.tool_success_counter.add(1, &labels);
    }

    pub fn incr_tool_failure(&self, session_key: &str, tool_name: &str, error_code: &str) {
        let labels = [
            KeyValue::new(LABEL_SESSION_KEY, session_key.to_string()),
            KeyValue::new(LABEL_TOOL_NAME, tool_name.to_string()),
            KeyValue::new(LABEL_ERROR_CODE, error_code.to_string()),
        ];
        self.tool_failure_counter.add(1, &labels);
    }

    pub fn incr_retry(&self, session_key: &str, error_code: &str) {
        let labels = [
            KeyValue::new(LABEL_SESSION_KEY, session_key.to_string()),
            KeyValue::new(LABEL_ERROR_CODE, error_code.to_string()),
        ];
        self.retry_counter.add(1, &labels);
    }

    pub fn incr_deadletter(&self, session_key: &str) {
        let labels = [KeyValue::new(LABEL_SESSION_KEY, session_key.to_string())];
        self.deadletter_counter.add(1, &labels);
    }

    pub fn set_queue_depth(&self, session_key: &str, depth: i64) {
        let labels = [KeyValue::new(LABEL_SESSION_KEY, session_key.to_string())];
        self.queue_depth_gauge.record(depth, &labels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_name_as_str_matches_constants() {
        assert_eq!(
            MetricName::InboundConsumedTotal.as_str(),
            METRIC_INBOUND_CONSUMED_TOTAL
        );
        assert_eq!(
            MetricName::OutboundPublishedTotal.as_str(),
            METRIC_OUTBOUND_PUBLISHED_TOTAL
        );
        assert_eq!(MetricName::RunDurationMs.as_str(), METRIC_RUN_DURATION_MS);
        assert_eq!(
            MetricName::ToolSuccessTotal.as_str(),
            METRIC_TOOL_SUCCESS_TOTAL
        );
        assert_eq!(
            MetricName::ToolFailureTotal.as_str(),
            METRIC_TOOL_FAILURE_TOTAL
        );
        assert_eq!(MetricName::RetryTotal.as_str(), METRIC_RETRY_TOTAL);
        assert_eq!(
            MetricName::DeadLetterTotal.as_str(),
            METRIC_DEADLETTER_TOTAL
        );
        assert_eq!(
            MetricName::SessionQueueDepth.as_str(),
            METRIC_SESSION_QUEUE_DEPTH
        );
    }
}
