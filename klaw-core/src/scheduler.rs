use async_trait::async_trait;
use std::time::Duration;

/// A task that can be scheduled by the session scheduler.
pub trait SessionTask: Send + Sync {
    /// Returns the session key that serializes this task.
    fn session_key(&self) -> &str;
    /// Returns the unique identifier for this task.
    fn task_id(&self) -> &str;
}

/// Overflow policy applied when a session queue is already saturated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueOverflowPolicy {
    /// Collect the work and keep it in the same queue.
    Collect,
    /// Convert the work into a follow-up task.
    FollowUp,
    /// Reject the work immediately.
    Drop,
}

/// Scheduling decision returned by a session scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskScheduleDecision {
    /// Execute the task immediately.
    ExecuteNow,
    /// The task was enqueued and the current queue depth is returned.
    Enqueued { queue_depth: usize },
    /// The task was rejected with a stable reason string.
    Rejected { reason: &'static str },
}

/// Serial scheduling abstraction for session-scoped work.
#[async_trait]
pub trait SessionScheduler<T>: Send + Sync
where
    T: SessionTask,
{
    /// Schedules a task and returns the resulting decision.
    async fn schedule(&self, task: T, overflow_policy: QueueOverflowPolicy)
    -> TaskScheduleDecision;

    /// Marks a task as complete and releases any session lock.
    async fn complete(&self, session_key: &str, task_id: &str);

    /// Returns the current queue depth for the session.
    async fn queue_depth(&self, session_key: &str) -> usize;

    /// Returns the maximum queue depth allowed by this scheduler.
    fn max_queue_depth(&self) -> usize;

    /// Returns the lock TTL used for session serialization.
    fn session_lock_ttl(&self) -> Duration;
}
