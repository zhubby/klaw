use crate::protocol::Envelope;
use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

/// 消息交付语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    /// 至少一次，可能重复。
    AtLeastOnce,
    /// 至多一次，可能丢失。
    AtMostOnce,
    /// 精确一次（实现成本高，通常依赖外部事务语义）。
    ExactlyOnce,
}

/// 可自定义主题名封装。
#[derive(Debug, Clone)]
pub struct MessageTopic(pub &'static str);

/// 订阅配置。
#[derive(Debug, Clone)]
pub struct Subscription {
    /// 订阅主题。
    pub topic: &'static str,
    /// 消费组标识。
    pub consumer_group: String,
    /// 可见性超时（用于重投）。
    pub visibility_timeout: Duration,
}

/// transport 返回的消息及其确认句柄。
#[derive(Debug, Clone)]
pub struct TransportMessage<T> {
    /// 业务消息。
    pub payload: Envelope<T>,
    /// 用于 ack/nack 的句柄。
    pub ack_handle: TransportAckHandle,
}

/// ack/nack 句柄。
#[derive(Debug, Clone)]
pub struct TransportAckHandle {
    /// broker 侧消息 ID。
    pub broker_message_id: String,
    /// 当前投递次数。
    pub delivery_attempt: u32,
}

/// 传输层错误。
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

/// 统一消息传输抽象。
#[async_trait]
pub trait MessageTransport<T>: Send + Sync {
    /// 返回传输层交付模式。
    fn mode(&self) -> DeliveryMode;

    /// 发布消息到指定主题。
    async fn publish(&self, topic: &'static str, msg: Envelope<T>) -> Result<(), TransportError>;

    /// 拉取一条消息。
    async fn consume(
        &self,
        subscription: &Subscription,
    ) -> Result<TransportMessage<T>, TransportError>;

    /// 确认处理完成。
    async fn ack(&self, handle: &TransportAckHandle) -> Result<(), TransportError>;

    /// 拒绝处理，可选延迟重投。
    async fn nack(
        &self,
        handle: &TransportAckHandle,
        requeue_after: Option<Duration>,
    ) -> Result<(), TransportError>;

    /// 以延迟方式重投消息（默认委托给 `nack`）。
    async fn requeue(
        &self,
        handle: &TransportAckHandle,
        delay: Duration,
    ) -> Result<(), TransportError> {
        self.nack(handle, Some(delay)).await
    }
}
