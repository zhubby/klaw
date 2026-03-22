mod auth;
mod error;
mod handlers;
mod runtime;
mod state;
mod tailscale;
mod tests;
mod webhook;
mod websocket;

pub use error::GatewayError;
pub use runtime::{
    run_gateway, run_gateway_with_options, spawn_gateway, spawn_gateway_with_options,
    GatewayOptions,
};
pub use state::{GatewayHandle, GatewayRuntimeInfo};
pub use tailscale::{TailscaleRuntimeInfo, TailscaleStatus};
pub use webhook::{GatewayWebhookHandler, GatewayWebhookRequest, GatewayWebhookResponse};
