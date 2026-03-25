use crate::client::McpClient;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct McpToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub server_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone)]
pub struct McpBootstrapFailure {
    pub server_id: String,
    pub reason: String,
}

#[derive(Default)]
pub struct McpRuntimeHandles {
    pub stdio_servers: Vec<String>,
}

pub struct McpBootstrapResult {
    pub descriptors: Vec<McpToolDescriptor>,
    pub hub: McpClientHub,
    pub runtime_handles: McpRuntimeHandles,
    pub failures: Vec<McpBootstrapFailure>,
}

#[derive(Debug, Error)]
pub enum McpClientHubError {
    #[error("mcp server `{0}` not found")]
    ServerNotFound(String),
    #[error("mcp call failed: {0}")]
    CallFailed(String),
}

#[derive(Default, Clone)]
pub struct McpClientHub {
    clients: Arc<RwLock<BTreeMap<String, Arc<dyn McpClient>>>>,
}

impl McpClientHub {
    pub(crate) fn insert(&self, server_id: String, client: Arc<dyn McpClient>) {
        self.clients
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .insert(server_id, client);
    }

    pub fn remove(&self, server_id: &str) {
        self.clients
            .write()
            .unwrap_or_else(|err| err.into_inner())
            .remove(server_id);
    }

    pub fn server_ids(&self) -> Vec<String> {
        self.clients
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    pub fn contains(&self, server_id: &str) -> bool {
        self.clients
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .contains_key(server_id)
    }

    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, McpClientHubError> {
        let client = {
            let guard = self.clients.read().unwrap_or_else(|err| err.into_inner());
            guard.get(server_id).cloned()
        };
        let Some(client) = client else {
            return Err(McpClientHubError::ServerNotFound(server_id.to_string()));
        };
        client
            .call_tool(tool_name, arguments)
            .await
            .map_err(|err| McpClientHubError::CallFailed(err.to_string()))
    }

    pub async fn shutdown_all(&self) -> Result<(), McpClientHubError> {
        let clients: Vec<(String, Arc<dyn McpClient>)> = self
            .clients
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .map(|(server_id, client)| (server_id.clone(), Arc::clone(client)))
            .collect();
        for (server_id, client) in clients {
            client
                .shutdown()
                .await
                .map_err(|err| McpClientHubError::CallFailed(format!("{server_id}: {err}")))?;
        }
        Ok(())
    }
}

pub fn format_tool_result_for_model(result: &Value) -> String {
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        let text_parts: Vec<&str> = content
            .iter()
            .filter_map(|item| {
                let item_type = item.get("type").and_then(Value::as_str)?;
                if item_type == "text" {
                    return item.get("text").and_then(Value::as_str);
                }
                None
            })
            .collect();
        if !text_parts.is_empty() {
            return text_parts.join("\n");
        }
    }
    result.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_tool_result_prefers_text_content() {
        let value = json!({
            "content": [
                {"type":"text","text":"hello"},
                {"type":"text","text":"world"}
            ]
        });
        assert_eq!(format_tool_result_for_model(&value), "hello\nworld");
    }
}
