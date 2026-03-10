# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test

```bash
# Build entire workspace
cargo build --workspace

# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p klaw-core

# Run a single test by name
cargo test -p klaw-core -- <test_name>

# Run the CLI
cargo run -p klaw-cli          # Starts interactive stdio mode
cargo run -p klaw-cli -- --help

# One-shot request
cargo run -p klaw-cli -- once --input "your prompt"
```

## Architecture

**Klaw** is a Rust-based AI agent framework with MQ-style message passing and reliability controls.

### Workspace Structure

| Crate | Purpose |
|-------|---------|
| `klaw-config` | TOML configuration loading (`~/.klaw/config.toml`) |
| `klaw-llm` | LLM provider abstraction (OpenAI-compatible, Anthropic) |
| `klaw-tool` | Tool trait definition and built-in tools (shell, fs, web, etc.) |
| `klaw-core` | Agent runtime: message protocol, scheduler, reliability controls |
| `klaw-cli` | CLI entrypoint with stdio/once commands |
| `klaw-mcp`, `klaw-skill`, `klaw-memory` | Extension points (MCP, skills, memory) |

### Message Flow

```
User Input → klaw-cli → InboundMessage (agent.inbound)
                    → AgentLoop.run_once_reliable()
                    → OutboundMessage (agent.outbound) → Response
                                               ↘ DeadLetterMessage (agent.dlq)
```

### Core Components

- **AgentLoop** (`klaw-core/src/agent/`): State machine driving session execution (`Received` → `Validating` → `Scheduling` → `CallingModel` → `ToolLoop` → `Finalizing` → `Publishing` → `Completed`)
- **SessionScheduler** (`klaw-core/src/scheduler.rs`): Serial execution per `session_key` with queue strategies (Collect/FollowUp/Drop)
- **Reliability** (`klaw-core/src/reliability.rs`): Retry policies (exponential backoff), idempotency stores, circuit breakers, dead-letter handling
- **Transport** (`klaw-core/src/transport.rs`): In-memory transport implementing pub/sub semantics

### Configuration

Config file at `~/.klaw/config.toml`:

```toml
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
```

### Key Design Patterns

- **Session-based concurrency**: Same `session_key` guarantees serial execution; concurrent sessions run independently
- **At-least-once delivery**: Achieved via idempotency keys (`{message_id}:{session_key}:{stage}`)
- **Graceful degradation**: Tool timeouts → retry → fallback to no-tool response → DLQ

### Documentation

Detailed architecture docs in `docs/agent-core/`:
- `message-protocol.md`: Envelope schema, topics, error codes
- `runtime-state-machine.md`: State transitions and scheduling policies
- `reliability-controls.md`: Retry, idempotency, circuit breaker, budget guards
