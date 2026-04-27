//! `klaw-core` provides the core abstractions and runtime capabilities for the agent framework.
//! This crate serves as the foundation for building reliable, observable agent systems with
//! support for message transport, session scheduling, retry policies, and telemetry.

// ============================================================================
// Module Overview
// ============================================================================

/// Agent runtime and main orchestration module.
/// Coordinates the execution flow from message ingestion through tool invocation
/// to response publishing, with support for reliability patterns and telemetry.
pub mod agent_loop;

/// Core domain message models.
/// Defines the canonical structures for inbound user messages, outbound agent responses,
/// and dead-letter messages for failed processing attempts.
pub mod domain;

/// Cross-module media reference model.
/// Provides a standardized representation for media objects (images, files, etc.)
/// that can be passed between channels, tools, and archive storage.
pub mod media;

/// Local in-memory implementations for testing and development.
/// Includes mock transport, scheduler, and idempotency store for unit/integration tests
/// without requiring external infrastructure dependencies.
pub mod mock;

/// Observability abstractions for metrics, auditing, and health checks.
/// Defines telemetry traits and data structures for recording agent execution metrics,
/// tool outcomes, model request details, and health status of components.
pub mod observability;

/// System prompt file management.
/// Handles loading, composing, and managing runtime prompts from workspace templates.
/// Supports lazy-loading of skill documentation and workspace context for efficient
/// prompt construction during agent execution.
pub mod prompt;

/// Protocol layer definitions and error codes.
/// Provides message envelope structures, schema versioning, logical topic definitions,
/// and standardized error codes for consistent error handling across the system.
pub mod protocol;

/// Reliability control abstractions.
/// Implements retry policies (exponential backoff), circuit breakers, idempotency stores,
/// and dead-letter handling for building resilient agent systems that can handle
/// transient failures gracefully.
pub mod reliability;

/// Session scheduling abstractions.
/// Defines task scheduling strategies for ensuring session-level serial execution,
/// queue management with overflow policies, and session lock mechanisms to prevent
/// concurrent processing of the same session.
pub mod scheduler;

/// Transport layer abstractions.
/// Defines the message transport trait for publishing and consuming messages
/// with configurable delivery semantics (at-least-once, at-most-once, exactly-once).
/// Supports various message queue implementations through a unified interface.
pub mod transport;

// ============================================================================
// Public Re-exports
// ============================================================================

pub use agent_loop::{
    AgentLoop, AgentRunState, AgentRuntimeError, ProcessOutcome, ProviderRuntimeSnapshot,
    QueueStrategy, RunLimits, SessionSchedulingPolicy, StateTransitionEvent,
};
pub use domain::{DeadLetterMessage, InboundMessage, OutboundMessage, SessionKey};
pub use klaw_util::WORKSPACE_DIR_NAME;
pub use media::{MediaReference, MediaSourceKind};
pub use mock::{InMemoryIdempotencyStore, InMemorySessionScheduler, InMemoryTransport};
pub use observability::{AgentTelemetry, HealthStatus};
pub use prompt::{
    PromptError, PromptExtension, PromptTemplateWriteReport, RtkPromptExtension,
    RuntimePromptInput, SkillPromptEntry, build_runtime_system_prompt,
    build_runtime_system_prompt_with_extensions, compose_runtime_prompt, default_prompt_extensions,
    ensure_workspace_prompt_templates, ensure_workspace_prompt_templates_in_dir,
    format_skills_for_prompt, format_workspace_docs_for_prompt, get_default_template_content,
    skills_lazy_load_instructions,
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
