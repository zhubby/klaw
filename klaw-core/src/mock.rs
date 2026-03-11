use crate::{
    reliability::IdempotencyStore,
    scheduler::{QueueOverflowPolicy, SessionScheduler, SessionTask, TaskScheduleDecision},
    transport::{
        DeliveryMode, MessageTransport, Subscription, TransportAckHandle, TransportError,
        TransportMessage,
    },
    Envelope,
};
use async_trait::async_trait;
use std::{collections::HashSet, collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::Mutex;

/// 内存传输实现，用于本地与测试。
#[derive(Debug, Clone)]
pub struct InMemoryTransport<T> {
    queue: Arc<Mutex<VecDeque<Envelope<T>>>>,
    published: Arc<Mutex<Vec<Envelope<T>>>>,
}

impl<T> Default for InMemoryTransport<T> {
    fn default() -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            published: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl<T> InMemoryTransport<T> {
    /// 创建空队列传输实例。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入一条待消费消息。
    pub async fn enqueue(&self, msg: Envelope<T>) {
        self.queue.lock().await.push_back(msg);
    }
}

impl<T: Clone> InMemoryTransport<T> {
    /// 获取已发布消息快照。
    pub async fn published_messages(&self) -> Vec<Envelope<T>> {
        self.published.lock().await.clone()
    }
}

#[async_trait]
impl<T> MessageTransport<T> for InMemoryTransport<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// 内存实现默认采用 at-least-once 语义。
    fn mode(&self) -> DeliveryMode {
        DeliveryMode::AtLeastOnce
    }

    async fn publish(&self, _topic: &'static str, msg: Envelope<T>) -> Result<(), TransportError> {
        self.published.lock().await.push(msg);
        Ok(())
    }

    async fn consume(
        &self,
        _subscription: &Subscription,
    ) -> Result<TransportMessage<T>, TransportError> {
        let mut queue = self.queue.lock().await;
        let Some(payload) = queue.pop_front() else {
            return Err(TransportError::ConsumeFailed("queue empty".to_string()));
        };

        let handle = TransportAckHandle {
            broker_message_id: payload.header.message_id.to_string(),
            delivery_attempt: payload.header.attempt,
        };
        Ok(TransportMessage {
            payload,
            ack_handle: handle,
        })
    }

    async fn ack(&self, _handle: &TransportAckHandle) -> Result<(), TransportError> {
        Ok(())
    }

    async fn nack(
        &self,
        _handle: &TransportAckHandle,
        _requeue_after: Option<Duration>,
    ) -> Result<(), TransportError> {
        Ok(())
    }
}

/// 内存会话调度器，提供最小可用调度行为。
#[derive(Debug, Clone)]
pub struct InMemorySessionScheduler {
    max_depth: usize,
    lock_ttl: Duration,
}

impl InMemorySessionScheduler {
    /// 创建调度器。
    pub fn new(max_depth: usize, lock_ttl: Duration) -> Self {
        Self {
            max_depth,
            lock_ttl,
        }
    }
}

#[async_trait]
impl<T> SessionScheduler<T> for InMemorySessionScheduler
where
    T: SessionTask + Send + Sync + 'static,
{
    async fn schedule(
        &self,
        _task: T,
        overflow_policy: QueueOverflowPolicy,
    ) -> TaskScheduleDecision {
        match overflow_policy {
            QueueOverflowPolicy::Collect | QueueOverflowPolicy::FollowUp => {
                TaskScheduleDecision::ExecuteNow
            }
            QueueOverflowPolicy::Drop => TaskScheduleDecision::Rejected {
                reason: "drop policy active",
            },
        }
    }

    async fn complete(&self, _session_key: &str, _task_id: &str) {}

    async fn queue_depth(&self, _session_key: &str) -> usize {
        0
    }

    fn max_queue_depth(&self) -> usize {
        self.max_depth
    }

    fn session_lock_ttl(&self) -> Duration {
        self.lock_ttl
    }
}

/// 内存幂等存储实现。
#[derive(Debug, Default, Clone)]
pub struct InMemoryIdempotencyStore {
    keys: Arc<Mutex<HashSet<String>>>,
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn seen(&self, key: &str) -> bool {
        self.keys.lock().await.contains(key)
    }

    async fn mark_seen(&self, key: &str, _ttl: Duration) {
        self.keys.lock().await.insert(key.to_string());
    }

    async fn clear(&self, key: &str) {
        self.keys.lock().await.remove(key);
    }
}
