<div align="center">
  <h1>Klaw</h1>
  <img src="klaw-gui/assets/icons/logo.iconset/icon_128x128@2x.png" alt="Klaw logo" width="128" />
  <p>Crab ❤️ Claw.</p>
</div>

## Core Design

```
User Input → InboundMessage → AgentLoop → OutboundMessage
                              ↓
                         DeadLetterQueue
```

### Key Components

- **AgentLoop** (`klaw-core`): State machine driving sessions (`Received` → `Validating` → `Scheduling` → `CallingModel` → `ToolLoop` → `Completed`)
- **SessionScheduler**: Serial execution per `session_key` with configurable queue strategies
- **Reliability**: Retry policies, idempotency stores, circuit breakers, DLQ
- **Tool System**: Trait-based abstraction (shell, fs, web, memory, sub-agent)
- **Transport**: In-memory pub/sub with multi-channel support

### Workspace

| Crate | Purpose |
|-------|---------|
| `klaw-config` | TOML config (`~/.klaw/config.toml`) |
| `klaw-tool` | Tool trait & built-ins |
| `klaw-core` | Agent runtime, scheduler, reliability |
| `klaw-cli` | CLI binary (`klaw`) |
| `klaw-storage` | Session/cron persistence |
| `klaw-memory` | Long-term memory (BM25 + Vector) |
| `klaw-skill` | Skills lifecycle |
| `klaw-mcp` | Model Context Protocol |

## Quick Start

```bash
cargo build --workspace
cargo test --workspace

# Run
klaw                            # Launch GUI
klaw stdio                      # Interactive
klaw agent --input "prompt"     # One-shot
klaw gateway                    # WebSocket
```

## macOS Packaging

Build a native macOS app bundle and dmg from the existing GUI entrypoint:

```bash
make build-macos-app
make package-macos-dmg
```

Artifacts are written to `dist/macos/`:

- `dist/macos/Klaw.app`
- `dist/macos/Klaw-<version>-aarch64-apple-darwin.dmg`

Run skip quarantine

`xattr -rd com.apple.quarantine /Applications/Klaw.app`

See `docs/` for architecture details.

## License

MIT
