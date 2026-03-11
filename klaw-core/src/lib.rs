//! `klaw-core` 提供 agent 基座的核心抽象与运行时能力。

/// Agent 运行时与主编排模块。
pub mod agent_loop;
/// 核心领域消息模型。
pub mod domain;
/// 本地/测试用 mock 实现。
pub mod mock;
/// 指标、审计、健康抽象。
pub mod observability;
/// 协议层与错误码定义。
pub mod protocol;
/// 可靠性控制抽象。
pub mod reliability;
/// 会话调度抽象。
pub mod scheduler;
/// 传输层抽象。
pub mod transport;

pub use agent_loop::{
    AgentLoop, AgentRunState, AgentRuntimeError, ProcessOutcome, QueueStrategy, RunLimits,
    SessionSchedulingPolicy, StateTransitionEvent,
};
pub use domain::{DeadLetterMessage, InboundMessage, OutboundMessage, SessionKey};
pub use mock::{InMemoryIdempotencyStore, InMemorySessionScheduler, InMemoryTransport};
pub use observability::{AgentTelemetry, HealthStatus};
pub use protocol::{Envelope, EnvelopeHeader, ErrorCode, MessageTopic, SchemaVersion};
pub use reliability::{
    CircuitBreaker, CircuitBreakerPolicy, DeadLetterPolicy, ExponentialBackoffRetryPolicy,
    IdempotencyStore, InMemoryCircuitBreaker, RetryDecision, RetryPolicy,
};
pub use scheduler::{QueueOverflowPolicy, SessionScheduler, SessionTask, TaskScheduleDecision};
pub use transport::{
    DeliveryMode, MessageTransport, Subscription, TransportAckHandle, TransportError,
    TransportMessage,
};
