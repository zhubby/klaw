# Klaw

Klaw is a Rust-based AI Agent framework with an MQ-style message passing architecture and built-in reliability controls.

## Features

- **Session Isolation**: Serial execution guarantee based on `session_key`, ensuring in-order message processing within the same session
- **Reliability Controls**: Retry strategies (exponential backoff), idempotency stores, circuit breakers, and dead-letter queues
- **Extensible Tool System**: Tool trait abstraction with built-in tools including shell, web search, memory, and sub-agent
- **Skills Support**: Compatible with Anthropic/Vercel skills, with Git-based synchronization
- **Multi-Backend Storage**: Unified trait interface supporting Turso/libSQL and SQLx backends
- **Message Bus**: Decoupled communication between Channels and Agent via MessageBus, supporting multi-platform integration

## Architecture

```
User Input → InboundMessage (agent.inbound)
                    → AgentLoop.run_once_reliable()
                    → OutboundMessage (agent.outbound) → Response
                                               ↘ DeadLetterMessage (agent.dlq)
```

### Workspace Structure

| Crate | Purpose |
|-------|---------|
| `klaw-config` | TOML configuration loading (`~/.klaw/config.toml`) |
| `klaw-tool` | Tool trait definition and built-in tools (shell, fs, web, etc.) |
| `klaw-core` | Agent runtime: message protocol, scheduler, reliability controls |
| `klaw-cli` | CLI entrypoint (binary: `klaw`) |
| `klaw-storage` | Storage abstraction layer (session/cron persistence) |
| `klaw-gateway` | WebSocket gateway service |
| `klaw-skill` | Skills lifecycle management |
| `klaw-memory` | Long-term memory service (BM25 + Vector) |
| `klaw-mcp` | Model Context Protocol support |
| `klaw-cron` | Scheduled job management |
| `klaw-heartbeat` | Session heartbeat monitoring |
| `klaw-channel` | Multi-platform channel adapters |

## Installation & Build

```bash
# Build the entire workspace
cargo build --workspace

# Run all tests
cargo test --workspace
```

## Configuration

The configuration file is located at `~/.klaw/config.toml`. It will be auto-generated on first run.

### Basic Configuration

```toml
model_provider = "openai"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "chat_completions"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
```

### Enable Web Search

```toml
[tools.web_search]
enabled = true
provider = "tavily"

[tools.web_search.tavily]
env_key = "TAVILY_API_KEY"
search_depth = "basic"
include_answer = true
```

### Enable Memory

```toml
[tools.memory]
enabled = true
search_limit = 8
use_vector = true

[memory.embedding]
enabled = true
provider = "openai"
model = "text-embedding-3-small"
```

### Configure Skills

```toml
[skills]
sync_timeout = 60

[skills.anthropic]
address = "https://github.com/anthropics/skills"

[skills.vercel]
address = "https://github.com/vercel-labs/skills"
installed = ["brainstorming"]
```

### Shell Tool Configuration

```toml
[tools.shell]
approval_policy = "on_request"  # "never" or "on_request"
max_timeout_ms = 120000
max_output_bytes = 131072
```

## Usage

### stdio Mode (Local Interactive)

```bash
klaw stdio
```

- Type any text and press Enter to start a conversation
- Type `/exit` to exit

### One-Shot Request

```bash
klaw agent --input "your prompt"
```

### Gateway Mode (WebSocket)

```bash
klaw gateway
```

Connect to `ws://127.0.0.1:8080/ws/chat?session_key=your-room` after startup.

### Daemon Mode (User-Level Service)

```bash
# Install as a user-level daemon
klaw daemon install

# Check daemon status
klaw daemon status

# Stop the daemon
klaw daemon stop

# Uninstall the daemon
klaw daemon uninstall
```

- `install` registers `klaw gateway` as a user-level system service and starts it immediately
- macOS uses `launchd`, Linux uses `systemd --user`

## Session Management

```bash
# List all sessions
klaw session list

# Get session details
klaw session get --session-key "stdio:my-chat"
```

## Core Concepts

### Message Protocol

- **InboundMessage**: Normalized incoming messages from any channel (Telegram, Discord, Webhook, etc.)
- **OutboundMessage**: Normalized outgoing messages to channels
- **session_key**: `{channel}:{chat_id}` - ensures serial execution for the same session

### AgentLoop

The central orchestrator that:
1. Consumes inbound messages
2. Manages session context
3. Calls LLM with tool definitions
4. Executes tool loops
5. Publishes outbound messages

### Tool Loop

When the LLM requests tool calls:
1. Limit checks (max_tool_calls, token budget, loop guard)
2. Execute tools (parallel or serial based on category)
3. Write results back to session
4. Re-call LLM with updated context
5. Repeat until no more tool calls or limits reached

### Reliability Controls

- **Retry Policy**: Exponential backoff with configurable attempts
- **Idempotency Store**: Prevents duplicate processing
- **Circuit Breaker**: Prevents cascading failures
- **Dead Letter Queue**: Captures failed messages for inspection

## Documentation

- [Quickstart Guide](docs/src/quickstart.md)
- [Agent Core Documentation](docs/src/agent-core/README.md)
- [Tool Documentation](docs/src/tools/README.md)
- [Storage Documentation](docs/src/storage/README.md)
- [Design Plans](docs/src/plans/README.md)

## License

MIT - see [LICENSE](LICENSE)
