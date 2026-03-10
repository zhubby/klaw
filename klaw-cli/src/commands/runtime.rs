use klaw_core::{
    AgentLoop, CircuitBreakerPolicy, DeadLetterMessage, DeadLetterPolicy, Envelope,
    EnvelopeHeader, ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker,
    InMemoryIdempotencyStore, InMemoryTransport, InboundMessage, OutboundMessage, QueueStrategy,
    RunLimits, SessionSchedulingPolicy, Subscription,
};
use klaw_config::{AppConfig, ModelProviderConfig};
use klaw_llm::{OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use klaw_tool::{ShellTool, ToolRegistry};
use std::{collections::BTreeMap, env, error::Error, io, sync::Arc, time::Duration};
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
}

pub fn build_runtime_bundle(config: &AppConfig) -> Result<RuntimeBundle, Box<dyn Error>> {
    let provider = build_provider_from_config(config)?;
    let mut tools = ToolRegistry::default();
    tools.register(ShellTool::new());

    let runtime = AgentLoop::new(
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
        provider,
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
    })
}

pub async fn submit_and_get_output(
    runtime: &RuntimeBundle,
    input: String,
    session_key: String,
    chat_id: String,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    runtime
        .inbound_transport
        .enqueue(Envelope {
            header: EnvelopeHeader::new(session_key.clone()),
            metadata: BTreeMap::new(),
            payload: InboundMessage {
                channel: "stdio".to_string(),
                sender_id: "local-user".to_string(),
                chat_id,
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
        Some(msg) => Ok(Some(msg.payload.content.clone())),
        None => {
            warn!(error = ?outcome.error_code, "no outbound response produced");
            Ok(None)
        }
    }
}

fn build_provider_from_config(config: &AppConfig) -> Result<Arc<dyn klaw_llm::LlmProvider>, Box<dyn Error>> {
    let provider = config
        .model_providers
        .get(&config.model_provider)
        .ok_or_else(|| config_err(format!("model_provider '{}' not found", config.model_provider)))?;

    match provider.wire_api.as_str() {
        "chat_completions" | "responses" => {}
        other => {
            return Err(config_err(format!(
                "unsupported wire_api '{other}' for provider '{}'",
                config.model_provider
            )))
        }
    }

    let api_key = resolve_api_key(provider).ok_or_else(|| {
        config_err(format!(
            "provider '{}' requires api_key or env_key",
            config.model_provider
        ))
    })?;

    info!(provider_id = %config.model_provider, "using configured provider");

    Ok(Arc::new(OpenAiCompatibleProvider::new(
        OpenAiCompatibleConfig {
            base_url: provider.base_url.clone(),
            api_key,
            default_model: provider.default_model.clone(),
        },
    )))
}

fn resolve_api_key(provider: &ModelProviderConfig) -> Option<String> {
    provider.api_key.clone().or_else(|| {
        provider
            .env_key
            .as_ref()
            .and_then(|env_name| env::var(env_name).ok())
    })
}

fn config_err(message: String) -> Box<dyn Error> {
    Box::new(io::Error::other(message))
}
