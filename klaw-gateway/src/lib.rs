mod archive;
mod auth;
mod chat_page;
mod embedded;
mod error;
mod handlers;
mod home;
mod protocol;
mod providers;
mod routes;
mod runtime;
mod state;
mod tailscale;
mod tests;
mod webhook;
mod websocket;

pub use error::GatewayError;
pub use protocol::{
    GATEWAY_WEBSOCKET_PROTOCOL_VERSION, GatewayApprovalDecision, GatewayApprovalRequest,
    GatewayApprovalScope, GatewayContentBlock, GatewayProtocolCapabilities,
    GatewayProtocolClientInfo, GatewayProtocolError, GatewayProtocolErrorCode,
    GatewayProtocolMethod, GatewayProtocolSchemaBundle, GatewayProtocolServerInfo,
    GatewayRpcMessage, GatewayServerRequestResolved, GatewayThreadItem, GatewayThreadItemStatus,
    GatewayThreadItemType, GatewayToolCall, GatewayToolCallStatus, GatewayTurnStatus,
    GatewayWebsocketProtocolInitializeParams, GatewayWebsocketProtocolInitializeResult,
    GatewayWebsocketProtocolVersion, GatewayWebsocketTurnStarted,
};
pub use routes::Route;
pub use runtime::{
    GatewayOptions, run_gateway, run_gateway_with_options, spawn_gateway,
    spawn_gateway_with_options,
};
pub use state::{
    GatewayArchiveState, GatewayHandle, GatewayProvidersState, GatewayRuntimeInfo,
    GatewayWebsocketBroadcaster,
};
pub use tailscale::{TailscaleHostInfo, TailscaleManager, TailscaleRuntimeInfo, TailscaleStatus};
pub use webhook::{
    GatewayWebhookAgentRequest, GatewayWebhookAgentResponse, GatewayWebhookHandler,
    GatewayWebhookHandlerError, GatewayWebhookRequest, GatewayWebhookResponse,
};
pub use websocket::{
    GATEWAY_WEBSOCKET_MAX_ACTIVE_TURNS_PER_CONNECTION, GATEWAY_WEBSOCKET_MAX_TEXT_FRAME_BYTES,
    GATEWAY_WEBSOCKET_OUTBOUND_QUEUE_CAPACITY, GatewayProviderCatalog, GatewayProviderEntry,
    GatewaySessionHistoryMessage, GatewaySessionHistoryPage, GatewayWebsocketAttachmentRef,
    GatewayWebsocketErrorFrame, GatewayWebsocketFrameTx, GatewayWebsocketHandler,
    GatewayWebsocketHandlerError, GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest,
    GatewayWorkspaceBootstrap, GatewayWorkspaceSession, InboundMethod, META_WEBSOCKET_MODEL,
    META_WEBSOCKET_MODEL_PROVIDER, META_WEBSOCKET_V1_THREAD_ID, META_WEBSOCKET_V1_TURN_ID,
    OutboundEvent,
};
