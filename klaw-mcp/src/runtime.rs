use async_trait::async_trait;
use klaw_tool::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

use crate::{McpClientHub, McpToolDescriptor, format_tool_result_for_model};

pub struct McpProxyTool {
    descriptor: McpToolDescriptor,
    hub: McpClientHub,
}

impl McpProxyTool {
    pub fn new(descriptor: McpToolDescriptor, hub: McpClientHub) -> Self {
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
            signals: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{McpClient, McpClientError, McpRemoteTool};
    use async_trait::async_trait;
    use klaw_tool::ToolContext;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    struct TestClient;

    #[async_trait]
    impl McpClient for TestClient {
        async fn initialize(&self) -> Result<(), McpClientError> {
            Ok(())
        }

        async fn list_tools(&self) -> Result<Vec<McpRemoteTool>, McpClientError> {
            Ok(Vec::new())
        }

        async fn call_tool(
            &self,
            tool_name: &str,
            arguments: serde_json::Value,
        ) -> Result<serde_json::Value, McpClientError> {
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("{tool_name}:{}", arguments["scope"].as_str().unwrap_or_default()),
                }]
            }))
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_key: "test-session".to_string(),
            metadata: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn proxy_tool_uses_live_hub_state_after_client_insert() {
        let hub = McpClientHub::default();
        let tool = McpProxyTool::new(
            McpToolDescriptor {
                name: "configuration_contexts_list".to_string(),
                description: "list contexts".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "scope": { "type": "string" }
                    }
                }),
                server_id: "kubernetes-mcp".to_string(),
                tool_name: "configuration_contexts_list".to_string(),
            },
            hub.clone(),
        );

        hub.insert("kubernetes-mcp".to_string(), Arc::new(TestClient));

        let output = tool
            .execute(json!({"scope": "all"}), &test_ctx())
            .await
            .expect("proxy tool should resolve client from shared hub");

        assert_eq!(output.content_for_model, "configuration_contexts_list:all");
        assert_eq!(
            output.content_for_user.as_deref(),
            Some("configuration_contexts_list:all")
        );
    }
}
