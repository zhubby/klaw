# Klaw Agent Loop — Language-Agnostic Technical Specification

> **Version**: 1.0.0 · **Date**: 2026-05-26
> **Source**: klaw v0.18.3 (`klaw-core`, `klaw-agent`, `klaw-llm`, `klaw-tool`)
> **Purpose**: Enable faithful reproduction of the Klaw agent runtime in any programming language.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Layer Model](#2-layer-model)
3. [Protocol Layer — Envelope & Message Types](#3-protocol-layer)
4. [Transport Layer — Message Delivery](#4-transport-layer)
5. [Reliability Layer — Retry, Circuit Breaker, Idempotency](#5-reliability-layer)
6. [Scheduler Layer — Session Serialization](#6-scheduler-layer)
7. [Agent Execution Kernel — The Inner Tool Loop](#7-agent-execution-kernel)
8. [Agent Loop Orchestrator — The Outer Runtime](#8-agent-loop-orchestrator)
9. [LLM Provider Abstraction](#9-llm-provider-abstraction)
10. [Tool System Abstraction](#10-tool-system-abstraction)
11. [Context Compression](#11-context-compression)
12. [Observability & Telemetry](#12-observability--telemetry)
13. [State Machine — Formal Definition](#13-state-machine)
14. [Data Flow Diagrams](#14-data-flow-diagrams)
15. [Configuration Schema](#15-configuration-schema)
16. [Error Handling Matrix](#16-error-handling-matrix)
17. [Implementation Checklist](#17-implementation-checklist)

---

## 1. Architecture Overview

Klaw is a **reliable, observable, session-scoped agent runtime** built around a two-layer execution model:

```
┌─────────────────────────────────────────────────────────────────┐
│                     Agent Loop (Outer Runtime)                  │
│                                                                 │
│  InboundMessage ─→ Dedup ─→ CircuitBreaker ─→ RetryPolicy ─┐   │
│                                                             │   │
│  ┌──────────────────────────────────────────────────────┐   │   │
│  │              Agent Execution Kernel (Inner Loop)      │   │   │
│  │                                                       │   │   │
│  │  [system_prompt] + [history] + [user_message]         │   │   │
│  │         │                                             │   │   │
│  │         ▼                                             │   │   │
│  │    ┌──────────┐     ┌───────────┐     ┌──────────┐   │   │   │
│  │    │   LLM    │────▶│ Tool Exec │────▶│  Tool    │   │   │   │
│  │    │ Provider │◀────│  Router   │◀────│  Result  │   │   │   │
│  │    └──────────┘     └───────────┘     └──────────┘   │   │   │
│  │         │              iteration loop                  │   │   │
│  │         ▼                                             │   │   │
│  │    Final Response (or signal short-circuit)            │   │   │
│  └──────────────────────────────────────────────────────┘   │   │
│                                                             │   │
│  OutboundMessage ◀── Publish ── Idempotency ── ACK ◀───────┘   │
│                                                                 │
│  On failure: DeadLetter ◀── DLQ Transport                      │
└─────────────────────────────────────────────────────────────────┘
```

### Core Design Principles

| Principle | Implementation |
|-----------|---------------|
| **Session serialization** | Same `session_key` → strictly sequential processing |
| **At-least-once delivery** | Idempotency keys + transport ACK/NACK |
| **Graceful degradation** | Tool timeout → retry → fallback → DLQ |
| **Separation of concerns** | Protocol / Transport / Reliability / Execution are independent layers |
| **Provider agnostic** | Any LLM behind a unified `LlmProvider` trait |
| **Tool extensibility** | Registry pattern; tools are self-describing with JSON Schema params |

---

## 2. Layer Model

The system is organized into **six independent layers**, each with clear contracts:

```
┌──────────────────────────────────────────────────────┐
│  L6: Presentation  (CLI / TUI / GUI / WebUI)         │  ← out of scope
├──────────────────────────────────────────────────────┤
│  L5: Channels      (Terminal / Telegram / WebSocket) │  ← adapters
├──────────────────────────────────────────────────────┤
│  L4: Agent Loop    (Outer orchestrator)              │  ★ THIS SPEC
│      - State machine, telemetry, provider routing    │
├──────────────────────────────────────────────────────┤
│  L3: Agent Kernel  (Inner execution loop)            │  ★ THIS SPEC
│      - LLM ↔ Tool iteration, budget guards           │
├──────────────────────────────────────────────────────┤
│  L2: Infrastructure (Transport, Reliability, Sched)  │  ★ THIS SPEC
│      - MQ, retry, circuit breaker, idempotency       │
├──────────────────────────────────────────────────────┤
│  L1: Protocol      (Envelope, ErrorCode, Topics)     │  ★ THIS SPEC
│      - Message format, versioning, error taxonomy    │
└──────────────────────────────────────────────────────┘
```

Each layer depends only on the layer below it through **trait/protocol boundaries**, making every layer independently testable and replaceable.

---

## 3. Protocol Layer

### 3.1 Envelope Structure

Every message in the system is wrapped in a generic `Envelope<T>`:

```
Envelope<T> {
    header:   EnvelopeHeader     // routing, tracing, retry metadata
    metadata: Map<String, JSON>  // business-level extensions
    payload:  T                  // typed payload (Inbound | Outbound | DeadLetter | Event)
}
```

#### EnvelopeHeader

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `message_id` | UUID | ✅ | Globally unique message identifier |
| `trace_id` | UUID | ✅ | End-to-end distributed trace ID |
| `session_key` | String | ✅ | Session serialization key (e.g., `"telegram:chat-123"`) |
| `timestamp` | DateTime | ✅ | Message creation time |
| `attempt` | u32 | ✅ | Current retry attempt (starts at 1) |
| `schema_version` | SchemaVersion | ✅ | Protocol version `{major: u16, minor: u16}` |
| `tenant_id` | String? | ❌ | Multi-tenant isolation |
| `namespace` | String? | ❌ | Environment/domain routing |
| `priority` | u8? | ❌ | Queue priority (higher = more urgent) |
| `ttl_ms` | u64? | ❌ | Time-to-live in milliseconds |
| `routing_hints` | Map<String, JSON> | ✅ | Provider-specific routing metadata |

#### SchemaVersion

```
SchemaVersion { major: u16, minor: u16 }
```

**Evolution rule** (SemVer):
- Same `major` + `to.minor >= from.minor` → backward compatible
- Consumers MUST ignore unknown fields
- Breaking changes require `major` increment

### 3.2 Message Types

#### InboundMessage

```
InboundMessage {
    channel:          String            // "terminal" | "telegram" | "discord" | ...
    sender_id:        String            // user/bot identifier
    chat_id:          String            // conversation identifier
    session_key:      String            // "{channel}:{chat_id}"
    content:          String            // user text input
    media_references: [MediaReference]  // attachments (images, files)
    metadata:         Map<String, JSON> // provider/model overrides, conversation history
}
```

**Key metadata keys**:

| Key | Type | Purpose |
|-----|------|---------|
| `agent.system_prompt` | String | Override system prompt |
| `agent.tool_choice` | JSON | Model tool_choice parameter |
| `agent.provider_id` | String | Target LLM provider |
| `agent.model` | String | Target model name |
| `agent.conversation_history` | [ConversationMessage] | Prior conversation context |
| `agent.resume_turn` | bool | Allow empty content (continuation) |

#### OutboundMessage

```
OutboundMessage {
    channel:   String             // target delivery channel
    chat_id:   String             // target conversation
    content:   String             // response text
    reply_to:  String?            // threading (optional)
    metadata:  Map<String, JSON>  // disposition, usage records, signals
}
```

**Key metadata keys in outbound**:

| Key | Type | Purpose |
|-----|------|---------|
| `agent.disposition` | String | `"final_message"` \| `"approval_required"` \| `"stopped"` |
| `llm.usage.records` | [JSON] | Token usage per LLM request |
| `llm.audit.records` | [JSON] | Full request/response audit trail |
| `turn.stopped` | bool | Whether a stop signal was received |
| `turn.stop_signal` | JSON | Stop signal details |
| `approval.required` | bool | Whether approval is pending |
| `approval.id` | String | Approval request identifier |
| `im.card` | JSON | Interactive card for UI rendering |

#### DeadLetterMessage

```
DeadLetterMessage {
    original_message_id: String
    session_key:         String
    final_error:         String
    attempts:            u32
    reason:              String
    original_payload:    InboundMessage
}
```

### 3.3 Logical Topics

| Topic | Direction | Purpose |
|-------|-----------|---------|
| `agent.inbound` | → Agent | User messages entering the system |
| `agent.outbound` | Agent → | Final responses to users |
| `agent.events` | Agent → | Intermediate events (streaming, progress) |
| `agent.dlq` | Agent → | Permanently failed messages |

### 3.4 Error Codes

```
enum ErrorCode {
    // Validation (non-retryable)
    InvalidSchema            // Envelope structure mismatch
    ValidationFailed         // Business rule violation

    // Idempotency (non-retryable)
    DuplicateMessage         // Already processed

    // Scheduling (retryable)
    SessionBusy              // Session concurrency limit

    // Runtime (retryable)
    AgentTimeout             // Total execution timeout
    ToolTimeout              // Single tool timeout

    // Dependency (mixed)
    ProviderUnavailable      // LLM service down (retryable)
    ProviderResponseInvalid  // Bad response format (non-retryable)

    // Infrastructure (retryable)
    TransportUnavailable     // Message broker failure

    // Reliability (terminal)
    RetryExhausted           // All retries consumed
    BudgetExceeded           // Token budget overrun
    SentToDeadLetter         // Moved to DLQ
}
```

**Error classification for retry**:

| Error Kind | Bucket | Default Retry Decision |
|-----------|--------|----------------------|
| `validation`, `schema`, `duplicate` | Non-retryable | `Abort` |
| `provider_unavailable`, `transport_unavailable`, `tool_timeout` | Transient | `RetryAfter(backoff)` |
| Everything else | Unknown | `RetryNow` |

---

## 4. Transport Layer

### 4.1 Delivery Modes

```
enum DeliveryMode {
    AtLeastOnce   // May duplicate, never lose — requires idempotency
    AtMostOnce    // May lose, never duplicate — fire-and-forget
    ExactlyOnce   // Precisely once — requires transactional semantics
}
```

**Recommended default**: `AtLeastOnce` with idempotency dedup.

### 4.2 Subscription

```
Subscription {
    topic:              String     // e.g., "agent.inbound"
    consumer_group:     String     // load balancing group
    visibility_timeout: Duration   // message re-delivery deadline
}
```

### 4.3 Transport Message

```
TransportMessage<T> {
    payload:    Envelope<T>         // The wrapped business message
    ack_handle: TransportAckHandle  // For ACK/NACK operations
}

TransportAckHandle {
    broker_message_id: String  // Broker-level message identifier
    delivery_attempt:  u32     // Current delivery attempt
}
```

### 4.4 Transport Trait (Interface)

```
interface MessageTransport<T> {
    mode() -> DeliveryMode

    publish(topic: String, msg: Envelope<T>) -> Result<(), TransportError>

    consume(subscription: Subscription) -> Result<TransportMessage<T>, TransportError>

    ack(handle: TransportAckHandle) -> Result<(), TransportError>

    nack(handle: TransportAckHandle, requeue_after: Duration?) -> Result<(), TransportError>

    requeue(handle: TransportAckHandle, delay: Duration) -> Result<(), TransportError>
        // default: nack(handle, Some(delay))
}
```

### 4.5 Transport Errors

```
enum TransportError {
    Unavailable(String)
    PublishFailed(String)
    ConsumeFailed(String)
    AckFailed(String)
    NackFailed(String)
}
```

### 4.6 Reference Implementation: In-Memory Transport

For testing and development, an in-memory transport uses:
- A channel/queue per topic
- Immediate delivery semantics
- `AtMostOnce` delivery (no persistence)

---

## 5. Reliability Layer

### 5.1 Retry Policy

#### RetryDecision

```
enum RetryDecision {
    RetryNow                    // Retry immediately
    RetryAfter(Duration)        // Retry after delay
    SendToDeadLetter            // Move to DLQ
    Abort                       // Stop processing, no DLQ
}
```

#### RetryPolicy Interface

```
interface RetryPolicy {
    max_attempts() -> u32
    classify(error_kind: String, attempt: u32) -> RetryDecision
}
```

#### Exponential Backoff Implementation

```
ExponentialBackoffRetryPolicy {
    max_attempts:  u32       // e.g., 5
    base_delay:    Duration  // e.g., 200ms
    max_delay:     Duration  // e.g., 30s
    jitter_ratio:  f32       // e.g., 0.2 (reserved, not yet randomized)
}
```

**Delay formula**:

```
delay = min(base_delay * 2^(attempt - 1), max_delay)
```

Example with `base_delay=200ms`, `max_delay=30s`:

| Attempt | Delay |
|---------|-------|
| 1 | 200ms |
| 2 | 400ms |
| 3 | 800ms |
| 4 | 1.6s |
| 5 | 3.2s |
| 6 | 6.4s |
| 7 | 12.8s |
| 8+ | 25.6s → capped at 30s |

**Classification logic**:

```
function classify(error_kind, attempt):
    if attempt >= max_attempts:
        return SendToDeadLetter

    switch error_kind:
        case "validation" | "schema" | "duplicate":
            return Abort
        case "provider_unavailable" | "transport_unavailable" | "tool_timeout":
            return RetryAfter(delay_for(attempt))
        default:
            return RetryNow
```

### 5.2 Idempotency Store

```
interface IdempotencyStore {
    seen(key: String) -> bool
    mark_seen(key: String, ttl: Duration) -> void
    clear(key: String) -> void
}
```

**Key format**: `{message_id}:{session_key}:{stage}`

- `stage` values: `"ingress"`, `"agent_run"`, `"egress"`
- TTL should be ≥ `agent_timeout + lock_ttl`

**Workflow**:
1. Before processing: `seen(key)` → if `true`, skip (return `DuplicateMessage`)
2. After success: `mark_seen(key, ttl)`
3. On re-delivery: `seen(key)` → `true` → short-circuit

### 5.3 Circuit Breaker

```
interface CircuitBreaker {
    allow_request() -> bool    // Is the circuit closed/half-open?
    on_success() -> void       // Record success, reset failure count
    on_failure() -> void       // Record failure, may open circuit
}
```

#### CircuitBreakerPolicy

```
CircuitBreakerPolicy {
    failure_threshold:      u32       // e.g., 5 consecutive failures
    open_interval:          Duration  // e.g., 60s — how long circuit stays open
    half_open_max_requests: u32       // e.g., 3 — probe requests in half-open
}
```

#### State Machine

```
        ┌─────────────────────────────────────────┐
        │                                         │
        ▼                                         │
   ┌─────────┐   failures >= threshold    ┌──────────┐
   │ CLOSED  │ ──────────────────────────▶ │  OPEN    │
   │ (normal)│                             │ (failing)│
   └─────────┘                             └────┬─────┘
        ▲                                       │
        │         success                       │ open_interval elapsed
        │                                       ▼
        │                                ┌───────────┐
        └─────────────────────────────── │ HALF-OPEN │
                                         │ (testing) │
                                         └───────────┘
```

**In-Memory Implementation**:

```
InMemoryCircuitBreaker {
    policy:               CircuitBreakerPolicy
    consecutive_failures: AtomicU32
    open_until_epoch_ms:  AtomicU64
}

allow_request():
    return now() >= open_until_epoch_ms

on_success():
    consecutive_failures = 0
    open_until_epoch_ms = 0

on_failure():
    failures = ++consecutive_failures
    if failures >= policy.failure_threshold:
        open_until_epoch_ms = now() + policy.open_interval
        consecutive_failures = 0  // reset for next cycle
```

### 5.4 Dead Letter Policy

```
DeadLetterPolicy {
    topic:             String   // "agent.dlq"
    max_payload_bytes: usize    // e.g., 1MB
    include_error_stack: bool   // include full stack traces
}
```

---

## 6. Scheduler Layer

### 6.1 Session Task

```
interface SessionTask {
    session_key() -> String
    task_id() -> String
}
```

### 6.2 Queue Overflow Policy

```
enum QueueOverflowPolicy {
    Collect   // Accept and queue
    FollowUp  // Convert to follow-up task
    Drop      // Reject immediately
}
```

### 6.3 Schedule Decision

```
enum TaskScheduleDecision {
    ExecuteNow
    Enqueued { queue_depth: u32 }
    Rejected { reason: String }
}
```

### 6.4 Session Scheduler Interface

```
interface SessionScheduler<T: SessionTask> {
    schedule(task: T, overflow: QueueOverflowPolicy) -> TaskScheduleDecision
    complete(session_key: String, task_id: String) -> void
    queue_depth(session_key: String) -> u32
    max_queue_depth() -> u32
    session_lock_ttl() -> Duration
}
```

**Key invariant**: Messages with the same `session_key` are processed **one at a time**. This prevents race conditions in multi-turn conversations.

---

## 7. Agent Execution Kernel

This is the **inner loop** — the core LLM ↔ Tool iteration engine.

### 7.1 Execution Input

```
AgentExecutionInput {
    user_content:        String               // Current user message
    user_media:          [LlmMedia]           // Images/media from user
    conversation_history: [ConversationMessage] // Prior turns
    session_key:         String               // Session identifier
    execution_context:   AgentExecutionContext  // Provider, model, prompts, metadata
}

AgentExecutionContext {
    system_prompt:      String?               // System instruction
    tool_choice:        JSON?                 // "auto" | "required" | "none" | {name: "..."}
    provider_id:        String?               // Target provider
    resolved_model:     String?               // Target model
    parent_session_key: String?               // Parent session (for sub-agents)
    message_id:         String?               // Current message ID
    current_attachments: [JSON]               // File attachment contexts
    tool_metadata:      Map<String, JSON>     // Metadata passed to tool executor
}
```

### 7.2 Execution Output

```
AgentExecutionOutput {
    content:          String                          // Final text response
    reasoning:        String?                         // Model reasoning (if available)
    disposition:      AgentExecutionDisposition       // How the turn ended
    tool_signals:     [ToolInvocationSignal]           // Signals from tools
    request_usages:   [AgentRequestUsage]              // Token usage per request
    request_audits:   [AgentRequestAudit]              // Full audit trail per request
    tool_audits:      [AgentToolAudit]                 // Tool execution audit trail
}

enum AgentExecutionDisposition {
    FinalMessage       // Normal completion with text response
    ApprovalRequired   // Tool requested user approval — turn paused
    Stopped            // Tool sent stop signal — turn halted
}
```

### 7.3 Execution Limits

```
AgentExecutionLimits {
    max_tool_iterations: u32   // Max LLM→Tool round-trips (0 = unlimited)
    max_tool_calls:      u32   // Max total tool invocations (0 = unlimited)
    token_budget:        u64   // Max total tokens across all requests (0 = unlimited)
}
```

### 7.4 The Inner Loop Algorithm

This is the **most critical algorithm** in the system. Pseudocode:

```
function run_agent_execution(provider, tools, input, limits, stream?) -> Output | Error:

    // ── Phase 1: Build Initial Message List ──
    tool_defs = tools.definitions()
    llm_messages = []

    if input.execution_context.system_prompt:
        llm_messages.append(SystemMessage(system_prompt))

    for msg in input.conversation_history:
        if msg.role in ["system", "user", "assistant", "tool"]:
            llm_messages.append(msg)

    if input.user_content is not empty OR input.user_media is not empty:
        llm_messages.append(UserMessage(input.user_content, input.user_media))

    // ── Phase 2: Iteration Loop ──
    tool_calls_used = 0
    tokens_used = 0
    iteration = 0

    loop:
        if limits.max_tool_iterations > 0 AND iteration >= limits.max_tool_iterations:
            break   // → ToolLoopExhausted

        iteration += 1

        // ── Final iteration warning ──
        if iteration == limits.max_tool_iterations AND limits.max_tool_iterations >= 3:
            // Inject a system message warning the model this is its last chance
            request_messages = llm_messages + [SystemMessage(
                "You are about to reach the maximum tool call limit. "
                "Please respond directly with a summary. Do NOT call any more tools."
            )]
        else:
            request_messages = llm_messages

        // ── Call LLM ──
        llm_response = provider.chat_stream(
            messages = request_messages,
            tools = tool_defs,
            model = input.execution_context.resolved_model,
            options = ChatOptions(temperature=0.2, tool_choice=...),
            stream = stream_forwarder
        )

        // ── Track Usage ──
        if llm_response.usage:
            tokens_used += llm_response.usage.total_tokens
            if limits.token_budget > 0 AND tokens_used > limits.token_budget:
                return Error::BudgetExceeded(tokens_used, token_budget)

        // ── Check Terminal Condition: No Tool Calls ──
        if llm_response.tool_calls is empty:
            return Output(
                content = llm_response.content,
                reasoning = llm_response.reasoning,
                disposition = FinalMessage,
                ...
            )

        // ── Process Tool Calls ──
        // Append assistant message with tool_calls to conversation
        llm_messages.append(AssistantMessage(
            content = llm_response.content,
            tool_calls = llm_response.tool_calls
        ))

        for call in llm_response.tool_calls:
            tool_calls_used += 1
            if limits.max_tool_calls > 0 AND tool_calls_used > limits.max_tool_calls:
                return Error::ToolLoopExhausted

            // Execute tool
            result = tools.execute(call.name, call.arguments, session_key, metadata)

            // Append tool result to conversation
            llm_messages.append(ToolMessage(
                content = result.to_tool_message_content(call.name),
                tool_call_id = call.id
            ))

            // ── Signal Short-Circuit Check ──
            if result.signals contains "approval_required":
                return Output(
                    content = assistant_content (if non-empty) else result.content,
                    disposition = ApprovalRequired,
                    ...
                )

            if result.signals contains "stop":
                return Output(
                    content = determine_stop_content(signals),
                    disposition = Stopped,
                    ...
                )

    // Loop exhausted without final response
    return Error::ToolLoopExhausted
```

### 7.5 Tool Result Envelope

Tool results are wrapped in a structured JSON envelope before being sent back to the model:

```json
{
    "ok": true/false,
    "tool": "tool_name",
    "content": "result text for model",
    "error": {                          // only if ok=false
        "code": "error_code",
        "details": {},
        "retryable": true/false
    },
    "signals": [...]                    // only if non-empty
}
```

### 7.6 Signal Short-Circuit Semantics

Two signals can **immediately terminate** the inner loop:

| Signal | Disposition | Behavior |
|--------|------------|----------|
| `approval_required` | `ApprovalRequired` | Tool needs user approval; turn pauses. If the assistant already produced text content, that content is preserved as the response. |
| `stop` | `Stopped` | Tool requests turn termination. Response is either `"Current turn stopped."` or empty (for question cards). |

---

## 8. Agent Loop Orchestrator

This is the **outer runtime** that wraps the inner kernel with reliability, observability, and transport integration.

### 8.1 Agent Run State Machine

```
enum AgentRunState {
    Received          // Message consumed from transport
    Validating        // Input validation in progress
    PreparingContext   // Building execution input
    Executing         // Inner kernel running
    Publishing        // Publishing outbound
    Completed         // Success
    Degraded          // Recoverable failure
    Failed            // Terminal failure
}
```

#### Transition Table

```
State × Event → New State

(Received, StartValidation)      → Validating
(Validating, ValidationPassed)   → PreparingContext
(Validating, ValidationFailed)   → Failed
(PreparingContext, ContextPrepared) → Executing
(Executing, ExecutionStarted)    → Executing (no-op)
(Executing, ExecutionFinished)   → Publishing
(Publishing, Published)          → Completed
(ANY, RecoverableError)          → Degraded
(ANY, FatalError)                → Failed
(otherwise)                      → No change
```

### 8.2 AgentLoop Configuration

```
AgentLoop {
    limits:           RunLimits
    scheduling:       SessionSchedulingPolicy
    provider_runtime: ProviderRuntimeSnapshot   // Multi-provider registry
    tools:            ToolRegistry
    system_prompt:    String?
    telemetry:        AgentTelemetry?
}

RunLimits {
    max_tool_iterations: u32
    max_tool_calls:      u32
    token_budget:        u64
    agent_timeout:       Duration    // Total turn timeout
    tool_timeout:        Duration    // Per-tool timeout
}

SessionSchedulingPolicy {
    strategy:         QueueStrategy  // Collect | FollowUp | Drop
    max_queue_depth:  u32
    lock_ttl:         Duration
}

ProviderRuntimeSnapshot {
    default_provider:        LlmProvider
    provider_registry:       Map<String, LlmProvider>
    default_provider_id:     String
    default_model:           String
    provider_default_models: Map<String, String>
}
```

### 8.3 Simple Execution: `run_once`

```
function run_once(inbound_transport, outbound_transport, subscription, idempotency):

    // 1. Consume inbound message
    inbound = inbound_transport.consume(subscription)

    // 2. Dedup check
    dedupe_key = idempotency_key(message_id, session_key, "agent_run")
    if idempotency.seen(dedupe_key):
        inbound_transport.ack(inbound.ack_handle)
        return ProcessOutcome(DuplicateMessage)

    // 3. Process message (calls inner kernel)
    outcome = process_message(inbound.payload)

    // 4. Publish outbound
    if outcome.final_response:
        outbound_transport.publish("agent.outbound", outcome.final_response)

    // 5. Mark idempotency
    idempotency.mark_seen(dedupe_key, agent_timeout + lock_ttl)

    // 6. ACK inbound
    inbound_transport.ack(inbound.ack_handle)

    return outcome
```

### 8.4 Reliable Execution: `run_once_reliable`

This is the **production-grade** execution path with full reliability stack:

```
function run_once_reliable(
    inbound_transport, outbound_transport, deadletter_transport,
    subscription, idempotency, retry_policy, deadletter_policy, circuit_breaker
):

    // 1. Consume + dedup (same as run_once)
    inbound = inbound_transport.consume(subscription)
    dedupe_key = idempotency_key(message_id, session_key, "agent_run")
    if idempotency.seen(dedupe_key):
        inbound_transport.ack(inbound.ack_handle)
        return ProcessOutcome(DuplicateMessage)

    attempt = max(inbound.header.attempt, 1)

    // 2. Retry loop
    loop:
        // 2a. Circuit breaker gate
        if NOT circuit_breaker.allow_request():
            decision = retry_policy.classify("provider_unavailable", attempt)
            result = handle_retry_decision(decision, attempt, ...)
            if result is terminal:
                return result
            attempt += 1
            continue

        // 2b. Process message
        outcome = process_message(inbound.payload)

        // 2c. Success path
        if outcome.error_code is None AND outcome.final_response exists:
            try:
                outbound_transport.publish("agent.outbound", outcome.final_response)
                circuit_breaker.on_success()
                idempotency.mark_seen(dedupe_key, agent_timeout + lock_ttl)
                inbound_transport.ack(inbound.ack_handle)
                return outcome
            catch TransportError:
                circuit_breaker.on_failure()
                decision = retry_policy.classify("transport_unavailable", attempt)
                result = handle_retry_decision(decision, attempt, ...)
                if result is terminal:
                    return result
                attempt += 1
                continue

        // 2d. Failure path
        error_kind = classify_error_kind(outcome.error_code)
        if error_code in [ProviderUnavailable, ToolTimeout]:
            circuit_breaker.on_failure()

        decision = retry_policy.classify(error_kind, attempt)
        result = handle_retry_decision(decision, attempt, ...)
        if result is terminal:
            return result
        attempt += 1
```

### 8.5 Retry Decision Handler

```
function handle_retry_decision(decision, attempt, inbound, transports, deadletter_policy):

    switch decision:
        case RetryNow:
            telemetry.incr_counter("agent_retry_total", ...)
            return None  // continue loop

        case RetryAfter(delay):
            telemetry.incr_counter("agent_retry_total", ...)
            sleep(delay)
            return None  // continue loop

        case Abort:
            inbound_transport.ack(ack_handle)
            return ProcessOutcome(RetryExhausted)

        case SendToDeadLetter:
            deadletter = DeadLetterMessage(
                original_message_id = inbound.header.message_id,
                session_key = inbound.header.session_key,
                final_error = "SentToDeadLetter",
                attempts = attempt,
                reason = "exhausted retries",
                original_payload = inbound.payload
            )
            deadletter_transport.publish("agent.dlq", deadletter)
            inbound_transport.ack(ack_handle)
            return ProcessOutcome(SentToDeadLetter)
```

### 8.6 Error Classification for Retry

```
function classify_error_kind(error_code) -> String:
    switch error_code:
        case ValidationFailed | InvalidSchema:  return "validation"
        case DuplicateMessage:                   return "duplicate"
        case ProviderUnavailable:                return "provider_unavailable"
        case ToolTimeout:                        return "tool_timeout"
        case TransportUnavailable:               return "transport_unavailable"
        default:                                 return "unknown"
```

### 8.7 LLM Error → Protocol Error Mapping

```
function map_llm_error_to_code(err: LlmError) -> ErrorCode:
    switch err:
        case ProviderUnavailable | RequestFailed | StreamFailed:
            return ProviderUnavailable
        case InvalidResponse:
            return ProviderResponseInvalid
```

### 8.8 Provider Resolution

The orchestrator supports **multi-provider routing**:

```
function resolve_provider(metadata, provider_runtime):
    // 1. Check for explicit provider override in metadata
    requested = metadata["agent.provider_id"]
    if requested exists in provider_runtime.registry:
        return (requested, registry[requested])

    // 2. Fallback to default
    if requested was specified but not found:
        log.warn("provider override not found, falling back to default")

    return (default_provider_id, default_provider)
```

### 8.9 Process Outcome

```
ProcessOutcome {
    final_response:    Envelope<OutboundMessage>?  // The response (if any)
    error_code:        ErrorCode?                  // Error classification
    final_state:       AgentRunState               // Terminal state
    llm_audits:        [LlmAuditPayload]           // LLM request audit trail
    tool_audits:       [AgentToolAudit]            // Tool execution audit trail
    audit_message_id:  UUID?                       // Original message ID
    audit_session_key: String?                     // Session key
    audit_chat_id:     String?                     // Chat ID
}
```

---

## 9. LLM Provider Abstraction

### 9.1 LlmMessage

```
LlmMessage {
    role:         String          // "system" | "user" | "assistant" | "tool"
    content:      String          // Text content
    media:        [LlmMedia]      // Image URLs (user messages only)
    tool_calls:   [ToolCall]?     // Assistant-initiated tool calls
    tool_call_id: String?         // Tool response correlation ID
}

LlmMedia {
    mime_type: String?    // e.g., "image/png"
    url:       String     // https:// URL or data: URI
}

ToolCall {
    id:        String?    // Provider-assigned call ID
    name:      String     // Tool name
    arguments: JSON       // Parsed arguments
}
```

### 9.2 ChatOptions

```
ChatOptions {
    temperature:          f32      // Sampling temperature (default: 0.2)
    max_tokens:           u32?     // Max generated tokens
    max_output_tokens:    u32?     // Responses API output limit
    previous_response_id: String?  // Responses API continuation
    instructions:         String?  // Responses API instructions
    tool_choice:          JSON?    // Tool selection strategy
    parallel_tool_calls:  bool?    // Allow parallel tool execution
    // ... additional provider-specific options
}
```

### 9.3 LlmResponse

```
LlmResponse {
    content:       String           // Text response
    reasoning:     String?          // Model reasoning chain (optional)
    tool_calls:    [ToolCall]       // Requested tool invocations
    usage:         LlmUsage?        // Token usage
    usage_source:  LlmUsageSource?  // How usage was determined
    audit:         LlmAuditPayload? // Request/response audit
}

LlmUsage {
    input_tokens:          u64
    output_tokens:         u64
    total_tokens:          u64
    cached_input_tokens:   u64?     // Prompt cache hits
    reasoning_tokens:      u64?     // Reasoning/thinking tokens
    provider_request_id:   String?  // Provider-side request ID
    provider_response_id:  String?  // Provider-side response ID
}

enum LlmUsageSource {
    ProviderReported   // Usage from provider response
    EstimatedLocal     // Estimated via local tokenizer
}
```

### 9.4 LlmAuditPayload

```
LlmAuditPayload {
    provider:              String
    model:                 String
    wire_api:              String          // "chat_completions" | "responses"
    status:                LlmAuditStatus  // Success | Failed
    error_code:            String?
    error_message:         String?
    provider_request_id:   String?
    provider_response_id:  String?
    request_body:          JSON            // Full request sent to provider
    response_body:         JSON?           // Full response received
    requested_at_ms:       i64            // Unix epoch milliseconds
    responded_at_ms:       i64?           // Unix epoch milliseconds
}
```

### 9.5 LlmProvider Interface

```
interface LlmProvider {
    name() -> String
    default_model() -> String
    wire_api() -> String?           // Optional, for audit/telemetry
    tokenizer_path() -> String?     // Optional local tokenizer

    chat(
        messages: [LlmMessage],
        tools:    [ToolDefinition],
        model:    String?,
        options:  ChatOptions
    ) -> Result<LlmResponse, LlmError>

    chat_stream(
        messages: [LlmMessage],
        tools:    [ToolDefinition],
        model:    String?,
        options:  ChatOptions,
        stream:   Sender<LlmStreamEvent>?
    ) -> Result<LlmResponse, LlmError>
        // default: delegates to chat(), then emits content as stream events
}
```

### 9.6 Streaming Events

```
enum LlmStreamEvent {
    ContentDelta(String)     // Incremental text content
    ReasoningDelta(String)   // Incremental reasoning content
}
```

### 9.7 LlmError

```
enum LlmError {
    ProviderUnavailable { message: String, audit: LlmAuditPayload? }
    InvalidResponse     { message: String, audit: LlmAuditPayload? }
    RequestFailed       { message: String, audit: LlmAuditPayload? }
    StreamFailed        { message: String, audit: LlmAuditPayload? }
}
```

Each variant optionally carries an `LlmAuditPayload` for full request/response capture even on failure.

### 9.8 Tool Definition (for LLM)

```
ToolDefinition {
    name:        String
    description: String
    parameters:  JSON    // JSON Schema for tool arguments
}
```

---

## 10. Tool System Abstraction

### 10.1 Tool Interface

```
interface Tool {
    name() -> String
    description() -> String
    parameters() -> JSON          // JSON Schema
    category() -> ToolCategory

    execute(args: JSON, ctx: ToolContext) -> Result<ToolOutput, ToolError>
}
```

### 10.2 Tool Category

```
enum ToolCategory {
    FilesystemRead       // Read-only file operations
    FilesystemWrite      // Write/modify file operations
    NetworkRead          // Read-only network operations
    NetworkWrite         // Network operations that modify external state
    Shell                // Command execution
    Hardware             // Hardware/peripheral operations
    Memory               // Memory read/write
    Knowledge            // Knowledge base retrieval
    Messaging            // Message sending
    Destructive          // High-risk/destructive operations
}
```

### 10.3 Tool Context

```
ToolContext {
    session_key: String                // Current session
    metadata:    Map<String, JSON>     // Execution metadata (provider, model, etc.)
}
```

### 10.4 Tool Output

```
ToolOutput {
    content_for_model: String           // What the LLM sees
    content_for_user:  String?          // What the user sees (optional)
    media:             [LlmMedia]       // Media returned to model
    signals:           [ToolSignal]     // Signals to runtime
}
```

### 10.5 Tool Signal

Signals are **out-of-band messages** from tools to the runtime/channel layer:

```
ToolSignal {
    kind:    String    // Signal type identifier
    payload: JSON      // Signal-specific data
}
```

**Built-in signal kinds**:

| Kind | Purpose | Payload |
|------|---------|---------|
| `approval_required` | Tool needs user approval before execution | `{approval_id, tool_name, session_key, risk_level?, command_preview?}` |
| `stop` | Tool requests immediate turn termination | `{reason?, source?}` |
| `im_card` | Interactive card for UI rendering | `{kind, title, body, actions, metadata}` |
| `channel_attachment` | File/media to send to chat channel | `{kind, archive_id?, path?, filename?, caption?}` |

### 10.6 Tool Error

```
enum ToolError {
    InvalidArgs(String)
    ExecutionFailed(String)
    StructuredExecutionFailed {
        message:   String
        code:      String        // Machine-readable error code
        details:   JSON?         // Additional error context
        retryable: bool          // Can the operation be retried?
        signals:   [ToolSignal]  // Signals even on error (e.g., approval_required)
    }
}
```

**Critical**: Tool errors can carry **signals**. The `approval_required` pattern works by returning an error with an `approval_required` signal — this is how the inner loop detects the need for short-circuit.

### 10.7 Tool Registry

```
ToolRegistry {
    register(tool: Tool) -> void
    register_shared(tool: SharedPtr<Tool>) -> void
    get(name: String) -> SharedPtr<Tool>?
    list() -> [String]
    unregister(name: String) -> bool
    unregister_many(names: [String]) -> u32
}
```

Thread-safe via internal read-write lock.

### 10.8 ToolExecutor Interface (for Agent Kernel)

The kernel interacts with tools through the `ToolExecutor` bridge:

```
interface ToolExecutor {
    definitions() -> [ToolDefinition]

    execute(
        name:      String,
        arguments: JSON,
        session_key: String,
        metadata:  Map<String, JSON>
    ) -> ToolInvocationResult
}

ToolInvocationResult {
    ok:                bool
    content_for_model: String
    error_code:        String?
    error_details:     JSON?
    retryable:         bool?
    signals:           [ToolInvocationSignal]
    media:             [LlmMedia]
}
```

---

## 11. Context Compression

For long conversations that exceed context windows, the system supports **conversation summarization**:

### 11.1 Conversation Summary

```
ConversationSummary {
    goal:      String      // Current stage's core goal
    progress:  [String]    // Completed items in chronological order
    pending:   [String]    // Unfinished tasks
    decisions: [String]    // Confirmed technical/product decisions
    notes:     [String]    // Key context (constraints, risks, conventions)
}
```

### 11.2 Compression Flow

```
Old Summary + New Messages → LLM Compression Prompt → New Summary
```

The compression prompt instructs the model to:
1. Preserve user goals
2. Preserve completed steps
3. Preserve pending tasks
4. Preserve key decisions
5. Remove chitchat and repetitive content

---

## 12. Observability & Telemetry

### 12.1 AgentTelemetry Interface

```
interface AgentTelemetry {
    // Counters
    incr_counter(name: String, labels: [(String, String)], value: u64)

    // Histograms
    observe_histogram(name: String, labels: [(String, String)], duration: Duration)

    // Structured audit events
    emit_audit_event(event_name: String, trace_id: UUID, payload: JSON)

    // Component health
    set_health(component: String, status: HealthStatus)

    // Domain-specific records
    record_tool_outcome(session_key, tool_name, status, error_code?, duration)
    record_model_request(ModelRequestRecord)
    record_model_tool_outcome(ModelToolOutcomeRecord)
    record_turn_outcome(TurnOutcomeRecord)
}
```

### 12.2 Telemetry Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `agent_inbound_consumed_total` | Counter | session_key, provider | Inbound messages processed |
| `agent_outbound_published_total` | Counter | session_key, provider | Successful outbound publishes |
| `agent_tool_success_total` | Counter | session_key, tool_name | Tool execution successes |
| `agent_tool_failure_total` | Counter | session_key, tool_name, error_code | Tool execution failures |
| `agent_retry_total` | Counter | session_key, error_code | Retry attempts |
| `agent_deadletter_total` | Counter | session_key | Messages sent to DLQ |
| `agent_run_duration_ms` | Histogram | session_key, stage | Execution duration |

### 12.3 Audit Events

| Event | Trigger |
|-------|---------|
| `inbound_received` | Message consumed from transport |
| `validation_failed` | Input validation failure |
| `tool_called` | Tool execution started |
| `tool_failed` | Tool execution failed |
| `final_response_published` | Response successfully published |
| `message_sent_dlq` | Message moved to dead letter |

### 12.4 Health Status

```
enum HealthStatus {
    Healthy
    Degraded
    Unhealthy
}
```

---

## 13. State Machine — Formal Definition

### 13.1 Agent Run State

```
States: {Received, Validating, PreparingContext, Executing, Publishing, Completed, Degraded, Failed}

Initial State: Received

Terminal States: {Completed, Degraded, Failed}

Transitions:
    Received × StartValidation     → Validating
    Validating × ValidationPassed  → PreparingContext
    Validating × ValidationFailed  → Failed
    PreparingContext × ContextPrepared → Executing
    Executing × ExecutionStarted   → Executing
    Executing × ExecutionFinished  → Publishing
    Publishing × Published         → Completed
    ANY × RecoverableError         → Degraded
    ANY × FatalError               → Failed
    _ × _                          → (no change)
```

### 13.2 Circuit Breaker State

```
States: {Closed, Open, HalfOpen}

Initial State: Closed

Transitions:
    Closed × failures >= threshold → Open
    Open × interval_elapsed        → HalfOpen
    HalfOpen × success             → Closed
    HalfOpen × failure             → Open
```

### 13.3 Inner Execution Loop State

```
States: {BuildingContext, CallingModel, ProcessingToolCalls, Completed, Exhausted, BudgetExceeded}

Transitions:
    BuildingContext × context_ready         → CallingModel
    CallingModel × response_no_tool_calls   → Completed
    CallingModel × response_with_tool_calls → ProcessingToolCalls
    ProcessingToolCalls × all_done          → CallingModel  (next iteration)
    ProcessingToolCalls × approval_signal   → Completed (short-circuit)
    ProcessingToolCalls × stop_signal       → Completed (short-circuit)
    ProcessingToolCalls × call_limit_hit    → Exhausted
    CallingModel × iteration_limit_hit      → Exhausted
    CallingModel × token_budget_exceeded    → BudgetExceeded
```

---

## 14. Data Flow Diagrams

### 14.1 Happy Path (Single Turn, No Tools)

```
User → [Channel] → InboundMessage
    → Envelope<InboundMessage>
    → Transport.consume()
    → Idempotency.seen()? → No
    → AgentLoop.process_message_inner()
        → State: Received → Validating → PreparingContext → Executing
        → Build messages [system?, history..., user]
        → LLM.chat(messages, tools=[])
        → LLM response: {content: "Hello!", tool_calls: []}
        → No tool calls → disposition = FinalMessage
        → State: Executing → Publishing → Completed
    → Transport.publish("agent.outbound", Envelope<OutboundMessage>)
    → Idempotency.mark_seen(key, ttl)
    → Transport.ack(handle)
→ [Channel] → User sees "Hello!"
```

### 14.2 Tool Iteration Path

```
User: "What's the weather in Tokyo?"
    → AgentLoop.process_message_inner()
        → Iteration 1:
            → LLM.chat(messages, tools=[weather_tool])
            → Response: tool_calls=[{name: "weather", args: {city: "Tokyo"}}]
            → ToolExecutor.execute("weather", {city: "Tokyo"})
            → Result: "23°C, sunny"
            → Append assistant(tool_calls) + tool(result) to messages

        → Iteration 2:
            → LLM.chat(messages, tools=[weather_tool])
            → Response: {content: "It's 23°C and sunny in Tokyo!", tool_calls: []}
            → No tool calls → disposition = FinalMessage
    → Publish response
```

### 14.3 Approval Short-Circuit Path

```
User: "Delete all logs"
    → Iteration 1:
        → LLM.chat → tool_calls=[{name: "shell", args: {cmd: "rm -rf /var/log/*"}}]
        → ShellTool.execute → ToolError {
            code: "approval_required",
            signals: [{kind: "approval_required", payload: {...}}]
          }
        → Signal detected! → Short-circuit
        → disposition = ApprovalRequired
    → Publish response with approval card
    → User sees: "⚠️ Approve: rm -rf /var/log/* ? [Approve] [Reject]"
```

### 14.4 Retry + DLQ Path

```
InboundMessage → consume
    → Attempt 1: process_message → ProviderUnavailable
        → circuit_breaker.on_failure()
        → classify("provider_unavailable", 1) → RetryAfter(200ms)
        → sleep(200ms)

    → Attempt 2: process_message → ProviderUnavailable
        → circuit_breaker.on_failure()
        → classify("provider_unavailable", 2) → RetryAfter(400ms)
        → sleep(400ms)

    → Attempt 3: process_message → ProviderUnavailable
        → circuit_breaker.on_failure()
        → classify("provider_unavailable", 3) → RetryAfter(800ms)
        → sleep(800ms)

    ... (assuming max_attempts = 5)

    → Attempt 5: still failing
        → classify("provider_unavailable", 5) → SendToDeadLetter
        → Publish to "agent.dlq"
        → ACK inbound
        → Return ProcessOutcome(SentToDeadLetter)
```

---

## 15. Configuration Schema

### 15.1 Application Config (TOML)

```toml
# ~/.klaw/config.toml

model_provider = "openai"          # Default provider ID

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"      # or "responses"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"         # Environment variable name
# api_key = "sk-..."               # Or direct key (not recommended)
stream = true
# tokenizer_path = "/path/to/tokenizer.json"
# proxy = "http://proxy:8080"

[model_providers.anthropic]
base_url = "https://api.anthropic.com"
wire_api = "chat_completions"
default_model = "claude-sonnet-4-20250514"
env_key = "ANTHROPIC_API_KEY"
```

### 15.2 Runtime Limits (defaults)

```
max_tool_iterations = 20
max_tool_calls      = 100
token_budget        = 0          # 0 = unlimited
agent_timeout       = 300s
tool_timeout        = 60s
```

### 15.3 Scheduling (defaults)

```
strategy          = "Collect"
max_queue_depth   = 10
lock_ttl          = 60s
```

### 15.4 Retry (defaults)

```
max_attempts = 5
base_delay   = "200ms"
max_delay    = "30s"
jitter_ratio = 0.2
```

### 15.5 Circuit Breaker (defaults)

```
failure_threshold      = 5
open_interval          = "60s"
half_open_max_requests = 3
```

---

## 16. Error Handling Matrix

### 16.1 Inner Loop Errors (Agent Kernel)

| Error | Cause | Handling |
|-------|-------|----------|
| `Provider(LlmError)` | LLM API failure | Propagated to outer loop |
| `ToolLoopExhausted` | `max_tool_iterations` or `max_tool_calls` reached | Outer loop returns user-friendly message |
| `BudgetExceeded` | `token_budget` exceeded | Outer loop returns error |

### 16.2 Outer Loop Errors (AgentLoop)

| Error Code | Retryable | Circuit Breaker | DLQ |
|-----------|-----------|----------------|-----|
| `InvalidSchema` | ❌ Abort | No | No |
| `ValidationFailed` | ❌ Abort | No | No |
| `DuplicateMessage` | ❌ Skip | No | No |
| `SessionBusy` | ⏳ RetryAfter | No | Yes (if exhausted) |
| `AgentTimeout` | ⏳ RetryAfter | No | Yes (if exhausted) |
| `ToolTimeout` | ⏳ RetryAfter | ✅ on_failure | Yes (if exhausted) |
| `ProviderUnavailable` | ⏳ RetryAfter | ✅ on_failure | Yes (if exhausted) |
| `ProviderResponseInvalid` | ❌ Abort | No | No |
| `TransportUnavailable` | ⏳ RetryAfter | ✅ on_failure | Yes (if exhausted) |
| `RetryExhausted` | Terminal | — | No (already handled) |
| `BudgetExceeded` | ❌ Abort | No | No |
| `SentToDeadLetter` | Terminal | — | Yes |

### 16.3 Tool Error → Result Mapping

```
Tool.execute() returns:
    Ok(ToolOutput) → ToolInvocationResult { ok: true, content, signals, media }
    Err(ToolError) → ToolInvocationResult { ok: false, content: error_msg, error_code, signals }

Note: Even errors produce a ToolInvocationResult (not a panic).
The error message becomes the tool message content sent to the LLM,
allowing the model to self-correct or explain the failure.
```

---

## 17. Implementation Checklist

### Phase 1: Protocol Foundation

- [ ] Define `Envelope<T>` with header, metadata, payload
- [ ] Define `EnvelopeHeader` with all fields
- [ ] Define `SchemaVersion` with evolution rules
- [ ] Define `MessageTopic` enum (Inbound, Outbound, Events, DeadLetter)
- [ ] Define `ErrorCode` enum with all variants
- [ ] Define `InboundMessage`, `OutboundMessage`, `DeadLetterMessage`
- [ ] Implement idempotency key format: `{message_id}:{session_key}:{stage}`

### Phase 2: Transport Layer

- [ ] Define `DeliveryMode` enum
- [ ] Define `Subscription` structure
- [ ] Define `TransportMessage<T>` and `TransportAckHandle`
- [ ] Implement `MessageTransport<T>` interface
- [ ] Implement in-memory transport for testing
- [ ] Define `TransportError` variants

### Phase 3: Reliability Layer

- [ ] Define `RetryDecision` enum
- [ ] Implement `RetryPolicy` interface
- [ ] Implement `ExponentialBackoffRetryPolicy`
- [ ] Implement `IdempotencyStore` interface
- [ ] Implement in-memory idempotency store
- [ ] Implement `CircuitBreaker` interface
- [ ] Implement in-memory circuit breaker
- [ ] Define `DeadLetterPolicy`

### Phase 4: LLM Provider

- [ ] Define `LlmMessage`, `ToolCall`, `LlmMedia`
- [ ] Define `ChatOptions`
- [ ] Define `LlmResponse`, `LlmUsage`, `LlmAuditPayload`
- [ ] Define `LlmError` with all variants
- [ ] Define `LlmStreamEvent` enum
- [ ] Implement `LlmProvider` interface
- [ ] Implement at least one provider (OpenAI-compatible)
- [ ] Define `ToolDefinition` for LLM consumption

### Phase 5: Tool System

- [ ] Define `Tool` interface with name, description, parameters, category
- [ ] Define `ToolContext`
- [ ] Define `ToolOutput` with content_for_model, signals, media
- [ ] Define `ToolSignal` with built-in signal kinds
- [ ] Define `ToolError` with structured variant
- [ ] Implement `ToolRegistry` (thread-safe)
- [ ] Implement `ToolExecutor` bridge interface
- [ ] Implement at least one tool (e.g., echo, shell, web_search)

### Phase 6: Agent Execution Kernel (Inner Loop)

- [ ] Define `AgentExecutionInput`, `AgentExecutionOutput`
- [ ] Define `AgentExecutionLimits`
- [ ] Define `AgentExecutionDisposition` (FinalMessage, ApprovalRequired, Stopped)
- [ ] Implement message list construction (system + history + user)
- [ ] Implement the iteration loop:
  - [ ] LLM call with tool definitions
  - [ ] Token budget tracking
  - [ ] Tool call execution and result formatting
  - [ ] Iteration count tracking
  - [ ] Total tool call count tracking
  - [ ] Final iteration warning system message
  - [ ] Approval signal short-circuit
  - [ ] Stop signal short-circuit
- [ ] Implement streaming support via event channel
- [ ] Implement conversation history filtering (role whitelist)

### Phase 7: Agent Loop Orchestrator (Outer Runtime)

- [ ] Define `AgentRunState` enum with all states
- [ ] Implement `transition()` state machine
- [ ] Implement `process_message_inner()`:
  - [ ] Provider resolution from metadata
  - [ ] System prompt resolution
  - [ ] Conversation history extraction
  - [ ] Media/attachment processing
  - [ ] User content augmentation
  - [ ] Execution context assembly
  - [ ] Inner kernel invocation
  - [ ] Outcome mapping (success, provider error, loop exhausted, budget exceeded)
  - [ ] Outbound metadata enrichment (disposition, usage, audit, signals)
- [ ] Implement `run_once()` (simple path)
- [ ] Implement `run_once_reliable()` (full reliability stack)
- [ ] Implement `handle_retry_decision()`

### Phase 8: Observability

- [ ] Define `AgentTelemetry` interface
- [ ] Implement counter metrics (inbound, outbound, tool, retry, deadletter)
- [ ] Implement histogram metrics (duration)
- [ ] Implement audit events
- [ ] Implement model request recording
- [ ] Implement turn outcome recording
- [ ] Implement health status tracking

### Phase 9: Context Compression (Optional)

- [ ] Define `ConversationSummary` structure
- [ ] Implement compression prompt builder
- [ ] Implement summary parser
- [ ] Implement merge/reset logic

### Phase 10: Integration & Testing

- [ ] End-to-end test: happy path (no tools)
- [ ] End-to-end test: tool iteration
- [ ] End-to-end test: approval short-circuit
- [ ] End-to-end test: stop signal
- [ ] End-to-end test: token budget enforcement
- [ ] End-to-end test: iteration limit exhaustion
- [ ] End-to-end test: retry + DLQ
- [ ] End-to-end test: circuit breaker
- [ ] End-to-end test: idempotency dedup
- [ ] End-to-end test: multi-provider routing
- [ ] End-to-end test: streaming

---

## Appendix A: Glossary

| Term | Definition |
|------|-----------|
| **Session Key** | Unique identifier for serializing message processing, typically `"{channel}:{chat_id}"` |
| **Envelope** | Generic message wrapper with header, metadata, and typed payload |
| **Turn** | One complete user→agent→user interaction cycle |
| **Iteration** | One LLM call within a turn (a turn may have multiple iterations if tools are called) |
| **Tool Call** | A single tool invocation requested by the LLM |
| **Signal** | An out-of-band message from a tool to the runtime (e.g., approval_required, stop) |
| **Short-Circuit** | Early termination of the inner loop due to a signal |
| **Disposition** | How a turn ended: FinalMessage, ApprovalRequired, or Stopped |
| **DLQ** | Dead Letter Queue — where permanently failed messages go |
| **Idempotency Key** | Deduplication key: `{message_id}:{session_key}:{stage}` |
| **Circuit Breaker** | Pattern that prevents cascading failures by stopping requests to failing services |
| **Wire API** | The underlying API protocol used by an LLM provider (e.g., chat_completions, responses) |

## Appendix B: Conversation Message Format (History)

```json
{
    "role": "user | assistant | system | tool",
    "content": "message text",
    "tool_calls": [
        {
            "id": "call_abc123",
            "name": "shell",
            "arguments": {"command": "ls -la"}
        }
    ],
    "tool_call_id": "call_abc123"
}
```

Only roles `system`, `user`, `assistant`, `tool` are forwarded to the LLM. Other roles in history are filtered out.

## Appendix C: Streaming Protocol

The streaming protocol uses an unbounded channel of `AgentExecutionStreamEvent`:

```
enum AgentExecutionStreamEvent {
    Snapshot {
        content:   String     // Accumulated content so far
        reasoning: String?    // Accumulated reasoning so far
    }
    Clear                    // Sent when switching from streaming to tool execution
}
```

**Flow**:
1. LLM streams content deltas → `Snapshot` events with growing content
2. If LLM returns tool calls → `Clear` event (reset UI)
3. Tool results are processed silently
4. Next iteration starts fresh streaming

## Appendix D: Metadata Key Registry

### Inbound Metadata Keys

| Key | Type | Source |
|-----|------|--------|
| `agent.system_prompt` | String | Channel/Config |
| `agent.tool_choice` | JSON | Channel |
| `agent.provider_id` | String | Channel/Config |
| `agent.model` | String | Channel/Config |
| `agent.conversation_history` | [ConversationMessage] | Session Store |
| `agent.resume_turn` | bool | Channel |
| `agent.parent_session_key` | String | Sub-agent |
| `agent.message_id` | String | Transport |
| `agent.current_attachments` | [JSON] | Channel |
| `trigger.kind` | String | Heartbeat/Channel |
| `heartbeat.*` | various | Heartbeat module |
| `channel.*` | various | Channel adapter |

### Outbound Metadata Keys

| Key | Type | Source |
|-----|------|--------|
| `agent.disposition` | String | Agent Kernel |
| `llm.usage.records` | [JSON] | Agent Kernel |
| `llm.audit.records` | [JSON] | Agent Kernel |
| `turn.stopped` | bool | Agent Kernel |
| `turn.stop_signal` | JSON | Agent Kernel |
| `approval.required` | bool | Agent Loop |
| `approval.id` | String | Agent Loop |
| `approval.signal` | JSON | Agent Loop |
| `im.card` | JSON | Agent Loop |
| `tool.signals` | [JSON] | Agent Kernel |
| `reasoning` | String | Agent Loop |
| `channel.attachments` | [JSON] | Agent Loop |
