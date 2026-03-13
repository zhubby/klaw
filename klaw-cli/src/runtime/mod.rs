pub mod service_loop;

use klaw_agent::build_provider_from_config;
use klaw_channel::{ChannelRequest, ChannelResponse, ChannelResult, ChannelRuntime};
use klaw_config::AppConfig;
use klaw_core::{
    load_or_create_system_prompt, AgentLoop, AgentRuntimeError, CircuitBreakerPolicy,
    DeadLetterMessage, DeadLetterPolicy, Envelope, EnvelopeHeader, ExponentialBackoffRetryPolicy,
    InMemoryCircuitBreaker, InMemoryIdempotencyStore, InMemoryTransport, InboundMessage,
    OutboundMessage, QueueStrategy, RunLimits, SessionSchedulingPolicy, Subscription,
    TransportError,
};
use klaw_heartbeat::{
    should_suppress_output, specs_from_config, CronHeartbeatScheduler, HeartbeatScheduler,
};
use klaw_mcp::{McpBootstrapHandle, McpBootstrapSummary, McpManager};
use klaw_skill::{open_default_skill_store, InstalledSkill, RegistrySource, SkillStore};
use klaw_storage::{
    open_default_store, ChatRecord, CronStorage, DefaultSessionStore, SessionStorage,
};
use klaw_tool::{
    CronManagerTool, FsTool, LocalSearchTool, MemoryTool, ShellTool, SkillsRegistryTool,
    SubAgentTool, TerminalMultiplexerTool, ToolRegistry, WebFetchTool, WebSearchTool,
};
use std::{collections::BTreeMap, error::Error, io, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Clone, Default)]
pub struct StartupReport {
    pub skill_names: Vec<String>,
    pub tool_names: Vec<String>,
    pub mcp_summary: Option<McpBootstrapSummary>,
}

pub struct RuntimeBundle {
    pub runtime: AgentLoop,
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
        let maybe_output = submit_and_get_output(
            self.runtime.as_ref(),
            request.channel,
            request.input,
            request.session_key,
            request.chat_id,
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

pub async fn build_runtime_bundle(config: &AppConfig) -> Result<RuntimeBundle, Box<dyn Error>> {
    info!(
        provider = %config.model_provider,
        "building runtime bundle"
    );
    let provider_instance = build_provider_from_config(config, &config.model_provider)
        .map_err(|err| config_err(err.to_string()))?;
    let session_store = open_default_store().await?;
    reconcile_heartbeats(config, &session_store)
        .await
        .map_err(|err| config_err(format!("heartbeat reconcile failed: {err}")))?;
    let mut tools = ToolRegistry::default();
    tools.register(FsTool::new());
    tools.register(ShellTool::new(config));
    tools.register(LocalSearchTool::new());
    tools.register(TerminalMultiplexerTool::new());
    tools.register(CronManagerTool::open_default().await?);
    if !config.skills.registries.is_empty() {
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
    } else {
        info!("skills registry tool disabled: no configured sources");
    }
    if config.tools.memory.enabled {
        tools.register(MemoryTool::open_default(config).await?);
    }
    if config.tools.web_fetch.enabled {
        tools.register(WebFetchTool::new(config));
    }
    if config.tools.web_search.enabled {
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
    if config.tools.sub_agent.enabled {
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

    let data_dir_system_prompt = load_data_dir_system_prompt().await;
    let loaded_skills = load_skills_system_prompt(config).await;
    let skill_names = loaded_skills.skill_names.clone();
    let system_prompt = compose_system_prompt(data_dir_system_prompt, loaded_skills.prompt);

    let runtime = AgentLoop::new_with_identity(
        RunLimits {
            max_tool_iterations: 8,
            max_tool_calls: 16,
            token_budget: 0,
            agent_timeout: Duration::from_secs(30),
            tool_timeout: Duration::from_secs(8),
        },
        SessionSchedulingPolicy {
            strategy: QueueStrategy::Collect,
            max_queue_depth: 32,
            lock_ttl: Duration::from_secs(15),
        },
        provider_instance.provider,
        config.model_provider.clone(),
        provider_instance.default_model.clone(),
        tools,
    )
    .with_system_prompt(system_prompt);

    info!(
        tool_count = runtime.tools.list().len(),
        "runtime bundle ready"
    );

    Ok(RuntimeBundle {
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
    guard
        .shutdown()
        .await
        .map_err(|err| config_err(format!("mcp shutdown failed: {err}")))?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct LoadedSkillsPrompt {
    prompt: Option<String>,
    skill_names: Vec<String>,
}

async fn load_skills_system_prompt(config: &AppConfig) -> LoadedSkillsPrompt {
    info!("loading local skills for system prompt");
    let store = match open_default_skill_store() {
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

    let skills = match store.load_all_skill_markdowns().await {
        Ok(items) => items,
        Err(err) => {
            warn!("failed to load local skills: {err}");
            return LoadedSkillsPrompt::default();
        }
    };

    if skills.is_empty() {
        info!("no local skills found for system prompt");
        return LoadedSkillsPrompt::default();
    }

    let skill_names: Vec<String> = skills.iter().map(|skill| skill.name.clone()).collect();
    info!(
        count = skill_names.len(),
        names = ?skill_names,
        "loaded local skills for system prompt"
    );

    let mut prompt = String::from(
        "You must follow the following loaded skill instructions when they are relevant to the user's request.\n",
    );
    for skill in skills {
        prompt.push_str("\n---\n");
        prompt.push_str("Skill: ");
        prompt.push_str(&skill.name);
        prompt.push('\n');
        prompt.push_str(&skill.content);
        if !prompt.ends_with('\n') {
            prompt.push('\n');
        }
    }
    LoadedSkillsPrompt {
        prompt: Some(prompt),
        skill_names,
    }
}

async fn load_data_dir_system_prompt() -> Option<String> {
    match load_or_create_system_prompt().await {
        Ok(prompt) => Some(prompt),
        Err(err) => {
            warn!("failed to load SYSTEM.md from data dir: {err}");
            None
        }
    }
}

fn compose_system_prompt(
    data_dir_system_prompt: Option<String>,
    runtime_system_prompt: Option<String>,
) -> Option<String> {
    let base = data_dir_system_prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let runtime = runtime_system_prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    match (base, runtime) {
        (Some(base), Some(runtime)) => Some(format!("{base}\n\n{runtime}")),
        (Some(base), None) => Some(base),
        (None, Some(runtime)) => Some(runtime),
        (None, None) => None,
    }
}

pub async fn submit_and_get_output(
    runtime: &RuntimeBundle,
    channel: String,
    input: String,
    session_key: String,
    chat_id: String,
) -> Result<Option<AssistantOutput>, Box<dyn std::error::Error>> {
    let header = EnvelopeHeader::new(session_key.clone());
    let user_record = ChatRecord::new("user", input.clone(), Some(header.message_id.to_string()));
    runtime
        .session_store
        .append_chat_record(&session_key, &user_record)
        .await?;
    runtime
        .session_store
        .touch_session(&session_key, &chat_id, &channel)
        .await?;

    runtime
        .inbound_transport
        .enqueue(Envelope {
            header,
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: channel.to_string(),
                sender_id: "local-user".to_string(),
                chat_id: chat_id.clone(),
                session_key,
                content: input,
                metadata: BTreeMap::new(),
            },
        })
        .await;

    let maybe_outbound = run_runtime_once(runtime).await?;
    match maybe_outbound {
        Some(msg) => {
            let agent_record = ChatRecord::new("assistant", msg.payload.content.clone(), None);
            runtime
                .session_store
                .append_chat_record(&msg.header.session_key, &agent_record)
                .await?;
            runtime
                .session_store
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
        runtime
            .session_store
            .append_chat_record(&msg.header.session_key, &agent_record)
            .await?;
        runtime
            .session_store
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
    use super::should_emit_outbound;
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
}
