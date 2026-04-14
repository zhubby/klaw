mod archive;
mod auth;
mod chat_page;
mod embedded;
mod error;
mod handlers;
mod home;
mod providers;
mod routes;
mod runtime;
mod state;
mod tailscale;
mod tests;
mod webhook;
mod websocket;

pub use error::GatewayError;
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
    GatewayProviderCatalog, GatewayProviderEntry, GatewaySessionHistoryMessage,
    GatewaySessionHistoryPage, GatewayWebsocketErrorFrame, GatewayWebsocketHandler,
    GatewayWebsocketHandlerError, GatewayWebsocketServerFrame, GatewayWebsocketSubmitRequest,
    GatewayWorkspaceBootstrap, GatewayWorkspaceSession, InboundMethod, META_WEBSOCKET_MODEL,
    META_WEBSOCKET_MODEL_PROVIDER, OutboundEvent,
};
