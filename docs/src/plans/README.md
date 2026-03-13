# 设计计划

本目录收录 Klaw 的设计计划和演进方案。

## 当前计划

### [Memory Turso Hybrid](./memory-turso-hybrid.md)

Memory 模块实施计划（Turso/libSQL 优先）：

- **目标架构**：BM25(FTS5) + Vector(libSQL native)
- **回退路径**：本地 SQLite（BM25 only）
- **关键约束**：Turso 路径不依赖 `sqlite-vec`，使用 `F32_BLOB + libsql_vector_idx`

涉及 crate：
- `klaw-config` - 配置模型扩展
- `klaw-storage` - memory.db 管理
- `klaw-memory` - MemoryService trait 与实现

### [Heartbeat 模块设计](./heartbeat-module-design.md)

自主心跳心跳机制设计：

- 系统按固定周期向 agent session 注入特殊消息
- agent 自主检查上下文并决定是否需要继续行动
- silent ack 语义：`HEARTBEAT_OK` 回复不产生 outbound

模块放置：
- `klaw-heartbeat` - 独立领域模块
- `klaw-cron` - 复用为底层调度机制
- `klaw-storage` - 复用 `CronStorage`（v1 不新增表）

## 已完成计划

- [x] Agent Core 基座设计（M1）
- [x] Runtime Skeleton（M2）
- [x] Reliability Closure（M3）
- [x] WebSocket Gateway

## 路线图参考

参见 [Agent Core Roadmap M1-M4](../agent-core/roadmap-m1-m4.md)
