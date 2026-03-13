mod client;
mod hub;
mod manager;
mod runtime;

pub use hub::{
    format_tool_result_for_model, McpBootstrapFailure, McpBootstrapResult, McpClientHub,
    McpClientHubError, McpRuntimeHandles, McpToolDescriptor,
};
pub use manager::{McpBootstrapError, McpBootstrapHandle, McpBootstrapSummary, McpManager};
pub use runtime::McpProxyTool;
