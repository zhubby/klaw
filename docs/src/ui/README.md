# UI 文档

本目录包含 Klaw 所有用户界面相关的文档：

- **桌面端 (klaw-gui)** - 原生桌面应用
- **Web 端 (klaw-webui)** - 浏览器 WASM 聊天客户端

## 内容

- [桌面架构设计](architecture.md) - 桌面端完整架构、模块结构、设计模式
- [实时日志流](log-stream.md) - GUI 工作区中的进程内实时日志
- [WASM 中文字体方案](wasm-cjk-fonts.md) - eframe/egui WASM 零打包中文字体方案
- [WebSocket Channel](websocket-channel.md) - WebSocket Channel 架构、协议与使用指南
- [WebUI 架构](webui-architecture.md) - klaw-webui 浏览器端 WASM 聊天客户端架构设计

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

## 面板文档

每个功能面板有单独的说明文档：

| 文档 | 功能 |
|------|------|
| [ACP](gui/acp.md) | ACP (Agent Connect Protocol) 客户端管理 |
| [分析仪表盘](gui/analyze-dashboard.md) | 系统运行数据分析统计 |
| [审批](gui/approval.md) | 人工审批请求管理 |
| [归档](gui/archive.md) | 归档会话管理 |
| [渠道](gui/channel.md) | 输入渠道配置管理 (WebSocket/DingTalk/Telegram...) |
| [配置](gui/configuration.md) | 全局配置编辑 |
| [Cron](gui/cron.md) | 定时任务管理 |
| [网关](gui/gateway.md) | HTTP/WebSocket 网关监控 |
| [Heartbeat](gui/heartbeat.md) | 会话心跳监控 |
| [LLM](gui/llm.md) | LLM 请求审计日志 |
| [Logs](gui/logs.md) | 实时进程日志查看 |
| [MCP](gui/mcp.md) | Model Context Protocol 服务器配置 |
| [Memory](gui/memory.md) | 记忆系统管理 |
| [Monitor](gui/monitor.md) | 系统监控（已弃用，见 system）|
| [Observability](gui/observability.md) | 可观测性配置 |
| [Profile](gui/profile.md) | 个人资料与系统提示词编辑 |
| [Provider](gui/provider.md) | LLM 模型提供者配置 |
| [Session](gui/session.md) | 对话会话管理 |
| [Setting](gui/setting.md) | 应用设置 |
| [Skills Manager](gui/skills-manager.md) | 已安装技能管理 |
| [Skills Registry](gui/skills-registry.md) | 技能注册表浏览安装 |
| [System](gui/system.md) | 系统信息与资源监控 |
| [Terminal](gui/terminal.md) | 嵌入式终端 |
| [Tool](gui/tool.md) | 工具状态与调用历史 |
| [Voice](gui/voice.md) | 语音交互配置 |
| [Webhook](gui/webhook.md) | Webhook 配置与请求历史 |
