use async_trait::async_trait;
use klaw_core::{
    AgentLoop, AgentRunState, CircuitBreakerPolicy, DeadLetterPolicy, Envelope, EnvelopeHeader,
    ExponentialBackoffRetryPolicy, InMemoryCircuitBreaker, InMemoryIdempotencyStore,
    InMemoryTransport, InboundMessage, QueueStrategy, RunLimits, SessionSchedulingPolicy,
    Subscription,
};
use klaw_llm::{
    ChatOptions, EchoProvider, LlmError, LlmMessage, LlmProvider, LlmResponse, ToolDefinition,
};
use klaw_tool::ToolRegistry;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

const META_CONVERSATION_HISTORY_KEY: &str = "agent.conversation_history";

#[tokio::test]
async fn run_once_should_consume_and_publish() {
    let inbound_transport = InMemoryTransport::new();
    let outbound_transport = InMemoryTransport::new();
    let idempotency = InMemoryIdempotencyStore::default();

    let inbound = Envelope {
        header: EnvelopeHeader::new("mq:chat-1"),
        metadata: BTreeMap::new(),
        payload: InboundMessage {
            channel: "mq".to_string(),
            sender_id: "user-1".to_string(),
            chat_id: "chat-1".to_string(),
            session_key: "mq:chat-1".to_string(),
            content: "hello".to_string(),
            media_references: Vec::new(),
            metadata: BTreeMap::new(),
        },
    };
    inbound_transport.enqueue(inbound).await;

    let loop_runtime = AgentLoop::new(
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
        Arc::new(EchoProvider),
        ToolRegistry::default(),
    );

    let subscription = Subscription {
        topic: "agent.inbound",
        consumer_group: "test".to_string(),
        visibility_timeout: Duration::from_secs(10),
    };

    let outcome = loop_runtime
        .run_once(
            &inbound_transport,
            &outbound_transport,
            &subscription,
            &idempotency,
        )
        .await
        .expect("run_once should succeed");

    assert_eq!(outcome.final_state, AgentRunState::Completed);

    let published = outbound_transport.published_messages().await;
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].payload.content, "EchoProvider: hello");
}

#[tokio::test]
async fn in_memory_transport_publish_makes_message_consumable() {
    let transport = InMemoryTransport::new();
    let subscription = Subscription {
        topic: "agent.inbound",
        consumer_group: "test".to_string(),
        visibility_timeout: Duration::from_secs(10),
    };
    let inbound = Envelope {
        header: EnvelopeHeader::new("mq:published"),
        metadata: BTreeMap::new(),
        payload: InboundMessage {
            channel: "mq".to_string(),
            sender_id: "user-1".to_string(),
            chat_id: "published".to_string(),
            session_key: "mq:published".to_string(),
            content: "hello publish".to_string(),
            media_references: Vec::new(),
            metadata: BTreeMap::new(),
        },
    };

    klaw_core::transport::MessageTransport::publish(&transport, "agent.inbound", inbound.clone())
        .await
        .expect("publish should succeed");

    let published = transport.published_messages().await;
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].payload.content, "hello publish");

    let consumed = klaw_core::transport::MessageTransport::consume(&transport, &subscription)
        .await
        .expect("consume should read published message");
    assert_eq!(consumed.payload.payload.content, "hello publish");
}

struct FailingProvider;

#[async_trait]
impl LlmProvider for FailingProvider {
    fn name(&self) -> &str {
        "failing"
    }

    fn default_model(&self) -> &str {
        "none"
    }

    async fn chat(
        &self,
        _messages: Vec<LlmMessage>,
        _tools: Vec<ToolDefinition>,
        _model: Option<&str>,
        _options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        Err(LlmError::ProviderUnavailable(
            "simulated failure".to_string(),
        ))
    }
}

#[derive(Default)]
struct CaptureHistoryProvider;

#[async_trait]
impl LlmProvider for CaptureHistoryProvider {
    fn name(&self) -> &str {
        "capture-history"
    }

    fn default_model(&self) -> &str {
        "capture-history-v1"
    }

    async fn chat(
        &self,
        messages: Vec<LlmMessage>,
        _tools: Vec<ToolDefinition>,
        _model: Option<&str>,
        _options: ChatOptions,
    ) -> Result<LlmResponse, LlmError> {
        let summary: Vec<(&str, &str)> = messages
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("user", "previous user"),
                ("assistant", "previous assistant"),
                ("user", "current user"),
            ]
        );
        Ok(LlmResponse {
            content: "ok".to_string(),
            reasoning: None,
            tool_calls: Vec::new(),
        })
    }
}

#[tokio::test]
async fn run_once_includes_serialized_conversation_history_from_metadata() {
    let inbound_transport = InMemoryTransport::new();
    let outbound_transport = InMemoryTransport::new();
    let idempotency = InMemoryIdempotencyStore::default();

    let inbound = Envelope {
        header: EnvelopeHeader::new("mq:chat-history"),
        metadata: BTreeMap::new(),
        payload: InboundMessage {
            channel: "mq".to_string(),
            sender_id: "user-1".to_string(),
            chat_id: "chat-history".to_string(),
            session_key: "mq:chat-history".to_string(),
            content: "current user".to_string(),
            media_references: Vec::new(),
            metadata: BTreeMap::from([(
                META_CONVERSATION_HISTORY_KEY.to_string(),
                serde_json::json!([
                    {"role": "user", "content": "previous user"},
                    {"role": "assistant", "content": "previous assistant"},
                ]),
            )]),
        },
    };
    inbound_transport.enqueue(inbound).await;

    let loop_runtime = AgentLoop::new(
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
        Arc::new(CaptureHistoryProvider),
        ToolRegistry::default(),
    );

    let subscription = Subscription {
        topic: "agent.inbound",
        consumer_group: "test".to_string(),
        visibility_timeout: Duration::from_secs(10),
    };

    let outcome = loop_runtime
        .run_once(
            &inbound_transport,
            &outbound_transport,
            &subscription,
            &idempotency,
        )
        .await
        .expect("run_once should succeed");

    assert_eq!(outcome.final_state, AgentRunState::Completed);
}

#[tokio::test]
async fn run_once_reliable_should_send_to_dlq_after_retry_exhausted() {
    let inbound_transport = InMemoryTransport::new();
    let outbound_transport = InMemoryTransport::new();
    let deadletter_transport = InMemoryTransport::new();
    let idempotency = InMemoryIdempotencyStore::default();
    let retry_policy = ExponentialBackoffRetryPolicy {
        max_attempts: 2,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(3),
        jitter_ratio: 0.0,
    };
    let deadletter_policy = DeadLetterPolicy {
        topic: "agent.dlq",
        max_payload_bytes: 1024 * 1024,
        include_error_stack: false,
    };
    let circuit_breaker = InMemoryCircuitBreaker::new(CircuitBreakerPolicy {
        failure_threshold: 10,
        open_interval: Duration::from_secs(1),
        half_open_max_requests: 1,
    });

    let inbound = Envelope {
        header: EnvelopeHeader::new("mq:chat-2"),
        metadata: BTreeMap::new(),
        payload: InboundMessage {
            channel: "mq".to_string(),
            sender_id: "user-2".to_string(),
            chat_id: "chat-2".to_string(),
            session_key: "mq:chat-2".to_string(),
            content: "hello".to_string(),
            media_references: Vec::new(),
            metadata: BTreeMap::new(),
        },
    };
    inbound_transport.enqueue(inbound).await;

    let loop_runtime = AgentLoop::new(
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
        Arc::new(FailingProvider),
        ToolRegistry::default(),
    );

    let subscription = Subscription {
        topic: "agent.inbound",
        consumer_group: "test".to_string(),
        visibility_timeout: Duration::from_secs(10),
    };

    let outcome = loop_runtime
        .run_once_reliable(
            &inbound_transport,
            &outbound_transport,
            &deadletter_transport,
            &subscription,
            &idempotency,
            &retry_policy,
            &deadletter_policy,
            &circuit_breaker,
        )
        .await
        .expect("run_once_reliable should complete");

    assert_eq!(
        outcome.error_code,
        Some(klaw_core::ErrorCode::SentToDeadLetter)
    );
    assert_eq!(outbound_transport.published_messages().await.len(), 0);
    assert_eq!(deadletter_transport.published_messages().await.len(), 1);
}
