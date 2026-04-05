use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Health status values reported by runtime components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Dependencies are ready and the component can serve traffic.
    Ready,
    /// The process is alive.
    Live,
    /// The component is available but running in a degraded mode.
    Degraded,
    /// The component is unavailable.
    Unavailable,
}

/// Canonical metric names emitted by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricName {
    InboundConsumedTotal,
    OutboundPublishedTotal,
    AgentRunDurationMs,
    ToolSuccessTotal,
    ToolFailureTotal,
    RetryTotal,
    DeadLetterTotal,
    SessionQueueDepth,
}

impl MetricName {
    /// Returns the stable metric name string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InboundConsumedTotal => "agent_inbound_consumed_total",
            Self::OutboundPublishedTotal => "agent_outbound_published_total",
            Self::AgentRunDurationMs => "agent_run_duration_ms",
            Self::ToolSuccessTotal => "agent_tool_success_total",
            Self::ToolFailureTotal => "agent_tool_failure_total",
            Self::RetryTotal => "agent_retry_total",
            Self::DeadLetterTotal => "agent_deadletter_total",
            Self::SessionQueueDepth => "agent_session_queue_depth",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolOutcomeStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelRequestStatus {
    Success,
    Failure,
}

impl ModelRequestStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequestRecord {
    pub session_key: String,
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub status: ModelRequestStatus,
    pub error_code: Option<String>,
    pub duration: Duration,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub reasoning_tokens: u64,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub tool_call_count: u32,
    pub has_tool_call: bool,
    pub empty_response: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelToolOutcomeRecord {
    pub session_key: String,
    pub provider: String,
    pub model: String,
    pub tool_name: String,
    pub status: ToolOutcomeStatus,
    pub error_code: Option<String>,
    pub duration: Duration,
    pub approval_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutcomeRecord {
    pub session_key: String,
    pub provider: String,
    pub model: String,
    pub requests_in_turn: u32,
    pub tool_iterations: u32,
    pub completed: bool,
    pub degraded: bool,
    pub token_budget_exceeded: bool,
    pub tool_loop_exhausted: bool,
}

/// Structured payload recorded for audit events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Event name.
    pub event_name: String,
    /// Trace identifier.
    pub trace_id: Uuid,
    /// Session key, when available.
    pub session_key: Option<String>,
    /// Error code, when available.
    pub error_code: Option<String>,
    /// Additional event payload.
    pub payload: serde_json::Value,
}

/// Observability reporting abstraction used by the runtime.
#[async_trait]
pub trait AgentTelemetry: Send + Sync {
    /// Records the outcome of a tool invocation.
    async fn record_tool_outcome(
        &self,
        session_key: &str,
        tool_name: &str,
        status: ToolOutcomeStatus,
        error_code: Option<&str>,
        duration: Duration,
    );
    /// Records an LLM request result.
    async fn record_model_request(&self, record: ModelRequestRecord);
    /// Records a tool result attributed to a specific provider/model pair.
    async fn record_model_tool_outcome(&self, record: ModelToolOutcomeRecord);
    /// Records the outcome of a single agent turn.
    async fn record_turn_outcome(&self, record: TurnOutcomeRecord);
    /// Increments a counter metric.
    async fn incr_counter(&self, name: &'static str, labels: &[(&str, &str)], value: u64);
    /// Observes a duration in a histogram metric.
    async fn observe_histogram(
        &self,
        name: &'static str,
        labels: &[(&str, &str)],
        duration: Duration,
    );
    /// Emits an audit event.
    async fn emit_audit_event(
        &self,
        event_name: &'static str,
        trace_id: Uuid,
        payload: serde_json::Value,
    );
    /// Updates component health state.
    async fn set_health(&self, component: &'static str, status: HealthStatus);
}
