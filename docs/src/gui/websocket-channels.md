# WebSocket Channels

WebSocket channels allow external clients (such as the Klaw web UI, browser-based chat interfaces, or custom integrations) to connect to Klaw via WebSocket and receive agent responses streamed in real-time.

## Overview

Klaw's WebSocket channel system enables:

- **Real-time streaming**: LLM responses are streamed token-by-token to the connected client
- **Multiple concurrent connections**: Multiple WebSocket channels can be configured, each with different settings
- **Reasoning display**: Optional display of model reasoning alongside the final response
- **Integration-friendly**: Easy to integrate with web frontends, mobile apps, or other services

## Architecture

```
External Client → Gateway → WebSocket Connection → Channel Manager → Agent Execution
                                                              ↓
                         Streaming Response ← Channel ← Agent Response
```

The WebSocket channel integrates with:
- **klaw-channel**: Core channel abstraction and WebSocket driver
- **klaw-gateway**: HTTP/WebSocket gateway server that accepts incoming connections
- **klaw-gui**: Configuration panel for managing WebSocket channels

## Configuration in GUI

The Channel panel in the Klaw GUI allows you to:

- List all configured WebSocket channels
- Add new WebSocket channels
- Edit existing channel settings
- Toggle enable/disable channels
- Delete channels

### Channel Settings

| Setting | Description | Default |
|---------|-------------|---------|
| **ID** | Unique identifier for the channel (used in routing) | - |
| **Enabled** | Whether the channel accepts incoming connections | `true` |
| **Show Reasoning** | Include model reasoning in responses | `false` |
| **Stream Output** | Stream tokens incrementally instead of sending the full response at once | `true` |

### Example Configuration

In `~/.klaw/config.toml`, a WebSocket channel configuration looks like:

```toml
[channels]
websocket = [
    {
        id = "browser",
        enabled = true,
        show_reasoning = false,
        stream_output = true
    }
]
```

## Usage with Gateway

When the Klaw gateway is running with WebSocket support enabled, clients can connect to:

```
ws://host:port/ws/chat?channel={channel_id}&token={auth_token}
```

The authentication token matches the gateway's configured Bearer token for security.

## Message Protocol

### Incoming (Client → Server)

Clients send JSON formatted requests:

```json
{
  "jsonrpc": "2.0",
  "id": "request-id",
  "method": "session/update",
  "params": {
    "session_key": "my-session",
    "title": "Session Title",
    "content": "User message content"
  }
}
```

### Outgoing (Server → Client)

Server sends responses, either as a single message or streaming:

**Complete response (non-streaming):**
```json
{
  "jsonrpc": "2.0",
  "id": "request-id",
  "result": {
    "content": "Agent response content",
    "reasoning": "Optional reasoning text",
    "metadata": {}
  }
}
```

**Streaming (chunked):**
Multiple messages are sent as tokens are generated, with a final complete message.

## Use Cases

### 1. Klaw Web UI

The built-in web-based chat UI (`klaw-webui`) uses a WebSocket channel to connect to the backend:

```toml
# Example for web UI
[channels]
websocket = [
    { id = "web-ui", enabled = true, stream_output = true, show_reasoning = false }
]
```

### 2. Custom Browser Integration

Build your own custom chat interface in HTML/JavaScript and connect via the browser's built-in `WebSocket` API.

### 3. Mobile Apps

Connect from mobile apps to get real-time streaming responses from Klaw.

## GUI Panel Features

### Status Indicators

- **Enabled**: Channel is active and will accept incoming connections
- **Disabled**: Channel is inactive, connections will be rejected

### Form Validation

The GUI validates:
- Unique channel IDs (no duplicates)
- Non-empty ID field before saving
- Configuration reload after changes

## Related Documentation

- [Architecture Overview](architecture.md) - GUI architecture and module structure
- [Gateway Documentation](../server/gateway.md) - Gateway configuration and authentication
- [klaw-channel](https://github.com/klaw-rs/klaw/tree/main/klaw-channel) - Core channel implementation
