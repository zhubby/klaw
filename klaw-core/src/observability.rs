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
    /// 记录模型请求结果。
    async fn record_model_request(&self, record: ModelRequestRecord);
    /// 记录按模型归因的工具调用结果。
    async fn record_model_tool_outcome(&self, record: ModelToolOutcomeRecord);
    /// 记录单轮 agent 执行结果。
    async fn record_turn_outcome(&self, record: TurnOutcomeRecord);
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
