use crate::manager::AcpManager;
use async_trait::async_trait;
use klaw_tool::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpToolDescriptor {
    pub name: String,
    pub description: String,
    pub agent_id: String,
}

#[derive(Debug, Error)]
pub enum AcpExecutionError {
    #[error("missing required string field `{0}`")]
    MissingField(&'static str),
    #[error("acp agent `{agent_id}` is not registered")]
    AgentNotFound { agent_id: String },
    #[error("invalid working directory `{0}`")]
    InvalidWorkingDirectory(String),
    #[error("failed to start acp worker runtime: {0}")]
    Runtime(String),
    #[error("failed to spawn acp agent process: {0}")]
    Spawn(String),
    #[error("failed to resolve working directory: {0}")]
    WorkingDirectory(String),
    #[error("acp initialize failed: {0}")]
    Initialize(String),
    #[error("acp new_session failed: {0}")]
    NewSession(String),
    #[error("acp prompt failed: {0}")]
    Prompt(String),
    #[error("acp prompt timed out for `{agent_id}` after {timeout:?}")]
    Timeout { agent_id: String, timeout: Duration },
    #[error("acp worker join failed: {0}")]
    WorkerJoin(String),
    #[error("acp agent `{agent_id}` returned no usable output")]
    EmptyResponse { agent_id: String },
}

#[derive(Clone)]
pub struct AcpProxyTool {
    descriptor: AcpToolDescriptor,
    manager: Arc<tokio::sync::Mutex<AcpManager>>,
}

impl AcpProxyTool {
    #[must_use]
    pub fn new(
        descriptor: AcpToolDescriptor,
        manager: Arc<tokio::sync::Mutex<AcpManager>>,
    ) -> Self {
        Self {
            descriptor,
            manager,
        }
    }
}

#[async_trait]
impl Tool for AcpProxyTool {
    fn name(&self) -> &str {
        &self.descriptor.name
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Natural language task for the external ACP agent."
                },
                "working_directory": {
                    "type": "string",
                    "description": "Optional working directory for the ACP agent session."
                },
                "timeout_seconds": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Optional timeout override for the ACP prompt turn."
                }
            },
            "required": ["prompt"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Shell
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let prompt = args
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(ToolError::InvalidArgs(
                AcpExecutionError::MissingField("prompt").to_string(),
            ))?;
        let working_directory = args
            .get("working_directory")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let timeout = args
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .map(Duration::from_secs);

        let mut manager = self.manager.lock().await;
        let content = manager
            .execute_prompt(
                &self.descriptor.agent_id,
                prompt,
                working_directory.as_deref(),
                timeout,
            )
            .await
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        Ok(ToolOutput {
            content_for_model: content.clone(),
            content_for_user: Some(content),
            signals: Vec::new(),
        })
    }
}
