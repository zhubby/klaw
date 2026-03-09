use klaw_core::{
    AgentLoop, CircuitBreakerPolicy, DeadLetterMessage, DeadLetterPolicy, Envelope,
    EnvelopeHeader, ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker,
    InMemoryIdempotencyStore, InMemoryTransport, InboundMessage, OutboundMessage, QueueStrategy,
    RunLimits, SessionSchedulingPolicy, Subscription,
};
use klaw_llm::{EchoProvider, OpenAiCompatibleConfig, OpenAiCompatibleProvider};
use klaw_tool::{EchoTool, ToolRegistry};
use std::{collections::BTreeMap, env, sync::Arc, time::Duration};
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

pub fn build_runtime_bundle() -> RuntimeBundle {
    let provider = build_provider_from_env();
    let mut tools = ToolRegistry::default();
    tools.register(EchoTool);

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

    RuntimeBundle {
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
    }
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

fn build_provider_from_env() -> Arc<dyn klaw_llm::LlmProvider> {
    let base_url = env::var("OPENAI_BASE_URL").ok();
    let api_key = env::var("OPENAI_API_KEY").ok();
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    match (base_url, api_key) {
        (Some(base_url), Some(api_key)) => {
            info!("using OpenAI compatible provider");
            Arc::new(OpenAiCompatibleProvider::new(OpenAiCompatibleConfig {
                base_url,
                api_key,
                default_model: model,
            }))
        }
        _ => {
            warn!("OPENAI_BASE_URL/OPENAI_API_KEY not set, fallback to EchoProvider");
            Arc::new(EchoProvider)
        }
    }
}
