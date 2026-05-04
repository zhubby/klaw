use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const GATEWAY_WEBSOCKET_PROTOCOL_VERSION: &str = "gateway.websocket.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GatewayWebsocketProtocolVersion {
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayProtocolClientInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayProtocolCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<GatewayWebsocketProtocolVersion>,
    #[serde(default)]
    pub experimental: bool,
    #[serde(default)]
    pub turns: bool,
    #[serde(default)]
    pub items: bool,
    #[serde(default)]
    pub tools: bool,
    #[serde(default)]
    pub approvals: bool,
    #[serde(default)]
    pub server_requests: bool,
    #[serde(default)]
    pub cancellation: bool,
    #[serde(default)]
    pub steering: bool,
    #[serde(default)]
    pub schema: bool,
    #[serde(default)]
    pub notification_opt_out: Vec<GatewayProtocolMethod>,
}

impl Default for GatewayProtocolCapabilities {
    fn default() -> Self {
        Self {
            protocol_version: Some(GatewayWebsocketProtocolVersion::V1),
            experimental: false,
            turns: true,
            items: true,
            tools: false,
            approvals: false,
            server_requests: false,
            cancellation: true,
            steering: true,
            schema: true,
            notification_opt_out: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayWebsocketProtocolInitializeParams {
    pub client_info: GatewayProtocolClientInfo,
    #[serde(default)]
    pub capabilities: GatewayProtocolCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayWebsocketProtocolInitializeResult {
    pub protocol_version: GatewayWebsocketProtocolVersion,
    pub protocol_name: String,
    pub connection_id: String,
    pub capabilities: GatewayProtocolCapabilities,
    pub server_info: GatewayProtocolServerInfo,
}

impl GatewayWebsocketProtocolInitializeResult {
    #[must_use]
    pub fn negotiate(
        connection_id: String,
        params: GatewayWebsocketProtocolInitializeParams,
    ) -> Self {
        let mut capabilities = GatewayProtocolCapabilities::default();
        capabilities.experimental = params.capabilities.experimental;
        capabilities.notification_opt_out = params.capabilities.notification_opt_out;
        Self {
            protocol_version: GatewayWebsocketProtocolVersion::V1,
            protocol_name: GATEWAY_WEBSOCKET_PROTOCOL_VERSION.to_string(),
            connection_id,
            capabilities,
            server_info: GatewayProtocolServerInfo {
                name: "klaw-gateway".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayProtocolServerInfo {
    pub name: String,
    pub version: String,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum GatewayProtocolMethod {
    #[serde(rename = "initialize")]
    Initialize,
    #[serde(rename = "initialized")]
    Initialized,
    #[serde(rename = "session/list")]
    SessionList,
    #[serde(rename = "session/create")]
    SessionCreate,
    #[serde(rename = "session/update")]
    SessionUpdate,
    #[serde(rename = "session/delete")]
    SessionDelete,
    #[serde(rename = "session/subscribe")]
    SessionSubscribe,
    #[serde(rename = "session/unsubscribe")]
    SessionUnsubscribe,
    #[serde(rename = "session/subscribed")]
    SessionSubscribed,
    #[serde(rename = "session/unsubscribed")]
    SessionUnsubscribed,
    #[serde(rename = "provider/list")]
    ProviderList,
    #[serde(rename = "thread/start")]
    ThreadStart,
    #[serde(rename = "thread/resume")]
    ThreadResume,
    #[serde(rename = "thread/read")]
    ThreadRead,
    #[serde(rename = "thread/list")]
    ThreadList,
    #[serde(rename = "thread/history")]
    ThreadHistory,
    #[serde(rename = "thread/rollback")]
    ThreadRollback,
    #[serde(rename = "thread/started")]
    ThreadStarted,
    #[serde(rename = "thread/statusChanged")]
    ThreadStatusChanged,
    #[serde(rename = "thread/closed")]
    ThreadClosed,
    #[serde(rename = "turn/start")]
    TurnStart,
    #[serde(rename = "turn/steer")]
    TurnSteer,
    #[serde(rename = "turn/cancel")]
    TurnCancel,
    #[serde(rename = "turn/read")]
    TurnRead,
    #[serde(rename = "turn/started")]
    TurnStarted,
    #[serde(rename = "turn/completed")]
    TurnCompleted,
    #[serde(rename = "turn/failed")]
    TurnFailed,
    #[serde(rename = "turn/interrupted")]
    TurnInterrupted,
    #[serde(rename = "item/started")]
    ItemStarted,
    #[serde(rename = "item/updated")]
    ItemUpdated,
    #[serde(rename = "item/completed")]
    ItemCompleted,
    #[serde(rename = "item/agentMessage/delta")]
    ItemAgentMessageDelta,
    #[serde(rename = "item/reasoning/delta")]
    ItemReasoningDelta,
    #[serde(rename = "item/plan/delta")]
    ItemPlanDelta,
    #[serde(rename = "approval/respond")]
    ApprovalRespond,
    #[serde(rename = "tool/respond")]
    ToolRespond,
    #[serde(rename = "user_input/respond")]
    UserInputRespond,
    #[serde(rename = "approval/request")]
    ApprovalRequest,
    #[serde(rename = "tool/requestUserInput")]
    ToolRequestUserInput,
    #[serde(rename = "serverRequest/resolved")]
    ServerRequestResolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum GatewayRpcMessage {
    Request {
        id: String,
        method: GatewayProtocolMethod,
        #[serde(default)]
        params: Value,
    },
    Notification {
        method: GatewayProtocolMethod,
        #[serde(default)]
        params: Value,
    },
    Success {
        id: String,
        #[serde(default)]
        result: Value,
    },
    Error {
        id: Option<String>,
        error: GatewayProtocolError,
    },
}

impl GatewayRpcMessage {
    #[must_use]
    pub fn request(id: impl Into<String>, method: GatewayProtocolMethod, params: Value) -> Self {
        Self::Request {
            id: id.into(),
            method,
            params,
        }
    }

    #[must_use]
    pub fn notification(method: GatewayProtocolMethod, params: Value) -> Self {
        Self::Notification { method, params }
    }

    #[must_use]
    pub fn success(id: impl Into<String>, result: Value) -> Self {
        Self::Success {
            id: id.into(),
            result,
        }
    }

    #[must_use]
    pub fn error(
        id: Option<String>,
        code: GatewayProtocolErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self::Error {
            id,
            error: GatewayProtocolError {
                code,
                message: message.into(),
                data: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayProtocolError {
    pub code: GatewayProtocolErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GatewayProtocolErrorCode {
    InvalidJson,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    NotInitialized,
    UnsupportedCapability,
    Overloaded,
    PayloadTooLarge,
    RateLimited,
    TooManyActiveTurns,
    SessionNotFound,
    ThreadNotFound,
    TurnNotFound,
    PermissionDenied,
    ModelError,
    ToolError,
    Cancelled,
    Timeout,
    InternalError,
}

impl GatewayProtocolErrorCode {
    #[must_use]
    pub fn stable_v1() -> Vec<Self> {
        vec![
            Self::InvalidJson,
            Self::InvalidRequest,
            Self::MethodNotFound,
            Self::InvalidParams,
            Self::NotInitialized,
            Self::UnsupportedCapability,
            Self::Overloaded,
            Self::PayloadTooLarge,
            Self::RateLimited,
            Self::TooManyActiveTurns,
            Self::SessionNotFound,
            Self::ThreadNotFound,
            Self::TurnNotFound,
            Self::PermissionDenied,
            Self::ModelError,
            Self::ToolError,
            Self::Cancelled,
            Self::Timeout,
            Self::InternalError,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayWebsocketTurnStarted {
    pub session_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub status: GatewayTurnStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GatewayTurnStatus {
    Queued,
    InProgress,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayThreadItem {
    pub item_id: String,
    pub turn_id: String,
    #[serde(rename = "type")]
    pub item_type: GatewayThreadItemType,
    pub status: GatewayThreadItemStatus,
    #[serde(default)]
    pub payload: Value,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum GatewayThreadItemType {
    UserMessage,
    AgentMessage,
    Reasoning,
    Plan,
    ToolCall,
    CommandExecution,
    FileChange,
    McpToolCall,
    ApprovalRequest,
    DynamicToolCall,
}

impl GatewayThreadItemType {
    #[must_use]
    pub fn stable_v1() -> Vec<Self> {
        vec![
            Self::UserMessage,
            Self::AgentMessage,
            Self::Reasoning,
            Self::Plan,
            Self::ToolCall,
            Self::CommandExecution,
            Self::FileChange,
            Self::McpToolCall,
            Self::ApprovalRequest,
            Self::DynamicToolCall,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GatewayThreadItemStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Declined,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GatewayContentBlock {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uri: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        archive_id: Option<String>,
    },
    Attachment {
        archive_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(default)]
        size_bytes: i64,
    },
    UiPayload {
        namespace: String,
        payload: Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GatewayApprovalScope {
    Turn,
    Session,
    Thread,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayToolCall {
    pub tool_call_id: String,
    pub name: String,
    pub kind: String,
    pub status: GatewayToolCallStatus,
    #[serde(default)]
    pub arguments: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<GatewayProtocolError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GatewayToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Declined,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayApprovalRequest {
    pub request_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub scope: GatewayApprovalScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub options: Vec<GatewayApprovalDecision>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GatewayApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayServerRequestResolved {
    pub thread_id: String,
    pub turn_id: String,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GatewayProtocolSchemaBundle {
    pub protocol_version: GatewayWebsocketProtocolVersion,
    pub definitions: BTreeMap<String, Value>,
    pub error_codes: Vec<GatewayProtocolErrorCode>,
}

impl GatewayProtocolSchemaBundle {
    #[must_use]
    pub fn v1() -> Self {
        let mut definitions = BTreeMap::new();
        insert_schema::<GatewayRpcMessage>(&mut definitions, "GatewayRpcMessage");
        insert_schema::<GatewayWebsocketProtocolInitializeParams>(
            &mut definitions,
            "GatewayWebsocketProtocolInitializeParams",
        );
        insert_schema::<GatewayWebsocketProtocolInitializeResult>(
            &mut definitions,
            "GatewayWebsocketProtocolInitializeResult",
        );
        insert_schema::<GatewayWebsocketTurnStarted>(
            &mut definitions,
            "GatewayWebsocketTurnStarted",
        );
        insert_schema::<GatewayThreadItem>(&mut definitions, "GatewayThreadItem");
        insert_schema::<GatewayContentBlock>(&mut definitions, "GatewayContentBlock");
        insert_schema::<GatewayToolCall>(&mut definitions, "GatewayToolCall");
        insert_schema::<GatewayApprovalRequest>(&mut definitions, "GatewayApprovalRequest");
        insert_schema::<GatewayServerRequestResolved>(
            &mut definitions,
            "GatewayServerRequestResolved",
        );

        Self {
            protocol_version: GatewayWebsocketProtocolVersion::V1,
            definitions,
            error_codes: GatewayProtocolErrorCode::stable_v1(),
        }
    }

    #[must_use]
    pub fn as_value(&self) -> Value {
        json!({
            "protocol_version": self.protocol_version,
            "definitions": self.definitions,
            "error_codes": self.error_codes,
        })
    }
}

fn insert_schema<T: JsonSchema>(definitions: &mut BTreeMap<String, Value>, name: &str) {
    let schema = schema_for!(T);
    let schema_value = serde_json::to_value(schema).unwrap_or_else(|_| json!({}));
    definitions.insert(name.to_string(), schema_value);
}
