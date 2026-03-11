use async_trait::async_trait;
use klaw_agent::{
    build_provider_from_config, run_agent_execution, AgentExecutionError, AgentExecutionInput,
    AgentExecutionLimits, ToolExecutor,
};
use klaw_config::{AppConfig, SubAgentConfig};
use klaw_llm::ToolDefinition;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolRegistry};

const META_PROVIDER_KEY: &str = "agent.provider_id";
const META_MODEL_KEY: &str = "agent.model";
const META_PARENT_SESSION_KEY: &str = "sub_agent.parent_session_key";
const META_CONTEXT_KEY: &str = "sub_agent.context";

pub struct SubAgentTool {
    config: Arc<AppConfig>,
    parent_tools: ToolRegistry,
    sub_config: SubAgentConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubAgentRequest {
    task: String,
    context: Value,
}

impl SubAgentTool {
    pub fn new(config: Arc<AppConfig>, parent_tools: ToolRegistry) -> Self {
        Self {
            sub_config: config.tools.sub_agent.clone(),
            config,
            parent_tools,
        }
    }

    fn parse_request(args: Value) -> Result<SubAgentRequest, ToolError> {
        let request: SubAgentRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        if request.task.trim().is_empty() {
            return Err(ToolError::InvalidArgs("`task` cannot be empty".to_string()));
        }
        if !request.context.is_object() {
            return Err(ToolError::InvalidArgs(
                "`context` must be an object".to_string(),
            ));
        }
        if request
            .context
            .get("session")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(ToolError::InvalidArgs(
                "`context.session` is required and must be a non-empty string".to_string(),
            ));
        }
        Ok(request)
    }

    fn resolve_provider_model(&self, ctx: &ToolContext) -> Result<(String, String), ToolError> {
        let parent_provider = ctx
            .metadata
            .get(META_PROVIDER_KEY)
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "missing parent provider in context metadata (`agent.provider_id`)".to_string(),
                )
            })?;

        let model = ctx
            .metadata
            .get(META_MODEL_KEY)
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "missing parent model in context metadata (`agent.model`)".to_string(),
                )
            })?;

        Ok((parent_provider, model))
    }

    fn build_child_registry(&self) -> ToolRegistry {
        let mut registry = ToolRegistry::default();
        if !self.sub_config.inherit_parent_tools {
            return registry;
        }

        for name in self.parent_tools.list() {
            if self
                .sub_config
                .exclude_tools
                .iter()
                .any(|excluded| excluded == &name)
            {
                continue;
            }
            if let Some(tool) = self.parent_tools.get(&name) {
                registry.register_shared(tool);
            }
        }
        registry
    }
}

struct RegistryToolExecutor<'a> {
    tools: &'a ToolRegistry,
}

#[async_trait]
impl ToolExecutor for RegistryToolExecutor<'_> {
    fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .list()
            .into_iter()
            .filter_map(|name| self.tools.get(&name))
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect()
    }

    async fn execute(
        &self,
        name: &str,
        arguments: Value,
        session_key: &str,
        metadata: &BTreeMap<String, Value>,
    ) -> String {
        let Some(tool) = self.tools.get(name) else {
            return format!("tool `{}` not found", name);
        };
        match tool
            .execute(
                arguments,
                &ToolContext {
                    session_key: session_key.to_string(),
                    metadata: metadata.clone(),
                },
            )
            .await
        {
            Ok(output) => output.content_for_model,
            Err(err) => format!("tool `{}` failed: {}", name, err),
        }
    }
}

#[async_trait]
impl Tool for SubAgentTool {
    fn name(&self) -> &str {
        "sub_agent"
    }

    fn description(&self) -> &str {
        "Run a delegated sub-agent task using the parent session context, inheriting parent provider/model and tool availability."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Task prompt delegated to the sub-agent."
                },
                "context": {
                    "type": "object",
                    "description": "Required context object. Must include `session` for parent session lookup/inheritance.",
                    "properties": {
                        "session": {
                            "type": "string",
                            "description": "Parent session identifier used to inherit model provider/model."
                        }
                    },
                    "required": ["session"]
                }
            },
            "required": ["task", "context"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Memory
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;
        let (provider_id, model) = self.resolve_provider_model(ctx)?;
        let provider_instance = build_provider_from_config(self.config.as_ref(), &provider_id)
            .map_err(|err| ToolError::ExecutionFailed(format!("build provider failed: {err}")))?;
        let child_tools = self.build_child_registry();
        let executor = RegistryToolExecutor { tools: &child_tools };
        let parent_session = request
            .context
            .get("session")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string();

        let mut child_metadata = ctx.metadata.clone();
        child_metadata.insert(
            META_PARENT_SESSION_KEY.to_string(),
            Value::String(parent_session.clone()),
        );
        child_metadata.insert(META_PROVIDER_KEY.to_string(), Value::String(provider_id));
        child_metadata.insert(META_MODEL_KEY.to_string(), Value::String(model.clone()));
        child_metadata.insert(META_CONTEXT_KEY.to_string(), request.context);

        let output = run_agent_execution(
            provider_instance.provider.as_ref(),
            &executor,
            AgentExecutionInput {
                user_content: request.task,
                session_key: format!("{}:subagent", parent_session),
                tool_metadata: child_metadata,
                model: Some(model),
            },
            AgentExecutionLimits {
                max_tool_iterations: self.sub_config.max_iterations.max(1),
                max_tool_calls: self.sub_config.max_tool_calls.max(1),
            },
        )
        .await
        .map_err(|err| match err {
            AgentExecutionError::Provider(inner) => {
                ToolError::ExecutionFailed(format!("sub-agent provider failed: {inner}"))
            }
            AgentExecutionError::ToolLoopExhausted => ToolError::ExecutionFailed(
                "sub-agent exceeded iteration/tool-call limits".to_string(),
            ),
        })?;

        Ok(ToolOutput {
            content_for_model: output.content.clone(),
            content_for_user: Some(output.content),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{ModelProviderConfig, ShellConfig, ToolsConfig, WebSearchConfig};

    fn test_config() -> Arc<AppConfig> {
        let mut providers = BTreeMap::new();
        providers.insert(
            "openai".to_string(),
            ModelProviderConfig {
                name: Some("OpenAI".to_string()),
                base_url: "https://api.openai.com/v1".to_string(),
                wire_api: "chat_completions".to_string(),
                default_model: "gpt-4o-mini".to_string(),
                api_key: Some("test".to_string()),
                env_key: None,
            },
        );
        Arc::new(AppConfig {
            model_provider: "openai".to_string(),
            model_providers: providers,
            tools: ToolsConfig {
                shell: ShellConfig::default(),
                web_search: WebSearchConfig::default(),
                sub_agent: SubAgentConfig {
                    enabled: true,
                    max_iterations: 6,
                    max_tool_calls: 12,
                    inherit_parent_tools: true,
                    exclude_tools: vec!["sub_agent".to_string()],
                },
            },
        })
    }

    #[test]
    fn parse_request_requires_task() {
        let err = SubAgentTool::parse_request(json!({})).expect_err("must fail");
        assert!(format!("{err}").contains("missing field"));
    }

    #[test]
    fn parse_request_requires_context() {
        let err = SubAgentTool::parse_request(json!({
            "task": "hello"
        }))
        .expect_err("must fail");
        assert!(format!("{err}").contains("missing field"));
    }

    #[test]
    fn parse_request_rejects_non_object_context() {
        let err = SubAgentTool::parse_request(json!({
            "task": "hello",
            "context": "invalid"
        }))
        .expect_err("must fail");
        assert!(format!("{err}").contains("`context` must be an object"));
    }

    #[test]
    fn parse_request_requires_context_session() {
        let err = SubAgentTool::parse_request(json!({
            "task": "hello",
            "context": {}
        }))
        .expect_err("must fail");
        assert!(format!("{err}").contains("`context.session`"));
    }

    #[test]
    fn parse_request_rejects_legacy_override_fields() {
        let err = SubAgentTool::parse_request(json!({
            "task": "hello",
            "context": { "session": "s1" },
            "model_provider": "openai"
        }))
        .expect_err("must fail");
        assert!(format!("{err}").contains("unknown field"));
    }

    #[test]
    fn resolve_provider_model_uses_parent_metadata() {
        let tool = SubAgentTool::new(test_config(), ToolRegistry::default());
        let mut metadata = BTreeMap::new();
        metadata.insert(META_PROVIDER_KEY.to_string(), Value::String("openai".to_string()));
        metadata.insert(META_MODEL_KEY.to_string(), Value::String("gpt-4.1".to_string()));
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata,
        };

        let (provider, model) = tool.resolve_provider_model(&ctx).unwrap();
        assert_eq!(provider, "openai");
        assert_eq!(model, "gpt-4.1");
    }

    #[test]
    fn resolve_provider_model_requires_parent_metadata() {
        let tool = SubAgentTool::new(test_config(), ToolRegistry::default());
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };

        let err = tool.resolve_provider_model(&ctx).expect_err("must fail");
        assert!(format!("{err}").contains("missing parent provider"));
    }
}
