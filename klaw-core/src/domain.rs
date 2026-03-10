use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 会话唯一键，通常由 `channel:chat_id` 组成。
pub type SessionKey = String;

/// 标准化后的入站消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    /// 入站来源通道（如 stdio、mq）。
    pub channel: String,
    /// 发送者标识。
    pub sender_id: String,
    /// 对话标识。
    pub chat_id: String,
    /// 会话串行调度键。
    pub session_key: SessionKey,
    /// 用户可读文本内容。
    pub content: String,
    /// 附加元数据，用于路由/策略扩展。
    pub metadata: BTreeMap<String, serde_json::Value>,
}

/// 标准化后的出站消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    /// 目标通道。
    pub channel: String,
    /// 目标会话。
    pub chat_id: String,
    /// 发送给用户的文本内容。
    pub content: String,
    /// 可选的回复引用 ID。
    pub reply_to: Option<String>,
    /// 附加元数据，用于平台侧路由提示。
    pub metadata: BTreeMap<String, serde_json::Value>,
}

/// 死信消息结构，用于回溯失败上下文和补偿处理。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterMessage {
    /// 原始消息 ID。
    pub original_message_id: String,
    /// 关联会话键。
    pub session_key: SessionKey,
    /// 最终错误类型字符串。
    pub final_error: String,
    /// 已尝试次数。
    pub attempts: u32,
    /// 进入死信的原因描述。
    pub reason: String,
    /// 原始入站负载。
    pub original_payload: InboundMessage,
}
