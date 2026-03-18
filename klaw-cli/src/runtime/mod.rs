pub mod service_loop;

use klaw_agent::build_provider_from_config;
use klaw_approval::{
    ApprovalManager, ApprovalResolveDecision, ApprovalStatus, SqliteApprovalManager,
};
use klaw_channel::{ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime};
use klaw_config::{AppConfig, ConfigStore, ToolEnabled};
use klaw_core::{
    compose_runtime_prompt, ensure_workspace_prompt_templates, AgentLoop, AgentRuntimeError,
    CircuitBreakerPolicy, DeadLetterMessage, DeadLetterPolicy, Envelope, EnvelopeHeader,
    ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker, InMemoryIdempotencyStore,
    InMemoryTransport, InboundMessage, MediaReference, OutboundMessage, QueueStrategy, RunLimits,
    RuntimePromptInput, SessionSchedulingPolicy, SkillPromptEntry, Subscription, TransportError,
};
use klaw_heartbeat::{
    should_suppress_output, specs_from_config, CronHeartbeatScheduler, HeartbeatScheduler,
};
use klaw_mcp::{McpBootstrapHandle, McpBootstrapSummary, McpManager};
use klaw_session::{ChatRecord, SessionManager, SqliteSessionManager};
use klaw_skill::{
    open_default_skills_manager, InstalledSkill, RegistrySource, SkillSourceKind, SkillsManager,
};
use klaw_storage::{open_default_store, CronStorage, DefaultSessionStore};
use klaw_tool::{
    ApplyPatchTool, ApprovalTool, CronManagerTool, LocalSearchTool, MemoryTool, ShellTool,
    SkillsManagerTool, SkillsRegistryTool, SubAgentTool, TerminalMultiplexerTool, ToolContext,
    ToolRegistry, WebFetchTool, WebSearchTool,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
    sync::Arc,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{sync::Mutex, time::timeout};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct StartupReport {
    pub skill_names: Vec<String>,
    pub tool_names: Vec<String>,
    pub mcp_summary: Option<McpBootstrapSummary>,
}

pub struct RuntimeBundle {
    pub runtime: AgentLoop,
    pub default_provider_id: String,
    pub provider_default_models: BTreeMap<String, String>,
    pub disable_session_commands_for: BTreeSet<String>,
    pub inbound_transport: InMemoryTransport<InboundMessage>,
    pub outbound_transport: InMemoryTransport<OutboundMessage>,
    pub deadletter_transport: InMemoryTransport<DeadLetterMessage>,
    pub idempotency: InMemoryIdempotencyStore,
    pub retry_policy: ExponentialBackoffRetryPolicy,
    pub deadletter_policy: DeadLetterPolicy,
    pub circuit_breaker: InMemoryCircuitBreaker,
    pub subscription: Subscription,
    pub session_store: DefaultSessionStore,
    pub mcp_bootstrap: Option<Mutex<McpBootstrapHandle>>,
    pub startup_report: StartupReport,
}

pub struct SharedChannelRuntime {
    runtime: Arc<RuntimeBundle>,
    background: Arc<service_loop::BackgroundServices>,
}

impl SharedChannelRuntime {
    pub fn new(
        runtime: Arc<RuntimeBundle>,
        background: Arc<service_loop::BackgroundServices>,
    ) -> Self {
        Self {
            runtime,
            background,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl ChannelRuntime for SharedChannelRuntime {
    async fn submit(&self, request: ChannelRequest) -> ChannelResult<Option<ChannelResponse>> {
        if !is_channel_commands_disabled(self.runtime.as_ref(), &request.channel)
            && request.input.trim_start().starts_with('/')
        {
            if let Some(response) = handle_im_command(
                self.runtime.as_ref(),
                request.channel.clone(),
                request.session_key.clone(),
                request.chat_id.clone(),
                request.input.clone(),
            )
            .await?
            {
                return Ok(Some(response));
            }
        }

        let route = resolve_session_route(
            self.runtime.as_ref(),
            &request.channel,
            &request.session_key,
            &request.chat_id,
        )
        .await?;
        let maybe_output = submit_and_get_output(
            self.runtime.as_ref(),
            request.channel,
            request.input,
            route.active_session_key,
            request.chat_id,
            route.model_provider,
            route.model,
            request.media_references,
            request.metadata,
        )
        .await?;

        Ok(maybe_output.map(|output| ChannelResponse {
            content: output.content,
            reasoning: output.reasoning,
        }))
    }

    fn cron_tick_interval(&self) -> Duration {
        self.background.cron_tick_interval()
    }

    fn runtime_tick_interval(&self) -> Duration {
        self.background.runtime_tick_interval()
    }

    async fn on_cron_tick(&self) {
        self.background.on_cron_tick().await;
    }

    async fn on_runtime_tick(&self) {
        self.background.on_runtime_tick(self.runtime.as_ref()).await;
    }
}

#[derive(Debug, Clone)]
pub struct AssistantOutput {
    pub content: String,
    pub reasoning: Option<String>,
}

const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";
const META_PROVIDER_KEY: &str = "agent.provider_id";
const META_MODEL_KEY: &str = "agent.model";
const WORKSPACE_PROMPT_DOC_FILES: [&str; 7] = [
    "AGENTS.md",
    "BOOTSTRAP.md",
    "HEARTBEAT.md",
    "IDENTITY.md",
    "SOUL.md",
    "TOOLS.md",
    "USER.md",
];

const RUNTIME_PROMPT_RULES: &str = "Prefer lazy-loading context from files and skills instead of relying on embedded long prompt bodies. Read only what is needed for the current user request.";
const RUNTIME_PROMPT_EXTRA_INSTRUCTIONS: &str = "When local workspace docs are relevant, read them from disk on demand before acting. Do not assume their content without reading the files.";

#[derive(Debug, Clone)]
struct SessionRoute {
    active_session_key: String,
    model_provider: String,
    model: String,
}

fn is_channel_commands_disabled(runtime: &RuntimeBundle, channel: &str) -> bool {
    runtime.disable_session_commands_for.contains(channel)
}

fn parse_im_command(input: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let rest = trimmed.trim_start_matches('/').trim();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let command = parts.next()?.trim();
    let arg = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Some((command, arg))
}

fn first_arg_token(arg: Option<&str>) -> Option<&str> {
    arg.and_then(|raw| raw.split_whitespace().next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

async fn execute_approved_shell(
    runtime: &RuntimeBundle,
    approval_id: &str,
    session_key: &str,
    command_text: &str,
) -> Result<String, Box<dyn Error>> {
    let Some(shell_tool) = runtime.runtime.tools.get("shell") else {
        return Ok(
            "⚠️ shell tool unavailable; approval has been recorded but command was not executed."
                .to_string(),
        );
    };
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "shell.approval_id".to_string(),
        serde_json::Value::String(approval_id.to_string()),
    );
    let output = shell_tool
        .execute(
            json!({ "command": command_text }),
            &ToolContext {
                session_key: session_key.to_string(),
                metadata,
            },
        )
        .await;
    match output {
        Ok(output) => Ok(output
            .content_for_user
            .unwrap_or_else(|| output.content_for_model)),
        Err(err) => Ok(format!("tool `shell` failed: {err}")),
    }
}

async fn resolve_session_route(
    runtime: &RuntimeBundle,
    channel: &str,
    base_session_key: &str,
    chat_id: &str,
) -> Result<SessionRoute, Box<dyn Error>> {
    let sessions = session_manager(runtime);
    let base = sessions
        .get_or_create_session_state(
            base_session_key,
            chat_id,
            channel,
            &runtime.default_provider_id,
            default_model_for_provider(runtime, &runtime.default_provider_id),
        )
        .await?;
    let active_session_key = base
        .active_session_key
        .clone()
        .unwrap_or_else(|| base_session_key.to_string());
    let provider = base
        .model_provider
        .clone()
        .unwrap_or_else(|| runtime.default_provider_id.clone());
    let model_default = default_model_for_provider(runtime, &provider).to_string();
    let active = sessions
        .get_or_create_session_state(
            &active_session_key,
            chat_id,
            channel,
            &provider,
            &model_default,
        )
        .await?;
    let model_provider = active.model_provider.unwrap_or(provider);
    let model = active.model.unwrap_or(model_default);
    Ok(SessionRoute {
        active_session_key,
        model_provider,
        model,
    })
}

fn default_model_for_provider<'a>(runtime: &'a RuntimeBundle, provider_id: &str) -> &'a str {
    runtime
        .provider_default_models
        .get(provider_id)
        .map(String::as_str)
        .unwrap_or(runtime.runtime.active_model.as_str())
}

fn render_help_text(runtime: &RuntimeBundle) -> String {
    let mut lines = vec![
        "📘 **Command Center**".to_string(),
        String::new(),
        "```text".to_string(),
    ];
    lines.push(format!("{:<24}{}", "/new", "Start a new session context"));
    lines.push(format!("{:<24}{}", "/help", "Show this help"));
    if runtime.provider_default_models.len() > 1 {
        lines.push(format!(
            "{:<24}{}",
            "/model-provider", "List available providers"
        ));
        lines.push(format!(
            "{:<24}{}",
            "/model-provider <id>", "Switch provider for current session"
        ));
    }
    lines.push(format!("{:<24}{}", "/model", "Show current model"));
    lines.push(format!(
        "{:<24}{}",
        "/model <model_name>", "Update current model for current session"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/approve <approval_id>", "Approve a pending tool action"
    ));
    lines.push(format!(
        "{:<24}{}",
        "/reject <approval_id>", "Reject a pending tool action"
    ));
    lines.push("```".to_string());
    if runtime.provider_default_models.len() > 1 {
        let providers = runtime
            .provider_default_models
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(String::new());
        lines.push(format!("🧩 Providers: {providers}"));
    }
    lines.join("\n")
}

fn approval_manager(runtime: &RuntimeBundle) -> SqliteApprovalManager {
    SqliteApprovalManager::from_store(runtime.session_store.clone())
}

fn session_manager(runtime: &RuntimeBundle) -> SqliteSessionManager {
    SqliteSessionManager::from_store(runtime.session_store.clone())
}

async fn handle_im_command(
    runtime: &RuntimeBundle,
    channel: String,
    base_session_key: String,
    chat_id: String,
    input: String,
) -> Result<Option<ChannelResponse>, Box<dyn Error>> {
    let Some((command, arg)) = parse_im_command(&input) else {
        return Ok(None);
    };
    let route = resolve_session_route(runtime, &channel, &base_session_key, &chat_id).await?;
    let response = match command {
        "help" => ChannelResponse {
            content: render_help_text(runtime),
            reasoning: None,
        },
        "new" => {
            let new_session_key = format!("{base_session_key}:{}", Uuid::new_v4().simple());
            let sessions = session_manager(runtime);
            sessions
                .get_or_create_session_state(
                    &new_session_key,
                    &chat_id,
                    &channel,
                    &route.model_provider,
                    &route.model,
                )
                .await?;
            sessions
                .set_active_session(&base_session_key, &chat_id, &channel, &new_session_key)
                .await?;
            ChannelResponse {
                content: format!(
                    "🆕 **New session started**\n\n🧵 Session: `{new_session_key}`\n🧩 Provider: `{}`\n🤖 Model: `{}`",
                    route.model_provider, route.model
                ),
                reasoning: None,
            }
        }
        "model-provider" => {
            if runtime.provider_default_models.len() <= 1 && arg.is_none() {
                return Ok(Some(ChannelResponse {
                    content: "ℹ️ Only one provider is configured, so switching is not required."
                        .to_string(),
                    reasoning: None,
                }));
            }
            if let Some(provider_id) = first_arg_token(arg) {
                let Some(default_model) = runtime.provider_default_models.get(provider_id) else {
                    let all = runtime
                        .provider_default_models
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Ok(Some(ChannelResponse {
                        content: format!(
                            "❌ Unknown provider: `{provider_id}`\n🧩 Available: {all}"
                        ),
                        reasoning: None,
                    }));
                };
                let sessions = session_manager(runtime);
                sessions
                    .set_model_provider(
                        &route.active_session_key,
                        &chat_id,
                        &channel,
                        provider_id,
                        default_model,
                    )
                    .await?;
                sessions
                    .set_model_provider(
                        &base_session_key,
                        &chat_id,
                        &channel,
                        provider_id,
                        default_model,
                    )
                    .await?;
                ChannelResponse {
                    content: format!(
                        "✅ **Provider switched**\n\n🧩 Provider: `{provider_id}`\n🤖 Model: `{default_model}`"
                    ),
                    reasoning: None,
                }
            } else {
                let lines = runtime
                    .provider_default_models
                    .iter()
                    .map(|(id, model)| {
                        if id == &route.model_provider {
                            format!("• `{id}`  ← current (default: `{model}`)")
                        } else {
                            format!("• `{id}`  (default: `{model}`)")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                ChannelResponse {
                    content: format!("🧩 **Providers**\n\n{lines}"),
                    reasoning: None,
                }
            }
        }
        "model" => {
            if let Some(model) = first_arg_token(arg) {
                if model.trim().is_empty() {
                    return Ok(Some(ChannelResponse {
                        content: "❌ Model name cannot be empty.".to_string(),
                        reasoning: None,
                    }));
                }
                let sessions = session_manager(runtime);
                sessions
                    .set_model(&route.active_session_key, &chat_id, &channel, model)
                    .await?;
                sessions
                    .set_model(&base_session_key, &chat_id, &channel, model)
                    .await?;
                ChannelResponse {
                    content: format!(
                        "✅ **Model updated**\n\n🧩 Provider: `{}`\n🤖 Model: `{model}`",
                        route.model_provider
                    ),
                    reasoning: None,
                }
            } else {
                ChannelResponse {
                    content: format!(
                        "🤖 **Current model**\n\n🧩 Provider: `{}`\n🤖 Model: `{}`",
                        route.model_provider, route.model
                    ),
                    reasoning: None,
                }
            }
        }
        "approve" => {
            let Some(approval_id) = first_arg_token(arg) else {
                return Ok(Some(ChannelResponse {
                    content: "❌ Usage: `/approve <approval_id>`".to_string(),
                    reasoning: None,
                }));
            };
            let manager = approval_manager(runtime);
            let approval = match manager.get_approval(approval_id).await {
                Ok(approval) => approval,
                Err(_) => {
                    return Ok(Some(ChannelResponse {
                        content: format!("❌ Approval not found: `{approval_id}`"),
                        reasoning: None,
                    }));
                }
            };
            if approval.session_key != route.active_session_key
                && approval.session_key != base_session_key
            {
                return Ok(Some(ChannelResponse {
                    content: format!(
                        "❌ Approval `{approval_id}` does not belong to current session."
                    ),
                    reasoning: None,
                }));
            }
            match approval.status {
                ApprovalStatus::Pending => {
                    if approval.expires_at_ms < now_ms() {
                        let _ = manager
                            .resolve_approval(
                                approval_id,
                                ApprovalResolveDecision::Approve,
                                Some("channel-user"),
                                now_ms(),
                            )
                            .await?;
                        return Ok(Some(ChannelResponse {
                            content: format!("⌛ Approval expired: `{approval_id}`"),
                            reasoning: None,
                        }));
                    }
                    let approved = manager
                        .resolve_approval(
                            approval_id,
                            ApprovalResolveDecision::Approve,
                            Some("channel-user"),
                            now_ms(),
                        )
                        .await?
                        .approval;
                    if approved.tool_name != "shell" {
                        return Ok(Some(ChannelResponse {
                            content: format!(
                                "✅ Approval granted: `{}` (`{}`).\n\n请重试之前触发审批的操作。",
                                approved.id, approved.tool_name
                            ),
                            reasoning: None,
                        }));
                    }
                    let execution_result = execute_approved_shell(
                        runtime,
                        &approved.id,
                        &approved.session_key,
                        &approved.command_text,
                    )
                    .await?;
                    let model_followup_input = format!(
                        "审批已通过并已执行命令。请基于以下执行结果给出最终回复。\n\
                        要求：\n\
                        1) 先说明成功/失败\n\
                        2) 如果失败，指出最关键原因和下一步建议\n\
                        3) 不要再调用任何工具\n\n\
                        approval_id: {}\n\
                        command: {}\n\
                        shell_result:\n{}",
                        approved.id, approved.command_preview, execution_result
                    );
                    let maybe_output = submit_and_get_output(
                        runtime,
                        channel.clone(),
                        model_followup_input,
                        approved.session_key.clone(),
                        chat_id.clone(),
                        route.model_provider.clone(),
                        route.model.clone(),
                        Vec::new(),
                        BTreeMap::new(),
                    )
                    .await?;
                    match maybe_output {
                        Some(output) => ChannelResponse {
                            content: output.content,
                            reasoning: output.reasoning,
                        },
                        None => ChannelResponse {
                            content: format!(
                                "✅ **Approval granted and command executed**\n\n{}",
                                execution_result
                            ),
                            reasoning: None,
                        },
                    }
                }
                other => ChannelResponse {
                    content: format!(
                        "ℹ️ Approval `{approval_id}` is already `{}`.",
                        other.as_str()
                    ),
                    reasoning: None,
                },
            }
        }
        "reject" => {
            let Some(approval_id) = first_arg_token(arg) else {
                return Ok(Some(ChannelResponse {
                    content: "❌ Usage: `/reject <approval_id>`".to_string(),
                    reasoning: None,
                }));
            };
            let manager = approval_manager(runtime);
            let approval = match manager.get_approval(approval_id).await {
                Ok(approval) => approval,
                Err(_) => {
                    return Ok(Some(ChannelResponse {
                        content: format!("❌ Approval not found: `{approval_id}`"),
                        reasoning: None,
                    }));
                }
            };
            if approval.session_key != route.active_session_key
                && approval.session_key != base_session_key
            {
                return Ok(Some(ChannelResponse {
                    content: format!(
                        "❌ Approval `{approval_id}` does not belong to current session."
                    ),
                    reasoning: None,
                }));
            }
            match approval.status {
                ApprovalStatus::Pending => {
                    if approval.expires_at_ms < now_ms() {
                        let _ = manager
                            .resolve_approval(
                                approval_id,
                                ApprovalResolveDecision::Reject,
                                Some("channel-user"),
                                now_ms(),
                            )
                            .await?;
                        return Ok(Some(ChannelResponse {
                            content: format!("⌛ Approval expired: `{approval_id}`"),
                            reasoning: None,
                        }));
                    }
                    manager
                        .resolve_approval(
                            approval_id,
                            ApprovalResolveDecision::Reject,
                            Some("channel-user"),
                            now_ms(),
                        )
                        .await?;
                    ChannelResponse {
                        content: format!(
                            "🛑 Approval rejected: `{approval_id}` (`{}`).",
                            approval.tool_name
                        ),
                        reasoning: None,
                    }
                }
                other => ChannelResponse {
                    content: format!(
                        "ℹ️ Approval `{approval_id}` is already `{}`.",
                        other.as_str()
                    ),
                    reasoning: None,
                },
            }
        }
        other => {
            let help = render_help_text(runtime);
            ChannelResponse {
                content: format!("❌ Unknown command: `/{other}`\n\n{help}"),
                reasoning: None,
            }
        }
    };
    Ok(Some(response))
}

pub async fn build_runtime_bundle(config: &AppConfig) -> Result<RuntimeBundle, Box<dyn Error>> {
    info!(
        provider = %config.model_provider,
        "building runtime bundle"
    );
    let mut provider_registry = BTreeMap::new();
    let mut provider_default_models = BTreeMap::new();
    for provider_id in config.model_providers.keys() {
        match build_provider_from_config(config, provider_id) {
            Ok(instance) => {
                provider_default_models.insert(provider_id.clone(), instance.default_model.clone());
                provider_registry.insert(provider_id.clone(), instance.provider);
            }
            Err(err) => {
                warn!(
                    provider_id = provider_id.as_str(),
                    error = %err,
                    "provider is configured but unavailable at startup; skipping"
                );
            }
        }
    }
    let default_provider = provider_registry
        .get(&config.model_provider)
        .cloned()
        .ok_or_else(|| {
            config_err(format!(
                "default provider '{}' is missing",
                config.model_provider
            ))
        })?;
    let default_model = provider_default_models
        .get(&config.model_provider)
        .cloned()
        .ok_or_else(|| {
            config_err(format!(
                "default model for provider '{}' is missing",
                config.model_provider
            ))
        })?;
    let session_store = open_default_store().await?;
    reconcile_heartbeats(config, &session_store)
        .await
        .map_err(|err| config_err(format!("heartbeat reconcile failed: {err}")))?;
    let mut tools = ToolRegistry::default();
    if config.tools.apply_patch.enabled() {
        tools.register(ApplyPatchTool::new(config));
    }
    if config.tools.shell.enabled() {
        tools.register(ShellTool::with_store(config, session_store.clone()));
    }
    if config.tools.approval.enabled() {
        tools.register(ApprovalTool::with_manager(
            SqliteApprovalManager::from_store(session_store.clone()),
        ));
    }
    if config.tools.local_search.enabled() {
        tools.register(LocalSearchTool::new());
    }
    if config.tools.terminal_multiplexers.enabled() {
        tools.register(TerminalMultiplexerTool::new());
    }
    if config.tools.cron_manager.enabled() {
        tools.register(CronManagerTool::open_default().await?);
    }
    if config.tools.skills_registry.enabled() && !config.skills.registries.is_empty() {
        info!(
            sources = config.skills.registries.len(),
            source_names = ?config.skills.registries.keys().cloned().collect::<Vec<_>>(),
            "registering skills registry tool"
        );
        match SkillsRegistryTool::open_default(config) {
            Ok(tool) => tools.register(tool),
            Err(err) => {
                warn!("skills registry tool disabled: {err}");
            }
        }
    } else if !config.tools.skills_registry.enabled() {
        info!("skills registry tool disabled by config");
    } else {
        info!("skills registry tool disabled: no configured sources");
    }
    if config.tools.skills_manager.enabled() {
        match SkillsManagerTool::open_default(config) {
            Ok(tool) => tools.register(tool),
            Err(err) => {
                warn!("skills manager tool disabled: {err}");
            }
        }
    } else {
        info!("skills manager tool disabled by config");
    }
    if config.tools.memory.enabled() {
        tools.register(MemoryTool::open_default(config).await?);
    }
    if config.tools.web_fetch.enabled() {
        tools.register(WebFetchTool::new(config));
    }
    if config.tools.web_search.enabled() {
        tools.register(WebSearchTool::new(config)?);
    }

    let configured_mcp_servers = config
        .mcp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .count();
    info!(
        enabled = config.mcp.enabled,
        configured_servers = configured_mcp_servers,
        startup_timeout_seconds = config.mcp.startup_timeout_seconds,
        "bootstrapping mcp servers"
    );
    if config.tools.sub_agent.enabled() {
        let parent_tools = tools.clone();
        tools.register(SubAgentTool::new(Arc::new(config.clone()), parent_tools));
    }

    let mcp_bootstrap = if config.mcp.enabled && configured_mcp_servers > 0 {
        Some(Mutex::new(McpManager::spawn_bootstrap(
            config.mcp.clone(),
            tools.clone(),
        )))
    } else {
        None
    };

    ensure_workspace_prompt_templates_if_possible().await;
    let loaded_skills = load_skills_system_prompt(config).await;
    let skill_names = loaded_skills.skill_names.clone();
    let system_prompt = compose_runtime_prompt(RuntimePromptInput {
        runtime_metadata: None,
        rules: Some(RUNTIME_PROMPT_RULES.to_string()),
        local_docs: Some(format_workspace_docs_for_prompt()),
        additional_instructions: Some(RUNTIME_PROMPT_EXTRA_INSTRUCTIONS.to_string()),
        skills: loaded_skills.skill_entries,
    });

    let runtime = AgentLoop::new_with_identity(
        RunLimits {
            max_tool_iterations: 0,
            max_tool_calls: 0,
            token_budget: 0,
            agent_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(8),
        },
        SessionSchedulingPolicy {
            strategy: QueueStrategy::Collect,
            max_queue_depth: 32,
            lock_ttl: Duration::from_secs(15),
        },
        default_provider,
        config.model_provider.clone(),
        default_model,
        tools,
    )
    .with_provider_registry(provider_registry)
    .with_system_prompt(system_prompt);

    info!(
        tool_count = runtime.tools.list().len(),
        "runtime bundle ready"
    );

    Ok(RuntimeBundle {
        default_provider_id: config.model_provider.clone(),
        provider_default_models,
        disable_session_commands_for: config
            .channels
            .disable_session_commands_for
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect(),
        startup_report: StartupReport {
            skill_names,
            tool_names: runtime.tools.list(),
            mcp_summary: None,
        },
        runtime,
        inbound_transport: InMemoryTransport::new(),
        outbound_transport: InMemoryTransport::new(),
        deadletter_transport: InMemoryTransport::new(),
        idempotency: InMemoryIdempotencyStore::default(),
        retry_policy: ExponentialBackoffRetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(150),
            max_delay: Duration::from_secs(2),
            jitter_ratio: 0.0,
        },
        deadletter_policy: DeadLetterPolicy {
            topic: "agent.dlq",
            max_payload_bytes: 1024 * 1024,
            include_error_stack: false,
        },
        circuit_breaker: InMemoryCircuitBreaker::new(CircuitBreakerPolicy {
            failure_threshold: 5,
            open_interval: Duration::from_secs(3),
            half_open_max_requests: 1,
        }),
        subscription: Subscription {
            topic: "agent.inbound",
            consumer_group: "stdio".to_string(),
            visibility_timeout: Duration::from_secs(10),
        },
        session_store,
        mcp_bootstrap,
    })
}

pub async fn shutdown_runtime_bundle(runtime: &RuntimeBundle) -> Result<(), Box<dyn Error>> {
    let Some(handle) = &runtime.mcp_bootstrap else {
        return Ok(());
    };

    info!("shutting down runtime mcp servers");
    let mut guard = handle.lock().await;
    let shutdown_deadline = Duration::from_secs(2);
    match timeout(shutdown_deadline, guard.shutdown()).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            return Err(config_err(format!("mcp shutdown failed: {err}")));
        }
        Err(_) => {
            warn!(
                timeout_seconds = shutdown_deadline.as_secs(),
                "mcp shutdown timed out; continuing process exit"
            );
        }
    }
    Ok(())
}

pub async fn reload_runtime_skills_prompt(
    runtime: &RuntimeBundle,
) -> Result<Vec<String>, Box<dyn Error>> {
    let store = ConfigStore::open(None)?;
    let snapshot = store.snapshot();
    ensure_workspace_prompt_templates_if_possible().await;
    let loaded_skills = load_skills_system_prompt(&snapshot.config).await;
    let skill_names = loaded_skills.skill_names.clone();
    let system_prompt = compose_runtime_prompt(RuntimePromptInput {
        runtime_metadata: None,
        rules: Some(RUNTIME_PROMPT_RULES.to_string()),
        local_docs: Some(format_workspace_docs_for_prompt()),
        additional_instructions: Some(RUNTIME_PROMPT_EXTRA_INSTRUCTIONS.to_string()),
        skills: loaded_skills.skill_entries,
    });
    runtime.runtime.set_system_prompt(system_prompt);
    info!(skills = ?skill_names, "reloaded runtime skills prompt");
    Ok(skill_names)
}

#[derive(Debug, Clone, Default)]
struct LoadedSkillsPrompt {
    skill_entries: Vec<SkillPromptEntry>,
    skill_names: Vec<String>,
}

async fn load_skills_system_prompt(config: &AppConfig) -> LoadedSkillsPrompt {
    info!("loading local skills for system prompt");
    let store = match open_default_skills_manager() {
        Ok(store) => store,
        Err(err) => {
            warn!("failed to open default skill store: {err}");
            return LoadedSkillsPrompt::default();
        }
    };

    let sources: Vec<RegistrySource> = config
        .skills
        .registries
        .iter()
        .map(|(name, registry)| RegistrySource {
            name: name.clone(),
            address: registry.address.clone(),
        })
        .collect();
    let installed: Vec<InstalledSkill> = config
        .skills
        .registries
        .iter()
        .flat_map(|(registry_name, registry)| {
            registry.installed.iter().map(|skill_name| InstalledSkill {
                registry: registry_name.clone(),
                name: skill_name.clone(),
            })
        })
        .collect();
    match store
        .sync_registry_installed_skills(&sources, &installed, config.skills.sync_timeout)
        .await
    {
        Ok(report) => {
            info!(
                synced_registries = ?report.synced_registries,
                installed_skills = ?report.installed_skills,
                removed_skills = ?report.removed_skills,
                "registry skills sync completed"
            );
        }
        Err(err) => {
            warn!("failed to sync registry-installed skills: {err}");
        }
    }

    let skills = match store.load_all_installed_skill_markdowns().await {
        Ok(items) => items,
        Err(err) => {
            warn!("failed to load local skills: {err}");
            return LoadedSkillsPrompt::default();
        }
    };

    let skill_names: Vec<String> = skills.iter().map(|skill| skill.name.clone()).collect();
    info!(
        count = skill_names.len(),
        names = ?skill_names,
        "loaded local skills for runtime prompt shortlist"
    );

    let skill_entries = skills
        .into_iter()
        .map(|skill| SkillPromptEntry {
            name: skill.name,
            path: skill.local_path.display().to_string(),
            description: extract_skill_short_description(&skill.content),
            source: format_skill_source(&skill.source_kind, skill.registry.as_deref(), skill.stale),
        })
        .collect();

    LoadedSkillsPrompt {
        skill_entries,
        skill_names,
    }
}

fn extract_skill_short_description(markdown: &str) -> String {
    let line = markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_default();

    if line.is_empty() {
        return "no description".to_string();
    }

    const MAX_LEN: usize = 180;
    if line.chars().count() <= MAX_LEN {
        return line.to_string();
    }

    let mut trimmed = line.chars().take(MAX_LEN).collect::<String>();
    trimmed.push_str("...");
    trimmed
}

fn format_skill_source(
    source_kind: &SkillSourceKind,
    registry: Option<&str>,
    stale: Option<bool>,
) -> String {
    let mut source = match source_kind {
        SkillSourceKind::Local => "workspace/local".to_string(),
        SkillSourceKind::Registry => format!(
            "managed/{}",
            registry
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("registry")
        ),
    };
    if stale.unwrap_or(false) {
        source.push_str(" (stale)");
    }
    source
}

async fn ensure_workspace_prompt_templates_if_possible() {
    if let Err(err) = ensure_workspace_prompt_templates().await {
        warn!("failed to initialize workspace prompt templates: {err}");
    }
}

fn format_workspace_docs_for_prompt() -> String {
    let base = std::env::var("HOME")
        .map(|home| format!("{home}/.klaw/workspace"))
        .unwrap_or_else(|_| "~/.klaw/workspace".to_string());
    let docs = WORKSPACE_PROMPT_DOC_FILES
        .iter()
        .map(|name| format!("- {base}/{name}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Read these workspace docs on demand when relevant:\n{docs}")
}

pub async fn submit_and_get_output(
    runtime: &RuntimeBundle,
    channel: String,
    input: String,
    session_key: String,
    chat_id: String,
    model_provider: String,
    model: String,
    media_references: Vec<MediaReference>,
    request_metadata: BTreeMap<String, serde_json::Value>,
) -> Result<Option<AssistantOutput>, Box<dyn std::error::Error>> {
    let sessions = session_manager(runtime);
    let conversation_history = sessions.read_chat_records(&session_key).await?;
    let header = EnvelopeHeader::new(session_key.clone());
    let user_record = ChatRecord::new("user", input.clone(), Some(header.message_id.to_string()));
    sessions
        .append_chat_record(&session_key, &user_record)
        .await?;
    sessions
        .touch_session(&session_key, &chat_id, &channel)
        .await?;

    let inbound_payload = InboundMessage {
        channel: channel.to_string(),
        sender_id: "local-user".to_string(),
        chat_id: chat_id.clone(),
        session_key,
        content: input,
        media_references,
        metadata: {
            let mut metadata = request_metadata;
            metadata.insert(
                META_CONVERSATION_HISTORY_KEY.to_string(),
                json!(conversation_history
                    .into_iter()
                    .map(|record| {
                        json!({
                            "role": record.role,
                            "content": record.content,
                        })
                    })
                    .collect::<Vec<_>>()),
            );
            metadata.insert(
                META_PROVIDER_KEY.to_string(),
                serde_json::Value::String(model_provider),
            );
            metadata.insert(META_MODEL_KEY.to_string(), serde_json::Value::String(model));
            metadata
        },
    };
    info!(inbound = ?inbound_payload, "channel inbound normalized");

    runtime
        .inbound_transport
        .enqueue(Envelope {
            header,
            metadata: BTreeMap::new(),
            payload: inbound_payload,
        })
        .await;

    let maybe_outbound = run_runtime_once(runtime).await?;
    match maybe_outbound {
        Some(msg) => {
            let agent_record = ChatRecord::new("assistant", msg.payload.content.clone(), None);
            let sessions = session_manager(runtime);
            sessions
                .append_chat_record(&msg.header.session_key, &agent_record)
                .await?;
            sessions
                .complete_turn(
                    &msg.header.session_key,
                    &msg.payload.chat_id,
                    &msg.payload.channel,
                )
                .await?;
            let reasoning = msg
                .payload
                .metadata
                .get("reasoning")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .filter(|value| !value.trim().is_empty());
            Ok(Some(AssistantOutput {
                content: msg.payload.content.clone(),
                reasoning,
            }))
        }
        None => {
            warn!("no outbound response produced");
            Ok(None)
        }
    }
}

pub async fn drain_runtime_queue(
    runtime: &RuntimeBundle,
    max_iterations: usize,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut drained = 0usize;
    for _ in 0..max_iterations.max(1) {
        let maybe_outbound = run_runtime_once(runtime).await?;
        let Some(msg) = maybe_outbound else {
            break;
        };
        let agent_record = ChatRecord::new("assistant", msg.payload.content.clone(), None);
        let sessions = session_manager(runtime);
        sessions
            .append_chat_record(&msg.header.session_key, &agent_record)
            .await?;
        sessions
            .complete_turn(
                &msg.header.session_key,
                &msg.payload.chat_id,
                &msg.payload.channel,
            )
            .await?;
        drained += 1;
    }
    Ok(drained)
}

async fn run_runtime_once(
    runtime: &RuntimeBundle,
) -> Result<Option<Envelope<OutboundMessage>>, Box<dyn std::error::Error>> {
    let before_len = runtime.outbound_transport.published_messages().await.len();
    let result = runtime
        .runtime
        .run_once_reliable(
            &runtime.inbound_transport,
            &runtime.outbound_transport,
            &runtime.deadletter_transport,
            &runtime.subscription,
            &runtime.idempotency,
            &runtime.retry_policy,
            &runtime.deadletter_policy,
            &runtime.circuit_breaker,
        )
        .await;

    match result {
        Ok(_) => {
            let published = runtime.outbound_transport.published_messages().await;
            Ok(published
                .get(before_len)
                .cloned()
                .filter(should_emit_outbound))
        }
        Err(err) if is_queue_empty_error(&err) => Ok(None),
        Err(err) => Err(Box::new(err)),
    }
}

fn is_queue_empty_error(err: &AgentRuntimeError) -> bool {
    match err {
        AgentRuntimeError::Transport(TransportError::ConsumeFailed(message)) => {
            message.contains("queue empty")
        }
        _ => false,
    }
}

fn config_err(message: String) -> Box<dyn Error> {
    Box::new(io::Error::other(message))
}

pub async fn finalize_startup_report(
    runtime: &mut RuntimeBundle,
) -> Result<StartupReport, Box<dyn Error>> {
    let mcp_summary = match &runtime.mcp_bootstrap {
        Some(handle) => {
            let mut guard = handle.lock().await;
            if guard.is_ready() {
                Some(
                    guard
                        .wait_until_ready()
                        .await
                        .map_err(|err| config_err(format!("mcp bootstrap failed: {err}")))?,
                )
            } else {
                None
            }
        }
        None => None,
    };

    runtime.startup_report.tool_names = runtime.runtime.tools.list();
    runtime.startup_report.mcp_summary = mcp_summary;
    Ok(runtime.startup_report.clone())
}

async fn reconcile_heartbeats<S>(config: &AppConfig, storage: &S) -> Result<(), Box<dyn Error>>
where
    S: CronStorage + Send + Sync + Clone + 'static,
{
    let specs = specs_from_config(config)?;
    if specs.is_empty() {
        return Ok(());
    }

    let scheduler = CronHeartbeatScheduler::new(Arc::new(storage.clone()));
    scheduler.reconcile(&specs).await?;
    Ok(())
}

fn should_emit_outbound(msg: &Envelope<OutboundMessage>) -> bool {
    !should_suppress_output(&msg.payload.content, &msg.payload.metadata)
}

#[cfg(test)]
mod tests {
    use super::{first_arg_token, parse_im_command, should_emit_outbound};
    use klaw_core::{Envelope, EnvelopeHeader, OutboundMessage};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn silent_heartbeat_ack_is_filtered() {
        let msg = Envelope {
            header: EnvelopeHeader::new("stdio:test"),
            metadata: BTreeMap::new(),
            payload: OutboundMessage {
                channel: "stdio".to_string(),
                chat_id: "test".to_string(),
                content: " HEARTBEAT_OK ".to_string(),
                reply_to: None,
                metadata: BTreeMap::from([
                    ("trigger.kind".to_string(), json!("heartbeat")),
                    (
                        "heartbeat.silent_ack_token".to_string(),
                        json!("HEARTBEAT_OK"),
                    ),
                ]),
            },
        };

        assert!(!should_emit_outbound(&msg));
    }

    #[test]
    fn normal_messages_are_not_filtered() {
        let msg = Envelope {
            header: EnvelopeHeader::new("stdio:test"),
            metadata: BTreeMap::new(),
            payload: OutboundMessage {
                channel: "stdio".to_string(),
                chat_id: "test".to_string(),
                content: "Need action".to_string(),
                reply_to: None,
                metadata: BTreeMap::from([("trigger.kind".to_string(), json!("heartbeat"))]),
            },
        };

        assert!(should_emit_outbound(&msg));
    }

    #[test]
    fn parse_im_command_supports_name_and_optional_arg() {
        assert_eq!(parse_im_command("/help"), Some(("help", None)));
        assert_eq!(
            parse_im_command("/model-provider openai"),
            Some(("model-provider", Some("openai")))
        );
        assert_eq!(
            parse_im_command("/model qwen-plus /help"),
            Some(("model", Some("qwen-plus /help")))
        );
        assert_eq!(parse_im_command("hello"), None);
    }

    #[test]
    fn first_arg_token_uses_first_token_only() {
        assert_eq!(first_arg_token(Some("openai /help")), Some("openai"));
        assert_eq!(
            first_arg_token(Some("qwen-plus extra words")),
            Some("qwen-plus")
        );
        assert_eq!(first_arg_token(Some("   ")), None);
    }
}
