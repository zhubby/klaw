use klaw_agent::build_provider_from_config;
use klaw_config::AppConfig;
use klaw_core::{
    AgentLoop, CircuitBreakerPolicy, DeadLetterMessage, DeadLetterPolicy, Envelope, EnvelopeHeader,
    ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker, InMemoryIdempotencyStore,
    InMemoryTransport, InboundMessage, OutboundMessage, QueueStrategy, RunLimits,
    SessionSchedulingPolicy, Subscription,
};
use klaw_storage::{open_default_store, ChatRecord, DefaultSessionStore, SessionStorage};
use klaw_tool::{
    MemoryTool, ShellTool, SubAgentTool, TerminalMultiplexerTool, ToolRegistry, WebFetchTool,
    WebSearchTool,
};
use std::{collections::BTreeMap, error::Error, io, sync::Arc, time::Duration};
use tracing::warn;

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
}

pub async fn build_runtime_bundle(config: &AppConfig) -> Result<RuntimeBundle, Box<dyn Error>> {
    let provider_instance = build_provider_from_config(config, &config.model_provider)
        .map_err(|err| config_err(err.to_string()))?;
    let session_store = open_default_store().await?;
    let mut tools = ToolRegistry::default();
    tools.register(ShellTool::new(config));
    tools.register(TerminalMultiplexerTool::new());
    if config.tools.memory.enabled {
        tools.register(MemoryTool::open_default(config).await?);
    }
    if config.tools.web_fetch.enabled {
        tools.register(WebFetchTool::new(config));
    }
    if config.tools.web_search.enabled {
        tools.register(WebSearchTool::new(config)?);
    }
    if config.tools.sub_agent.enabled {
        let parent_tools = tools.clone();
        tools.register(SubAgentTool::new(Arc::new(config.clone()), parent_tools));
    }

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
    })
}

pub async fn submit_and_get_output(
    runtime: &RuntimeBundle,
    input: String,
    session_key: String,
    chat_id: String,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
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

    let outcome = runtime
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
        .await?;

    let published = runtime.outbound_transport.published_messages().await;
    match published.last() {
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
            Ok(Some(msg.payload.content.clone()))
        }
        None => {
            warn!(error = ?outcome.error_code, "no outbound response produced");
            Ok(None)
        }
    }
}

fn config_err(message: String) -> Box<dyn Error> {
    Box::new(io::Error::other(message))
}
