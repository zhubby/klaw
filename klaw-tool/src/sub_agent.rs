use async_trait::async_trait;
use klaw_agent::{
    AgentExecutionError, AgentExecutionInput, AgentExecutionLimits, AgentExecutionOutput,
    ToolExecutor, ToolInvocationResult, ToolInvocationSignal, build_provider_from_config,
    run_agent_execution,
};
use klaw_config::{AppConfig, SubAgentConfig};
use klaw_llm::ToolDefinition;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{debug, info};
use uuid::Uuid;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput, ToolRegistry, ToolSignal};

const META_PROVIDER_KEY: &str = "agent.provider_id";
const META_MODEL_KEY: &str = "agent.model";
const META_PARENT_SESSION_KEY: &str = "sub_agent.parent_session_key";
const META_CONTEXT_KEY: &str = "sub_agent.context";
const TOOL_RESULT_LOG_LIMIT: usize = 4000;

pub struct SubAgentTool {
    config: Arc<AppConfig>,
    parent_tools: ToolRegistry,
    sub_config: SubAgentConfig,
    audit_sink: Option<Arc<dyn SubAgentAuditSink>>,
}

#[async_trait]
pub trait SubAgentAuditSink: Send + Sync {
    async fn persist_sub_agent_audits(
        &self,
        parent_session_key: &str,
        child_session_key: &str,
        output: &AgentExecutionOutput,
    ) -> Result<(), String>;
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubAgentRequest {
    task: String,
    #[serde(default)]
    context: Option<Value>,
}

impl SubAgentTool {
    pub fn new(config: Arc<AppConfig>, parent_tools: ToolRegistry) -> Self {
        Self::with_audit_sink(config, parent_tools, None)
    }

    pub fn with_audit_sink(
        config: Arc<AppConfig>,
        parent_tools: ToolRegistry,
        audit_sink: Option<Arc<dyn SubAgentAuditSink>>,
    ) -> Self {
        Self {
            sub_config: config.tools.sub_agent.clone(),
            config,
            parent_tools,
            audit_sink,
        }
    }

    fn parse_request(args: Value) -> Result<SubAgentRequest, ToolError> {
        let request: SubAgentRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        if request.task.trim().is_empty() {
            return Err(ToolError::InvalidArgs("`task` cannot be empty".to_string()));
        }
        if request
            .context
            .as_ref()
            .is_some_and(|context| !context.is_object())
        {
            return Err(ToolError::InvalidArgs(
                "`context` must be an object".to_string(),
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

    fn resolve_parent_session(ctx: &ToolContext) -> Result<String, ToolError> {
        let session_key = ctx.session_key.trim();
        if session_key.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "missing parent session key in tool context".to_string(),
            ));
        }
        Ok(session_key.to_string())
    }

    fn build_child_session_key(parent_session: &str) -> String {
        format!("{parent_session}:subagent:{}", Uuid::new_v4())
    }

    fn finalize_output(output: AgentExecutionOutput) -> Result<ToolOutput, ToolError> {
        if output.tool_signals.is_empty() {
            return Ok(ToolOutput {
                content_for_model: output.content.clone(),
                content_for_user: Some(output.content),
            });
        }

        let signal_kind = output
            .tool_signals
            .first()
            .map(|signal| signal.kind.as_str())
            .unwrap_or("sub_agent_signal");
        let (code, retryable, fallback_message) = match signal_kind {
            "approval_required" => (
                "approval_required",
                true,
                "sub-agent requested approval".to_string(),
            ),
            "stop" => (
                "stop_requested",
                false,
                "sub-agent requested stop".to_string(),
            ),
            _ => (
                "sub_agent_signaled",
                false,
                "sub-agent emitted tool signals".to_string(),
            ),
        };
        let message = if output.content.trim().is_empty() {
            fallback_message
        } else {
            output.content.clone()
        };
        let signal_kinds = output
            .tool_signals
            .iter()
            .map(|signal| signal.kind.clone())
            .collect::<Vec<_>>();
        let signals = output
            .tool_signals
            .into_iter()
            .map(|signal| ToolSignal {
                kind: signal.kind,
                payload: signal.payload,
            })
            .collect::<Vec<_>>();

        Err(ToolError::structured_execution_failed(
            message,
            code,
            Some(json!({
                "signal_kinds": signal_kinds,
                "sub_agent_output": output.content,
            })),
            retryable,
            signals,
        ))
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
    ) -> ToolInvocationResult {
        let Some(tool) = self.tools.get(name) else {
            return ToolInvocationResult::error(
                format!("tool `{name}` not found"),
                "tool_not_found".to_string(),
                None,
                false,
                Vec::new(),
            );
        };
        info!(tool = name, arguments = %arguments, "calling tool");
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
            Ok(output) => {
                debug!(
                    tool = name,
                    result = %truncate_for_log(&output.content_for_model, TOOL_RESULT_LOG_LIMIT),
                    "tool result"
                );
                ToolInvocationResult::success(output.content_for_model)
            }
            Err(err) => {
                let message = format!("tool `{name}` failed: {err}");
                let signals = err
                    .signals()
                    .iter()
                    .cloned()
                    .map(|signal| ToolInvocationSignal {
                        kind: signal.kind,
                        payload: signal.payload,
                    })
                    .collect::<Vec<_>>();
                debug!(
                    tool = name,
                    result = %truncate_for_log(&message, TOOL_RESULT_LOG_LIMIT),
                    "tool result"
                );
                ToolInvocationResult::error(
                    message,
                    err.code().to_string(),
                    err.details().cloned(),
                    err.retryable(),
                    signals,
                )
            }
        }
    }
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...[truncated]");
    truncated
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
                    "description": "Optional supplemental context object stored in child metadata as `sub_agent.context`. Do not include runtime session identifiers here.",
                    "additionalProperties": true
                }
            },
            "required": ["task"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Memory
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;
        let (provider_id, model) = self.resolve_provider_model(ctx)?;
        let parent_session = Self::resolve_parent_session(ctx)?;
        let provider_instance = build_provider_from_config(self.config.as_ref(), &provider_id)
            .map_err(|err| ToolError::ExecutionFailed(format!("build provider failed: {err}")))?;
        let child_tools = self.build_child_registry();
        let executor = RegistryToolExecutor {
            tools: &child_tools,
        };
        let child_context = request.context.unwrap_or_else(|| json!({}));
        let child_session_key = Self::build_child_session_key(&parent_session);

        let mut child_metadata = ctx.metadata.clone();
        child_metadata.insert(
            META_PARENT_SESSION_KEY.to_string(),
            Value::String(parent_session.clone()),
        );
        child_metadata.insert(META_PROVIDER_KEY.to_string(), Value::String(provider_id));
        child_metadata.insert(META_MODEL_KEY.to_string(), Value::String(model.clone()));
        child_metadata.insert(META_CONTEXT_KEY.to_string(), child_context);

        let output = run_agent_execution(
            provider_instance.provider.as_ref(),
            &executor,
            AgentExecutionInput {
                user_content: request.task,
                user_media: Vec::new(),
                conversation_history: Vec::new(),
                session_key: child_session_key.clone(),
                tool_metadata: child_metadata,
                model: Some(model),
            },
            AgentExecutionLimits {
                max_tool_iterations: self.sub_config.max_iterations.max(1),
                max_tool_calls: self.sub_config.max_tool_calls.max(1),
                token_budget: 0,
            },
            None,
        )
        .await
        .map_err(|err| match err {
            AgentExecutionError::Provider(inner) => {
                ToolError::ExecutionFailed(format!("sub-agent provider failed: {inner}"))
            }
            AgentExecutionError::ToolLoopExhausted => ToolError::ExecutionFailed(
                "sub-agent exceeded iteration/tool-call limits".to_string(),
            ),
            AgentExecutionError::BudgetExceeded { .. } => {
                ToolError::ExecutionFailed("sub-agent exceeded token budget".to_string())
            }
        })?;

        if let Some(audit_sink) = &self.audit_sink {
            if let Err(err) = audit_sink
                .persist_sub_agent_audits(&parent_session, &child_session_key, &output)
                .await
            {
                debug!(
                    parent_session = parent_session,
                    child_session = child_session_key,
                    error = %err,
                    "failed to persist sub-agent audit records"
                );
            }
        }

        Self::finalize_output(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_config::{ModelProviderConfig, ToolsConfig};

    fn test_config() -> Arc<AppConfig> {
        let mut providers = BTreeMap::new();
        providers.insert(
            "openai".to_string(),
            ModelProviderConfig {
                name: Some("OpenAI".to_string()),
                base_url: "https://api.openai.com/v1".to_string(),
                wire_api: "chat_completions".to_string(),
                default_model: "gpt-4o-mini".to_string(),
                tokenizer_path: None,
                proxy: false,
                stream: false,
                api_key: Some("test".to_string()),
                env_key: None,
            },
        );
        Arc::new(AppConfig {
            model_provider: "openai".to_string(),
            model_providers: providers,
            tools: ToolsConfig {
                sub_agent: SubAgentConfig {
                    enabled: true,
                    max_iterations: 6,
                    max_tool_calls: 12,
                    inherit_parent_tools: true,
                    exclude_tools: vec!["sub_agent".to_string()],
                },
                ..Default::default()
            },
            ..Default::default()
        })
    }

    #[test]
    fn parse_request_requires_task() {
        let err = SubAgentTool::parse_request(json!({})).expect_err("must fail");
        assert!(format!("{err}").contains("missing field"));
    }

    #[test]
    fn parse_request_allows_missing_context() {
        let request = SubAgentTool::parse_request(json!({
            "task": "hello"
        }))
        .expect("should parse without context");
        assert!(request.context.is_none());
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
    fn resolve_parent_session_uses_tool_context_session() {
        let ctx = ToolContext {
            session_key: "s1".to_string(),
            metadata: BTreeMap::new(),
        };

        let session = SubAgentTool::resolve_parent_session(&ctx).expect("should resolve session");
        assert_eq!(session, "s1");
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
        metadata.insert(
            META_PROVIDER_KEY.to_string(),
            Value::String("openai".to_string()),
        );
        metadata.insert(
            META_MODEL_KEY.to_string(),
            Value::String("gpt-4.1".to_string()),
        );
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

    #[test]
    fn build_child_session_key_creates_unique_child_scope() {
        let first = SubAgentTool::build_child_session_key("s1");
        let second = SubAgentTool::build_child_session_key("s1");
        assert!(first.starts_with("s1:subagent:"));
        assert!(second.starts_with("s1:subagent:"));
        assert_ne!(first, second);
    }

    #[test]
    fn finalize_output_returns_plain_content_without_signals() {
        let output = AgentExecutionOutput {
            content: "done".to_string(),
            reasoning: None,
            tool_signals: Vec::new(),
            request_usages: Vec::new(),
            request_audits: Vec::new(),
            tool_audits: Vec::new(),
        };

        let tool_output = SubAgentTool::finalize_output(output).expect("should succeed");
        assert_eq!(tool_output.content_for_model, "done");
        assert_eq!(tool_output.content_for_user.as_deref(), Some("done"));
    }

    #[test]
    fn finalize_output_propagates_approval_signal() {
        let output = AgentExecutionOutput {
            content: "waiting for approval".to_string(),
            reasoning: None,
            tool_signals: vec![ToolInvocationSignal {
                kind: "approval_required".to_string(),
                payload: json!({ "approval_id": "appr_123" }),
            }],
            request_usages: Vec::new(),
            request_audits: Vec::new(),
            tool_audits: Vec::new(),
        };

        let err = SubAgentTool::finalize_output(output).expect_err("signals should surface");
        assert_eq!(err.code(), "approval_required");
        assert!(err.retryable());
        assert!(
            err.signals()
                .iter()
                .any(|signal| signal.kind == "approval_required")
        );
    }

    #[test]
    fn finalize_output_propagates_stop_signal() {
        let output = AgentExecutionOutput {
            content: String::new(),
            reasoning: None,
            tool_signals: vec![ToolInvocationSignal {
                kind: "stop".to_string(),
                payload: json!({}),
            }],
            request_usages: Vec::new(),
            request_audits: Vec::new(),
            tool_audits: Vec::new(),
        };

        let err = SubAgentTool::finalize_output(output).expect_err("signals should surface");
        assert_eq!(err.code(), "stop_requested");
        assert!(!err.retryable());
        assert!(err.signals().iter().any(|signal| signal.kind == "stop"));
    }
}
