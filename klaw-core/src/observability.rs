use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// 健康状态枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// 依赖就绪，可接流量。
    Ready,
    /// 进程存活。
    Live,
    /// 可用但功能受限。
    Degraded,
    /// 不可用。
    Unavailable,
}

/// 统一指标名称。
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
    /// 返回指标的标准字符串名称。
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

/// 审计事件负载。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// 事件名称。
    pub event_name: String,
    /// 追踪 ID。
    pub trace_id: Uuid,
    /// 会话键（可选）。
    pub session_key: Option<String>,
    /// 错误码（可选）。
    pub error_code: Option<String>,
    /// 扩展负载。
    pub payload: serde_json::Value,
}

/// 可观测性上报抽象。
#[async_trait]
pub trait AgentTelemetry: Send + Sync {
    /// 记录工具调用结果。
    async fn record_tool_outcome(
        &self,
        session_key: &str,
        tool_name: &str,
        status: ToolOutcomeStatus,
        error_code: Option<&str>,
        duration: Duration,
    );
    /// 增加计数器。
    async fn incr_counter(&self, name: &'static str, labels: &[(&str, &str)], value: u64);
    /// 上报直方图时延。
    async fn observe_histogram(
        &self,
        name: &'static str,
        labels: &[(&str, &str)],
        duration: Duration,
    );
    /// 记录审计事件。
    async fn emit_audit_event(
        &self,
        event_name: &'static str,
        trace_id: Uuid,
        payload: serde_json::Value,
    );
    /// 设置组件健康状态。
    async fn set_health(&self, component: &'static str, status: HealthStatus);
}
