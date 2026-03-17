//! `klaw-core` 提供 agent 基座的核心抽象与运行时能力。

/// Agent 运行时与主编排模块。
pub mod agent_loop;
/// 核心领域消息模型。
pub mod domain;
/// 跨模块媒体引用模型。
pub mod media;
/// 本地/测试用 mock 实现。
pub mod mock;
/// 指标、审计、健康抽象。
pub mod observability;
/// System prompt 文件管理。
pub mod prompt;
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
pub use media::{MediaReference, MediaSourceKind};
pub use mock::{InMemoryIdempotencyStore, InMemorySessionScheduler, InMemoryTransport};
pub use observability::{AgentTelemetry, HealthStatus};
pub use prompt::{
    compose_runtime_prompt, ensure_workspace_prompt_templates,
    ensure_workspace_prompt_templates_in_dir, format_skills_for_prompt,
    load_or_create_system_prompt, load_or_create_system_prompt_in_dir,
    skills_lazy_load_instructions, PromptError, PromptTemplateWriteReport, RuntimePromptInput,
    SkillPromptEntry, WORKSPACE_DIR_NAME,
};
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
