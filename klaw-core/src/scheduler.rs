use async_trait::async_trait;
use std::time::Duration;

/// 可被会话调度器调度的任务。
pub trait SessionTask: Send + Sync {
    /// 返回该任务的会话键。
    fn session_key(&self) -> &str;
    /// 返回该任务唯一 ID。
    fn task_id(&self) -> &str;
}

/// 会话队列溢出策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueOverflowPolicy {
    /// 聚合并继续排队。
    Collect,
    /// 转化为 follow-up 任务。
    FollowUp,
    /// 直接拒绝。
    Drop,
}

/// 调度决策结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskScheduleDecision {
    /// 立即执行。
    ExecuteNow,
    /// 已入队，返回当前队列深度。
    Enqueued { queue_depth: usize },
    /// 被拒绝并附带原因。
    Rejected { reason: &'static str },
}

/// 会话串行调度抽象。
#[async_trait]
pub trait SessionScheduler<T>: Send + Sync
where
    T: SessionTask,
{
    /// 调度任务并返回决策。
    async fn schedule(
        &self,
        task: T,
        overflow_policy: QueueOverflowPolicy,
    ) -> TaskScheduleDecision;

    /// 标记任务执行完成，释放会话占用。
    async fn complete(&self, session_key: &str, task_id: &str);

    /// 查询会话队列深度。
    async fn queue_depth(&self, session_key: &str) -> usize;

    /// 返回允许的最大队列深度。
    fn max_queue_depth(&self) -> usize;

    /// 返回会话锁 TTL。
    fn session_lock_ttl(&self) -> Duration;
}
