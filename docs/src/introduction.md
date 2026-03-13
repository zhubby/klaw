# Klaw 简介

Klaw 是一个基于 Rust 的 AI Agent 框架，采用 MQ 风格的消息传递机制和可靠性控制设计。

## 核心特性

- **会话隔离**：基于 `session_key` 的串行执行保证，同会话消息顺序处理
- **可靠性保障**：重试策略（指数退避）、幂等存储、熔断器、死信队列
- **工具系统**：可扩展的工具抽象，内置 shell、web search、memory、sub-agent 等能力
- **Skills 支持**：兼容 Anthropic/Vercel skills，支持从 Git 仓库同步
- **多后端存储**：支持 Turso/libSQL 和 SQLx 后端，统一 trait 对外
- **消息总线**：Channel 与 Agent 通过 MessageBus 解耦，支持多平台接入

## 架构概览

```
User Input → InboundMessage (agent.inbound)
                    → AgentLoop.run_once_reliable()
                    → OutboundMessage (agent.outbound) → Response
                                               ↘ DeadLetterMessage (agent.dlq)
```

## 工作空间结构

| Crate | 职责 |
|-------|------|
| `klaw-config` | TOML 配置加载（`~/.klaw/config.toml`） |
| `klaw-tool` | 工具 trait 定义和内置工具（shell、fs、web 等） |
| `klaw-core` | Agent 运行时：消息协议、调度器、可靠性控制 |
| `klaw-cli` | CLI 入口（binary: `klaw`） |
| `klaw-storage` | 存储抽象层（session/cron持久化） |
| `klaw-gateway` | WebSocket 网关服务 |
| `klaw-skill` | Skills 生命周期管理 |
| `klaw-memory` | 长期记忆服务（BM25 + Vector） |

## 快速链接

- [快速开始](./quickstart.md)
- [Agent Core](./agent-core/README.md)
- [工具文档](./tools/README.md)
- [存储文档](./storage/README.md)
- [设计计划](./plans/README.md)
