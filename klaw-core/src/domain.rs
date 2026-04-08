use crate::media::MediaReference;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Session unique key for serializing message processing.
/// Typically composed from channel identifier and chat/conversation identifier,
/// e.g., "terminal:user123" or "telegram:chat456". Used by the scheduler to ensure
/// all messages within the same session are processed sequentially.
pub type SessionKey = String;

/// Normalized inbound message structure representing user input entering the agent system.
/// This is the canonical format after parsing and normalization from various input channels
/// (terminal, websocket, message queue, webhook, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    /// The source channel where this message originated.
    /// Examples: "terminal", "telegram", "discord", "slack", "kafka", etc.
    pub channel: String,
    /// Unique identifier of the sender (user, bot, or system).
    /// Format depends on channel type: user ID, session ID, or system identifier.
    pub sender_id: String,
    /// Conversation/dialogue identifier for grouping related messages.
    /// Together with channel, forms the basis for session_key.
    pub chat_id: String,
    /// Session serialization key for ensuring ordered processing.
    /// Derived from channel and chat_id, used by the scheduler for queue management.
    pub session_key: SessionKey,
    /// Human-readable text content from the user.
    /// This is the primary input that the agent processes.
    pub content: String,
    /// Media attachments (images, files, etc.) included with the message.
    /// Empty vector if no attachments are present.
    #[serde(default)]
    pub media_references: Vec<MediaReference>,
    /// Additional metadata for routing, policy decisions, and extensibility.
    /// Allows passing channel-specific or application-specific context.
    pub metadata: BTreeMap<String, serde_json::Value>,
}

/// Normalized outbound message structure representing agent output sent to users.
/// This is the canonical format for all responses before being translated to
/// channel-specific formats for delivery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    /// Target delivery channel for this message.
    pub channel: String,
    /// Target conversation/dialogue for delivery.
    pub chat_id: String,
    /// Text content to send to the user.
    /// This is the primary output from agent processing.
    pub content: String,
    /// Optional message ID being replied to, enabling threading.
    pub reply_to: Option<String>,
    /// Additional metadata for channel routing and platform-specific features.
    /// May include attachments, buttons, or other rich UI elements.
    pub metadata: BTreeMap<String, serde_json::Value>,
}

/// Dead letter message structure for failed processing attempts.
/// Contains complete context needed for manual review, reprocessing, or compensation.
/// When a message fails all retry attempts, it's moved to a dead letter queue
/// for later investigation or automated remediation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterMessage {
    /// Original message identifier from the failed message's header.
    /// Useful for correlation and looking up original request details.
    pub original_message_id: String,
    /// Session key from the original message for context reconstruction.
    pub session_key: SessionKey,
    /// Final error that caused the message to be moved to dead letter.
    /// Provides the primary reason for failure.
    pub final_error: String,
    /// Number of retry attempts made before reaching dead letter.
    /// Indicates how many times the system attempted to process this message.
    pub attempts: u32,
    /// Human-readable description of why the message entered dead letter.
    /// Includes context about which retry policy or circuit breaker triggered the move.
    pub reason: String,
    /// The complete original inbound payload for reprocessing or analysis.
    /// Contains all user content, media, and metadata from the original request.
    pub original_payload: InboundMessage,
}
