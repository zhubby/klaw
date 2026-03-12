use async_trait::async_trait;
use klaw_tool::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};
use std::sync::Arc;

use crate::{format_tool_result_for_model, McpClientHub, McpToolDescriptor};

pub struct McpProxyTool {
    descriptor: McpToolDescriptor,
    hub: Arc<McpClientHub>,
}

impl McpProxyTool {
    pub fn new(descriptor: McpToolDescriptor, hub: Arc<McpClientHub>) -> Self {
        Self { descriptor, hub }
    }
}

#[async_trait]
impl Tool for McpProxyTool {
    fn name(&self) -> &str {
        &self.descriptor.name
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.descriptor.parameters.clone()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::NetworkWrite
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let result = self
            .hub
            .call_tool(&self.descriptor.server_id, &self.descriptor.tool_name, args)
            .await
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
        let content = format_tool_result_for_model(&result);
        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
        })
    }
}
