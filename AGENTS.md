# Agents Project Guide

## Module Capabilities

### `crates/core`
- 核心领域模型：`InboundMessage`、`OutboundMessage`、`DeadLetterMessage`
- 消息协议与错误码：`Envelope`、`MessageTopic`、`ErrorCode`
- 传输抽象：`MessageTransport`（publish/consume/ack/nack/requeue）
- 会话调度抽象：`SessionScheduler`
- 可靠性抽象：`RetryPolicy`、`DeadLetterPolicy`、`CircuitBreaker`、`IdempotencyStore`
- 运行时主循环：`agent::AgentLoop`（含 `run_once`、`run_once_reliable`）
- 内存 Mock：`InMemoryTransport`、`InMemoryIdempotencyStore`、`InMemorySessionScheduler`

### `crates/llm`
- `LlmProvider` 抽象
- 统一 LLM 输入输出结构：`LlmMessage`、`ToolDefinition`、`LlmResponse`
- 本地占位实现：`EchoProvider`

### `crates/tool`
- `Tool` 抽象与 `ToolRegistry`
- `ToolContext`、`ToolOutput`、`ToolCategory`
- 本地占位实现：`EchoTool`

### `crates/cli`
- 项目启动入口
- 本地 `stdin/stdout` 交互运行模式（`cargo run -p klaw-cli`）

## Project Conventions

- 架构原则：核心运行时不依赖具体 MQ/具体 Provider/具体 Tool 实现。
- 依赖方向：`cli -> core`，`llm -> core`，`tool -> core`；禁止反向依赖。
- 可靠性基线：默认 at-least-once + 幂等去重 + 重试 + DLQ。
- 可扩展方式：新增 MQ/LLM/Tool 通过实现 trait 接入，不改主链路。
- 测试要求：核心链路至少覆盖成功路径与重试耗尽进 DLQ 路径。
- 入口规范：统一通过 `crates/cli` 启动，避免在库 crate 中混放 bin 入口。
