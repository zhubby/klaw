# 设计计划

本目录收录 Klaw 的设计计划和演进方案。

## 当前计划

### [Daemon Management](./daemon-management.md)

`klaw daemon` 守护进程管理计划：

- **托管目标**：仅管理 `klaw gateway`
- **平台范围**：`launchd` 用户级 + `systemd --user`
- **阶段目标**：先补齐安装/状态/卸载，再覆盖 start/stop/restart 生命周期控制

### [Memory Turso Hybrid](./memory-turso-hybrid.md)

Memory 模块实施计划（Turso/libSQL 优先）：

- **目标架构**：BM25(FTS5) + Vector(libSQL native)
- **回退路径**：本地 SQLite（BM25 only）
- **关键约束**：Turso 路径不依赖 `sqlite-vec`，使用 `F32_BLOB + libsql_vector_idx`

涉及 crate：
- `klaw-config` - 配置模型扩展
- `klaw-storage` - memory.db 管理
- `klaw-memory` - MemoryService trait 与实现

### [两层 Memory 设计](./two-layer-memory-design.md)

当前 memory 架构调整方案：

- **长期记忆**：写入 `long_term`，每轮整理后注入 `system prompt`
- **治理规则**：`kind/status/topic/supersedes` 正式化，支持冲突替换
- **session 记忆**：复用现有 `session/chat` 存储，仅通过 `search scope=session` 检索

### [Heartbeat 模块设计](./heartbeat-module-design.md)

自主心跳心跳机制设计：

- 系统按固定周期向 agent session 注入特殊消息
- agent 自主检查上下文并决定是否需要继续行动
- silent ack 语义：`HEARTBEAT_OK` 回复不产生 outbound

模块放置：

- `klaw-heartbeat` - 独立领域模块
- `klaw-cron` - 复用为底层调度机制
- `klaw-storage` - 复用 `CronStorage`（v1 不新增表）

### [ACP Client 集成设计](./acp-client-integration.md)

ACP 客户端接入设计:

- **定位**：让 klaw 成为 ACP client，调用外部 ACP Agent
- **目标能力**：以 tool 形式集成 Claude Code、Codex CLI 等编码 Agent
- **复用模式**：沿用 `klaw-mcp` 的 `manager + hub + proxy tool` 架构

涉及 crate：
- `klaw-acp` - ACP 客户端生命周期与连接管理
- `klaw-config` - ACP agent 配置模型
- `klaw-cli` - runtime 启动与 tool 注册
- `klaw-tool` - 复用 Tool 抽象承载 ACP proxy tool

### [Voice 模块设计](./voice-module-design.md)

语音转文字（STT）和文字转语音（TTS）能力设计：

- STT 在 Channel 层直接调用，转文字后提交给 runtime
- TTS 通过 ToolSignal 机制触发 channel 发送
- 支持流式 TTS 和 STT
- 供应商：ElevenLabs/Deepgram/AssemblyAI

模块放置：

- `klaw-voice` - 独立领域模块
- `klaw-tool` - 新增 TtsTool
- `klaw-channel` - 集成 STT 调用

## 已完成计划

- [x] Agent Core 基座设计（M1）
- [x] Runtime Skeleton（M2）
- [x] Reliability Closure（M3）
- [x] WebSocket Gateway

## 路线图参考

参见 [Agent Core Roadmap M1-M4](../agent-core/roadmap-m1-m4.md)
