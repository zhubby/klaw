use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::SystemTime};
use uuid::Uuid;

/// 协议版本号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaVersion {
    /// 主版本（破坏性变更递增）。
    pub major: u16,
    /// 次版本（向后兼容变更递增）。
    pub minor: u16,
}

impl SchemaVersion {
    /// 当前默认协议版本。
    pub const V1_0: Self = Self { major: 1, minor: 0 };
}

/// 逻辑主题类型（与具体 MQ 产品无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageTopic {
    /// 入站消息主题。
    Inbound,
    /// 最终出站消息主题。
    Outbound,
    /// 中间事件主题（流式/进度/状态）。
    Events,
    /// 死信主题。
    DeadLetter,
}

impl MessageTopic {
    /// 返回标准主题名。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inbound => "agent.inbound",
            Self::Outbound => "agent.outbound",
            Self::Events => "agent.events",
            Self::DeadLetter => "agent.dlq",
        }
    }
}

/// Envelope 头部，承载路由、追踪和重试语义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeHeader {
    /// 全局消息 ID。
    pub message_id: Uuid,
    /// 端到端链路追踪 ID。
    pub trace_id: Uuid,
    /// 会话串行键。
    pub session_key: String,
    /// 生成时间戳。
    pub timestamp: SystemTime,
    /// 当前重试次数（从 1 开始）。
    pub attempt: u32,
    /// 当前 schema 版本。
    pub schema_version: SchemaVersion,
    /// 多租户 ID（可选）。
    pub tenant_id: Option<String>,
    /// 逻辑命名空间（可选）。
    pub namespace: Option<String>,
    /// 优先级（可选）。
    pub priority: Option<u8>,
    /// 生存时间毫秒（可选）。
    pub ttl_ms: Option<u64>,
    /// 路由提示扩展字段。
    pub routing_hints: BTreeMap<String, serde_json::Value>,
}

impl EnvelopeHeader {
    /// 使用默认值创建头部。
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

/// 通用消息封装结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// 协议头。
    pub header: EnvelopeHeader,
    /// 业务扩展元数据。
    pub metadata: BTreeMap<String, serde_json::Value>,
    /// 实际负载。
    pub payload: T,
}

/// 核心错误码定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    /// schema 不合法。
    InvalidSchema,
    /// 业务字段校验失败。
    ValidationFailed,
    /// 命中幂等去重。
    DuplicateMessage,
    /// 会话繁忙。
    SessionBusy,
    /// agent 处理超时。
    AgentTimeout,
    /// tool 调用超时。
    ToolTimeout,
    /// provider 不可用。
    ProviderUnavailable,
    /// provider 返回格式非法。
    ProviderResponseInvalid,
    /// 传输层不可用。
    TransportUnavailable,
    /// 重试耗尽。
    RetryExhausted,
    /// 已进入死信。
    SentToDeadLetter,
}

/// 协议向后兼容规则接口。
pub trait SchemaEvolutionRule {
    /// 校验 `to` 是否可向后兼容 `from`。
    fn validate_backward_compatible(from: SchemaVersion, to: SchemaVersion) -> bool;
}

/// 基于语义化版本的默认规则实现。
pub struct SemverEvolutionRule;

impl SchemaEvolutionRule for SemverEvolutionRule {
    fn validate_backward_compatible(from: SchemaVersion, to: SchemaVersion) -> bool {
        from.major == to.major && to.minor >= from.minor
    }
}
