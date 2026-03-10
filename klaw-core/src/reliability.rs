use async_trait::async_trait;
use std::{
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// 重试策略返回的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    /// 立即重试。
    RetryNow,
    /// 延迟后重试。
    RetryAfter(Duration),
    /// 写入死信。
    SendToDeadLetter,
    /// 停止重试并终止。
    Abort,
}

/// 重试策略抽象。
#[async_trait]
pub trait RetryPolicy: Send + Sync {
    /// 最大尝试次数。
    fn max_attempts(&self) -> u32;
    /// 根据错误类型和次数返回重试决策。
    fn classify(&self, error_kind: &str, attempt: u32) -> RetryDecision;
}

/// 统一错误分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// 校验类错误。
    Validation,
    /// 临时依赖错误（可重试）。
    DependencyTemporary,
    /// 永久依赖错误（通常不可重试）。
    DependencyPermanent,
    /// 临时基础设施错误（可重试）。
    InfrastructureTemporary,
    /// 永久基础设施错误。
    InfrastructurePermanent,
    /// 预算限制错误。
    BudgetExceeded,
}

/// 死信策略配置。
#[derive(Debug, Clone)]
pub struct DeadLetterPolicy {
    /// 死信主题名。
    pub topic: &'static str,
    /// 允许的最大负载大小。
    pub max_payload_bytes: usize,
    /// 是否包含错误栈信息。
    pub include_error_stack: bool,
}

/// 熔断器策略配置。
#[derive(Debug, Clone)]
pub struct CircuitBreakerPolicy {
    /// 连续失败阈值。
    pub failure_threshold: u32,
    /// 断路保持时间。
    pub open_interval: Duration,
    /// 半开阶段允许的探测请求数。
    pub half_open_max_requests: u32,
}

/// 熔断器抽象。
#[async_trait]
pub trait CircuitBreaker: Send + Sync {
    /// 是否允许放行本次请求。
    async fn allow_request(&self) -> bool;
    /// 请求成功后回调。
    async fn on_success(&self);
    /// 请求失败后回调。
    async fn on_failure(&self);
}

/// 指数退避重试策略。
#[derive(Debug, Clone)]
pub struct ExponentialBackoffRetryPolicy {
    /// 最大重试次数。
    pub max_attempts: u32,
    /// 基础延迟。
    pub base_delay: Duration,
    /// 最大延迟。
    pub max_delay: Duration,
    /// 随机抖动比例（当前实现暂未注入随机数）。
    pub jitter_ratio: f32,
}

impl ExponentialBackoffRetryPolicy {
    /// 计算指定尝试次数对应的退避时长。
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

/// 构造默认幂等键。
pub fn idempotency_key(message_id: &str, session_key: &str, stage: &str) -> String {
    format!("{message_id}:{session_key}:{stage}")
}

/// 基于内存的熔断器实现，适用于本地和测试。
#[derive(Debug)]
pub struct InMemoryCircuitBreaker {
    policy: CircuitBreakerPolicy,
    consecutive_failures: AtomicU32,
    open_until_epoch_ms: AtomicU64,
}

impl InMemoryCircuitBreaker {
    /// 创建熔断器实例。
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
            let open_until =
                Self::now_epoch_ms() + self.policy.open_interval.as_millis() as u64;
            self.open_until_epoch_ms.store(open_until, Ordering::SeqCst);
            self.consecutive_failures.store(0, Ordering::SeqCst);
        }
    }
}

/// 幂等存储抽象。
#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// 查询键是否已存在。
    async fn seen(&self, key: &str) -> bool;
    /// 标记键为已处理。
    async fn mark_seen(&self, key: &str, ttl: Duration);
    /// 清理指定键。
    async fn clear(&self, key: &str);
}
