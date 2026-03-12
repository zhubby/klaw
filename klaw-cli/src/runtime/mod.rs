pub mod service_loop;

use klaw_agent::build_provider_from_config;
use klaw_config::AppConfig;
use klaw_core::{
    AgentLoop, AgentRuntimeError, CircuitBreakerPolicy, DeadLetterMessage, DeadLetterPolicy,
    Envelope, EnvelopeHeader, ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker,
    InMemoryIdempotencyStore, InMemoryTransport, InboundMessage, OutboundMessage, QueueStrategy,
    RunLimits, SessionSchedulingPolicy, Subscription, TransportError,
};
use klaw_mcp::{McpClientHub, McpManager, McpProxyTool, McpRuntimeHandles, McpToolDescriptor};
use klaw_skill::{open_default_skill_store, InstalledSkill, RegistrySource, SkillStore};
use klaw_storage::{open_default_store, ChatRecord, DefaultSessionStore, SessionStorage};
use klaw_tool::{
    CronManagerTool, FsTool, LocalSearchTool, MemoryTool, ShellTool, SkillsRegistryTool,
    SubAgentTool, TerminalMultiplexerTool, ToolRegistry, WebFetchTool, WebSearchTool,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
    sync::Arc,
    time::Duration,
};
use tracing::{info, warn};

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
    pub _mcp_hub: Option<Arc<McpClientHub>>,
    pub _mcp_runtime_handles: Option<McpRuntimeHandles>,
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

    let mut mcp_hub = None;
    let mut mcp_runtime_handles = None;
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
    let mut mcp_bootstrap = McpManager::bootstrap(&config.mcp).await;
    if !mcp_bootstrap.failures.is_empty() {
        for failure in &mcp_bootstrap.failures {
            warn!(
                server = %failure.server_id,
                reason = %failure.reason,
                "mcp server bootstrap failed, skipping"
            );
        }
    }
    if mcp_bootstrap.descriptors.is_empty() {
        info!("mcp bootstrap completed with no discovered tools");
    }
    if !mcp_bootstrap.descriptors.is_empty() {
        let mut blocked_servers = BTreeSet::new();
        let mut existing_names: BTreeSet<String> = tools.list().into_iter().collect();
        let mut by_server: BTreeMap<String, Vec<McpToolDescriptor>> = BTreeMap::new();
        for descriptor in mcp_bootstrap.descriptors {
            by_server
                .entry(descriptor.server_id.clone())
                .or_default()
                .push(descriptor);
        }

        for (server_id, descriptors) in &by_server {
            if descriptors
                .iter()
                .any(|descriptor| existing_names.contains(&descriptor.name))
            {
                blocked_servers.insert(server_id.clone());
                warn!(
                    server = %server_id,
                    "mcp server skipped due to tool name conflict with existing registry"
                );
                continue;
            }
            for descriptor in descriptors {
                existing_names.insert(descriptor.name.clone());
            }
        }

        for server_id in &blocked_servers {
            mcp_bootstrap.hub.remove(server_id);
        }
        mcp_bootstrap
            .runtime_handles
            .stdio_servers
            .retain(|server_id| !blocked_servers.contains(server_id));

        if !mcp_bootstrap.hub.server_ids().is_empty() {
            let hub = Arc::new(mcp_bootstrap.hub.clone());
            let active_servers = hub.server_ids();
            let total_mcp_tools: usize = by_server
                .iter()
                .filter(|(server_id, _)| !blocked_servers.contains(*server_id))
                .map(|(_, descriptors)| descriptors.len())
                .sum();
            for (server_id, descriptors) in by_server {
                if blocked_servers.contains(&server_id) {
                    continue;
                }
                for descriptor in descriptors {
                    tools.register(McpProxyTool::new(descriptor, Arc::clone(&hub)));
                }
            }
            mcp_hub = Some(hub);
            mcp_runtime_handles = Some(mcp_bootstrap.runtime_handles);
            info!(
                active_servers = ?active_servers,
                server_count = active_servers.len(),
                tool_count = total_mcp_tools,
                "mcp tools registered"
            );
        } else {
            info!("mcp bootstrap completed with no active servers");
        }
    }

    if config.tools.sub_agent.enabled {
        let parent_tools = tools.clone();
        tools.register(SubAgentTool::new(Arc::new(config.clone()), parent_tools));
    }

    let system_prompt = load_skills_system_prompt(config).await;

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
        _mcp_hub: mcp_hub,
        _mcp_runtime_handles: mcp_runtime_handles,
    })
}

async fn load_skills_system_prompt(config: &AppConfig) -> Option<String> {
    info!("loading local skills for system prompt");
    let store = match open_default_skill_store() {
        Ok(store) => store,
        Err(err) => {
            warn!("failed to open default skill store: {err}");
            return None;
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
            return None;
        }
    };

    if skills.is_empty() {
        info!("no local skills found for system prompt");
        return None;
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
    Some(prompt)
}

pub async fn submit_and_get_output(
    runtime: &RuntimeBundle,
    input: String,
    session_key: String,
    chat_id: String,
) -> Result<Option<AssistantOutput>, Box<dyn std::error::Error>> {
    let channel = "stdio";
    let header = EnvelopeHeader::new(session_key.clone());
    let user_record = ChatRecord::new("user", input.clone(), Some(header.message_id.to_string()));
    runtime
        .session_store
        .append_chat_record(&session_key, &user_record)
        .await?;
    runtime
        .session_store
        .touch_session(&session_key, &chat_id, channel)
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
            Ok(published.get(before_len).cloned())
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
