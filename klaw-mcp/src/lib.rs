mod client;
mod hub;
mod manager;
mod runtime;

pub use hub::{
    McpBootstrapFailure, McpBootstrapResult, McpClientHub, McpClientHubError, McpRuntimeHandles,
    McpToolDescriptor, format_tool_result_for_model,
};
pub use manager::{
    McpBootstrapError, McpConfigSnapshot, McpInitHandle, McpLifecycleState, McpManager,
    McpRuntimeSnapshot, McpServerDetail, McpServerKey, McpServerStatus, McpSyncResult,
};
pub use runtime::McpProxyTool;
