mod archive;
mod auth;
mod chat_page;
mod embedded;
mod error;
mod handlers;
mod home;
mod routes;
mod runtime;
mod state;
mod tailscale;
mod tests;
mod webhook;
mod websocket;

pub use error::GatewayError;
pub use routes::{
    ARCHIVE_DOWNLOAD_PATH, ARCHIVE_GET_PATH, ARCHIVE_LIST_PATH, ARCHIVE_UPLOAD_PATH,
    CHAT_DIST_JS_PATH, CHAT_DIST_WASM_PATH, CHAT_PATH, HOME_LOGO_PATH, HOME_PATH,
    WEBHOOK_AGENTS_PATH, WEBHOOK_EVENTS_PATH, WS_CHAT_PATH,
};
pub use runtime::{
    GatewayOptions, run_gateway, run_gateway_with_options, spawn_gateway,
    spawn_gateway_with_options,
};
pub use state::{GatewayArchiveState, GatewayHandle, GatewayRuntimeInfo};
pub use tailscale::{TailscaleHostInfo, TailscaleManager, TailscaleRuntimeInfo, TailscaleStatus};
pub use webhook::{
    GatewayWebhookAgentRequest, GatewayWebhookAgentResponse, GatewayWebhookHandler,
    GatewayWebhookHandlerError, GatewayWebhookRequest, GatewayWebhookResponse,
};
pub use websocket::{
    GatewaySessionHistoryMessage, GatewayWebsocketErrorFrame, GatewayWebsocketHandler,
    GatewayWebsocketHandlerError, GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest,
    GatewayWorkspaceBootstrap, GatewayWorkspaceSession, InboundMethod, OutboundEvent,
};
