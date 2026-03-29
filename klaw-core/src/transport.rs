use crate::protocol::Envelope;
use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

/// Message delivery semantics defining guarantees provided by the transport layer.
/// Different transport implementations may offer different delivery guarantees
/// depending on the underlying message broker capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    /// At-least-once delivery - messages may be delivered multiple times but never lost.
    /// Consumer must handle duplicates through idempotency.
    AtLeastOnce,
    /// At-most-once delivery - messages may be lost but never delivered multiple times.
    /// Suitable for fire-and-forget scenarios where duplicates are worse than loss.
    AtMostOnce,
    /// Exactly-once delivery - each message is delivered precisely once.
    /// Implementation typically requires external transaction semantics or
    /// two-phase commit protocols. Often expensive to implement.
    ExactlyOnce,
}

/// Wrapper for customizable topic names with static lifetime.
/// Allows transport implementations to use topic names efficiently without
/// heap allocation for each message.
#[derive(Debug, Clone)]
pub struct MessageTopic(pub &'static str);

/// Subscription configuration for consuming messages from a topic.
/// Defines how the consumer should connect to the message source and
/// how unacknowledged messages should be handled.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// The topic name to subscribe to for receiving messages.
    pub topic: &'static str,
    /// Consumer group identifier for load balancing across multiple consumers.
    /// Messages within the same group are distributed among group members.
    pub consumer_group: String,
    /// Visibility timeout duration for message re-delivery.
    /// If a message is not acknowledged within this period, it becomes
    /// visible to other consumers for redelivery.
    pub visibility_timeout: Duration,
}

/// Transport message wrapper containing the payload and acknowledgment handle.
/// The acknowledgment handle is required to confirm successful processing
/// or signal failure for retry/dead-letter handling.
#[derive(Debug, Clone)]
pub struct TransportMessage<T> {
    /// The actual business message content wrapped in an envelope.
    /// Contains header metadata and typed payload.
    pub payload: Envelope<T>,
    /// Handle for acknowledging or rejecting the message.
    /// Must be called to prevent message redelivery or trigger retry logic.
    pub ack_handle: TransportAckHandle,
}

/// Handle for message acknowledgment (ack) or rejection (nack).
/// Provides the transport layer with context needed to manage message delivery
/// and implement retry/redelivery logic.
#[derive(Debug, Clone)]
pub struct TransportAckHandle {
    /// Unique identifier for the message at the broker level.
    /// Used for correlation and deduplication at the transport layer.
    pub broker_message_id: String,
    /// Current delivery attempt number for this message.
    /// Starts at 1 and increments on each redelivery.
    pub delivery_attempt: u32,
}

/// Transport layer error types covering common failure scenarios.
/// Errors are categorized by the operation that failed to aid in
/// error handling and recovery strategy selection.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("transport unavailable: {0}")]
    Unavailable(String),
    #[error("publish failed: {0}")]
    PublishFailed(String),
    #[error("consume failed: {0}")]
    ConsumeFailed(String),
    #[error("ack failed: {0}")]
    AckFailed(String),
    #[error("nack failed: {0}")]
    NackFailed(String),
}

/// Unified message transport abstraction for publishing and consuming messages.
/// Defines the interface that all transport implementations must provide,
/// enabling the agent to work with various message brokers through a common API.
#[async_trait]
pub trait MessageTransport<T>: Send + Sync {
    /// Returns the delivery mode supported by this transport implementation.
    /// Indicates what delivery guarantees the transport provides.
    fn mode(&self) -> DeliveryMode;

    /// Publishes a message to the specified topic.
    /// The message is wrapped in an envelope containing routing metadata.
    async fn publish(&self, topic: &'static str, msg: Envelope<T>) -> Result<(), TransportError>;

    /// Pulls a single message from the subscription.
    /// Blocks until a message is available or an error occurs.
    async fn consume(
        &self,
        subscription: &Subscription,
    ) -> Result<TransportMessage<T>, TransportError>;

    /// Acknowledges successful processing of the message.
    /// After ack, the message will not be redelivered.
    async fn ack(&self, handle: &TransportAckHandle) -> Result<(), TransportError>;

    /// Rejects the message with optional delay before redelivery.
    /// If requeue_after is specified, the message will reappear after that duration.
    /// Otherwise, the message may be immediately available for redelivery.
    async fn nack(
        &self,
        handle: &TransportAckHandle,
        requeue_after: Option<Duration>,
    ) -> Result<(), TransportError>;

    /// Requeues the message with a specified delay before redelivery.
    /// Default implementation delegates to nack with a delay.
    /// Allows for simple delay-based retry without immediate requeue.
    async fn requeue(
        &self,
        handle: &TransportAckHandle,
        delay: Duration,
    ) -> Result<(), TransportError> {
        self.nack(handle, Some(delay)).await
    }
}
