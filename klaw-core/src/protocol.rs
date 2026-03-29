use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::SystemTime};
use uuid::Uuid;

/// Schema version identifier for message envelope compatibility tracking.
/// Used to ensure backward compatibility between different protocol versions
/// and to validate message structure evolution over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// Major version number - incremented for breaking changes that are not backward compatible.
    /// Changes in major version may require migration or version-specific handling.
    pub major: u16,
    /// Minor version number - incremented for backward-compatible additions.
    /// New minor versions should not break existing message parsing logic.
    pub minor: u16,
}

impl SchemaVersion {
    /// Current default protocol version used for newly created messages.
    /// This version represents the stable, production-ready protocol format.
    pub const V1_0: Self = Self { major: 1, minor: 0 };
}

/// Logical topic types for message routing independent of specific message queue implementations.
/// Provides a generic abstraction over message broker topics, allowing the same code
/// to work with different MQ systems (Redis, Kafka, SQS, etc.) through configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageTopic {
    /// Inbound message topic - receives messages from external sources (users, webhooks, etc.)
    Inbound,
    /// Outbound message topic - final responses sent back to users or downstream systems
    Outbound,
    /// Events topic - intermediate events for streaming, progress updates, and state changes
    Events,
    /// Dead letter topic - messages that failed processing after all retry attempts
    DeadLetter,
}

impl MessageTopic {
    /// Returns the canonical topic name string for this message topic type.
    /// These names are used as the actual queue/topic identifiers in message brokers.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inbound => "agent.inbound",
            Self::Outbound => "agent.outbound",
            Self::Events => "agent.events",
            Self::DeadLetter => "agent.dlq",
        }
    }
}

/// Envelope header containing routing, tracing, and retry semantics.
/// This header accompanies every message through the system and provides essential
/// metadata for processing, correlation, and observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeHeader {
    /// Globally unique message identifier for deduplication and correlation.
    /// Generated fresh for each new message entry into the system.
    pub message_id: Uuid,
    /// End-to-end trace identifier for distributed tracing across services.
    /// Allows correlating related messages and tracking request flow.
    pub trace_id: Uuid,
    /// Session serialization key for ensuring sequential processing.
    /// Messages with the same session_key are processed one at a time.
    pub session_key: String,
    /// Timestamp when the message was originally created/queued.
    /// Used for TTL calculations and temporal ordering.
    pub timestamp: SystemTime,
    /// Current retry attempt number (starts from 1).
    /// Incremented on each retry to implement exponential backoff and limit retry attempts.
    pub attempt: u32,
    /// Schema version of the envelope structure.
    /// Enables version-specific parsing and backward compatibility handling.
    pub schema_version: SchemaVersion,
    /// Multi-tenant identifier for isolation in multi-tenant deployments.
    /// When present, ensures message processing respects tenant boundaries.
    pub tenant_id: Option<String>,
    /// Logical namespace for additional routing and isolation beyond tenant_id.
    /// Useful for separating different environments or service domains.
    pub namespace: Option<String>,
    /// Message priority level for queue scheduling.
    /// Higher values indicate higher priority; None uses default priority.
    pub priority: Option<u8>,
    /// Time-to-live in milliseconds for message expiration.
    /// Messages exceeding their TTL are automatically moved to dead letter.
    pub ttl_ms: Option<u64>,
    /// Extension field for provider-specific routing hints.
    /// Allows passing additional routing metadata to transport layer or providers.
    pub routing_hints: BTreeMap<String, serde_json::Value>,
}

impl EnvelopeHeader {
    /// Creates a new envelope header with default values.
    /// Initializes all fields with sensible defaults including fresh UUIDs for
    /// message_id and timestamp, and attempt starting at 1.
    pub fn new(session_key: impl Into<String>) -> Self {
        Self {
            message_id: Uuid::new_v4(),
            trace_id: Uuid::new_v4(),
            session_key: session_key.into(),
            timestamp: SystemTime::now(),
            attempt: 1,
            schema_version: SchemaVersion::V1_0,
            tenant_id: None,
            namespace: None,
            priority: None,
            ttl_ms: None,
            routing_hints: BTreeMap::new(),
        }
    }
}

/// Generic message wrapper structure combining header, metadata, and payload.
/// The envelope pattern provides a consistent structure for all messages passing
/// through the agent system, enabling uniform handling regardless of message type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// Protocol header containing routing, tracing, and delivery metadata.
    /// Shared across all message types for consistent processing.
    pub header: EnvelopeHeader,
    /// Business-level extension metadata for custom attributes.
    /// Allows passing additional context without modifying the payload structure.
    pub metadata: BTreeMap<String, serde_json::Value>,
    /// The actual message payload content.
    /// Type parameter T allows type-safe handling of different message types.
    pub payload: T,
}

/// Core error codes for standardized error handling across the system.
/// These codes provide machine-readable error categorization for automated error handling,
/// retry logic, and user-facing error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    /// Message schema validation failed - payload structure doesn't match expected format.
    InvalidSchema,
    /// Business field validation failed - content doesn't meet business rules.
    ValidationFailed,
    /// Message is a duplicate of one already processed - idempotency check triggered.
    DuplicateMessage,
    /// Session is currently busy with another request - concurrency limit reached.
    SessionBusy,
    /// Agent processing exceeded the configured timeout threshold.
    AgentTimeout,
    /// Tool execution exceeded the configured timeout threshold.
    ToolTimeout,
    /// LLM provider service is unavailable - network or capacity issues.
    ProviderUnavailable,
    /// Provider response format is invalid or unparseable.
    ProviderResponseInvalid,
    /// Transport layer is unavailable - message broker connection failed.
    TransportUnavailable,
    /// All retry attempts exhausted - message moved to dead letter queue.
    RetryExhausted,
    /// Token budget limit exceeded - prevents runaway token consumption.
    BudgetExceeded,
    /// Message has been sent to dead letter queue after processing failure.
    SentToDeadLetter,
}

/// Trait for defining schema evolution and backward compatibility rules.
/// Implementations can define custom logic for determining if a newer schema version
/// can safely process messages from an older version.
pub trait SchemaEvolutionRule {
    /// Validates whether the 'to' schema version is backward compatible with 'from'.
    /// Returns true if messages from 'from' version can be processed by 'to' version.
    fn validate_backward_compatible(from: SchemaVersion, to: SchemaVersion) -> bool;
}

/// Default schema evolution rule implementation using semantic versioning.
/// Under semver rules, backward compatibility is maintained when major versions match
/// and the minor version of the target is greater than or equal to the source.
pub struct SemverEvolutionRule;

impl SchemaEvolutionRule for SemverEvolutionRule {
    fn validate_backward_compatible(from: SchemaVersion, to: SchemaVersion) -> bool {
        from.major == to.major && to.minor >= from.minor
    }
}
