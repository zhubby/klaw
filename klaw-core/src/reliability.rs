use async_trait::async_trait;
use std::{
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Retry decision returned by retry policies to indicate what action should be taken.
/// Provides explicit control over retry behavior rather than returning error codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    /// Retry the operation immediately without delay.
    /// Suitable for transient failures that are likely to succeed on retry.
    RetryNow,
    /// Retry the operation after the specified delay duration.
    /// Used for rate limiting or backoff scenarios.
    RetryAfter(Duration),
    /// Move the failed message to dead letter queue.
    /// Applied when the error indicates a permanent failure requiring manual intervention.
    SendToDeadLetter,
    /// Stop retrying and terminate processing.
    /// Used for validation errors or other non-recoverable situations.
    Abort,
}

/// Retry policy abstraction for determining retry behavior based on error type and attempt count.
/// Implementations can define custom logic for classifying errors and deciding retry strategy.
#[async_trait]
pub trait RetryPolicy: Send + Sync {
    /// Maximum number of retry attempts before giving up.
    /// Defines the upper bound for retry attempts.
    fn max_attempts(&self) -> u32;
    /// Classifies the error and returns the appropriate retry decision.
    /// @param error_kind - String identifier for the error category (e.g., "timeout", "validation")
    /// @param attempt - Current attempt number (starting from 1)
    fn classify(&self, error_kind: &str, attempt: u32) -> RetryDecision;
}

/// Error classification categories for systematic error handling.
/// Provides a structured way to categorize errors based on their nature and recoverability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Validation errors - payload or input doesn't meet requirements.
    /// These are typically permanent and should not be retried.
    Validation,
    /// Temporary dependency failures - external service temporarily unavailable.
    /// These are typically recoverable after a short delay.
    DependencyTemporary,
    /// Permanent dependency failures - external service returning persistent errors.
    /// These may require intervention and should not be blindly retried.
    DependencyPermanent,
    /// Temporary infrastructure failures - network or resource issues.
    /// These are typically recoverable after infrastructure recovers.
    InfrastructureTemporary,
    /// Permanent infrastructure failures - persistent system-level issues.
    InfrastructurePermanent,
    /// Budget or quota limit exceeded errors.
    /// These require quota increase or throttling adjustments to resolve.
    BudgetExceeded,
}

/// Dead letter policy configuration for handling permanently failed messages.
/// Defines how messages should be handled when all retry attempts are exhausted.
#[derive(Debug, Clone)]
pub struct DeadLetterPolicy {
    /// Dead letter topic/queue name where failed messages are sent.
    pub topic: &'static str,
    /// Maximum payload size allowed in bytes for dead letter messages.
    /// Messages exceeding this may be truncated or rejected.
    pub max_payload_bytes: usize,
    /// Whether to include full error stack traces in dead letter messages.
    /// Useful for debugging but may increase message size significantly.
    pub include_error_stack: bool,
}

/// Circuit breaker policy configuration for failure detection and recovery.
/// Circuit breakers prevent cascading failures by stopping requests to failing services.
#[derive(Debug, Clone)]
pub struct CircuitBreakerPolicy {
    /// Number of consecutive failures required to open the circuit.
    /// When exceeded, the circuit transitions from closed to open state.
    pub failure_threshold: u32,
    /// Duration the circuit remains open before transitioning to half-open.
    /// During half-open, limited test requests are allowed to check recovery.
    pub open_interval: Duration,
    /// Maximum number of requests allowed in half-open state.
    /// These test requests determine if the service has recovered.
    pub half_open_max_requests: u32,
}

/// Circuit breaker abstraction for preventing cascading failures.
/// Implements the circuit breaker pattern: closed (normal), open (failing), half-open (testing).
#[async_trait]
pub trait CircuitBreaker: Send + Sync {
    /// Checks if a request should be allowed to proceed.
    /// Returns true if circuit is closed or half-open with available test slots.
    async fn allow_request(&self) -> bool;
    /// Records a successful request, potentially closing an open circuit.
    /// Called after successful completion of a request.
    async fn on_success(&self);
    /// Records a failed request, potentially opening the circuit.
    /// Called when a request fails due to dependency issues.
    async fn on_failure(&self);
}

/// Exponential backoff retry policy with configurable delays.
/// Implements increasing delay between retries to avoid overwhelming failing services.
#[derive(Debug, Clone)]
pub struct ExponentialBackoffRetryPolicy {
    /// Maximum number of retry attempts before giving up.
    pub max_attempts: u32,
    /// Base delay duration for first retry.
    /// Each subsequent retry multiplies this delay exponentially.
    pub base_delay: Duration,
    /// Maximum delay cap to prevent excessively long wait times.
    /// Even with exponential backoff, delays are capped at this value.
    pub max_delay: Duration,
    /// Jitter ratio for adding randomness to delays (0.0 to 1.0).
    /// Helps prevent thundering herd when multiple clients retry simultaneously.
    /// Note: Current implementation does not yet inject random jitter.
    pub jitter_ratio: f32,
}

impl ExponentialBackoffRetryPolicy {
    /// Calculates the backoff delay for a given attempt number.
    /// Uses exponential formula: base_delay * 2^(attempt-1), capped at max_delay.
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let exp = 2u32.saturating_pow(attempt.saturating_sub(1));
        let delay = self.base_delay.saturating_mul(exp);
        delay.min(self.max_delay)
    }
}

impl RetryPolicy for ExponentialBackoffRetryPolicy {
    fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    fn classify(&self, error_kind: &str, attempt: u32) -> RetryDecision {
        if attempt >= self.max_attempts {
            return RetryDecision::SendToDeadLetter;
        }

        match error_kind {
            "validation" | "schema" | "duplicate" => RetryDecision::Abort,
            "provider_unavailable" | "transport_unavailable" | "tool_timeout" => {
                RetryDecision::RetryAfter(self.delay_for(attempt))
            }
            _ => RetryDecision::RetryNow,
        }
    }
}

/// Constructs a standardized idempotency key for deduplication.
/// Combines message ID, session key, and processing stage into a unique key.
pub fn idempotency_key(message_id: &str, session_key: &str, stage: &str) -> String {
    format!("{message_id}:{session_key}:{stage}")
}

/// In-memory circuit breaker implementation for local development and testing.
/// Not suitable for production distributed systems - use Redis-based implementations there.
#[derive(Debug)]
pub struct InMemoryCircuitBreaker {
    policy: CircuitBreakerPolicy,
    consecutive_failures: AtomicU32,
    open_until_epoch_ms: AtomicU64,
}

impl InMemoryCircuitBreaker {
    /// Creates a new in-memory circuit breaker with the given policy.
    pub fn new(policy: CircuitBreakerPolicy) -> Self {
        Self {
            policy,
            consecutive_failures: AtomicU32::new(0),
            open_until_epoch_ms: AtomicU64::new(0),
        }
    }

    fn now_epoch_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[async_trait]
impl CircuitBreaker for InMemoryCircuitBreaker {
    async fn allow_request(&self) -> bool {
        let now = Self::now_epoch_ms();
        let open_until = self.open_until_epoch_ms.load(Ordering::SeqCst);
        now >= open_until
    }

    async fn on_success(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.open_until_epoch_ms.store(0, Ordering::SeqCst);
    }

    async fn on_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if failures >= self.policy.failure_threshold {
            let open_until = Self::now_epoch_ms() + self.policy.open_interval.as_millis() as u64;
            self.open_until_epoch_ms.store(open_until, Ordering::SeqCst);
            self.consecutive_failures.store(0, Ordering::SeqCst);
        }
    }
}

/// Idempotency store abstraction for preventing duplicate message processing.
/// Implementations should support TTL for automatic cleanup of old entries.
#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// Checks if a key has been seen before.
    /// Returns true if the key exists (message already processed).
    async fn seen(&self, key: &str) -> bool;
    /// Marks a key as processed with a TTL for automatic expiration.
    /// The entry will automatically become "unseen" after TTL expires.
    async fn mark_seen(&self, key: &str, ttl: Duration);
    /// Removes a key from the idempotency store.
    /// Useful for manual cleanup or reprocessing scenarios.
    async fn clear(&self, key: &str);
}
