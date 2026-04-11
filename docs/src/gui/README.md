# GUI Documentation

This directory contains documentation for the Klaw desktop GUI application.

## Contents

- [Architecture Overview](architecture.md) - Complete GUI architecture, module structure, and design patterns
- [Live Log Stream](log-stream.md) - Real-time in-process logs in GUI workbench
- [WASM 中文字体方案](wasm-cjk-fonts.md) - eframe/egui WASM 零打包中文字体方案
- [WebSocket Channel](websocket-channel.md) - WebSocket channel 架构、协议与使用指南

## Quick Links

- [egui Documentation](https://docs.rs/egui)
- [eframe Documentation](https://docs.rs/eframe)
- [egui-phosphor](https://github.com/egui-phosphor/egui-phosphor)

## Overview

Klaw GUI is built with **egui**, an immediate-mode GUI framework in Rust. Key features:

- **Tabbed workbench** - Multi-panel workspace
- **State persistence** - Layout, theme, window size saved across sessions
- **21 feature panels** - Profile, Session, Provider, Memory, Skills, Heartbeat, Cron, Gateway, Webhook, LLM, Analyze, etc.
- **Toast notifications** - User feedback for operations
- **Theme support** - System/Light/Dark modes

## Panel Overview

| Panel | Description |
|-------|-------------|
| **Profile** | LLM provider profile management |
| **Session** | Chat session and message history |
| **Provider** | Active provider status and metrics |
| **Memory** | Memory entries management |
| **Skills Manager** | Installed skills management |
| **Skills Registry** | Browse and install skills from registries |
| **Heartbeat** | Heartbeat job scheduling and execution records |
| **Cron** | Scheduled job management and history |
| **Gateway** | WebSocket gateway status and port info |
| **Webhook** | Webhook request history and status |
| **LLM** | LLM request/response audit trail |
| **Analyze Dashboard** | Tool call statistics and analysis |
| **Approval** | Pending approval requests |
| **Archive** | Archived conversations |
| **Logs** | Real-time application logs |
| **Observability** | Metrics and tracing information |
| **Tool** | Tool call history and results |
| **System** | Environment check and system info |

## Module Structure

```
klaw-gui/src/
├── app/           # Main application (KlawGuiApp)
├── domain/        # Domain models (Menu)
├── ui/            # Shell, sidebar, workbench layout
├── panels/        # Feature panel implementations
├── state/         # UI state management & persistence
├── theme.rs       # Theme & fonts
├── notifications.rs
├── runtime_bridge.rs
└── widgets/
```
