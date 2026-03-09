# Delivery Roadmap (M1-M4)

## M1: Architecture Freeze

### Scope
- 核心 trait 冻结（transport/scheduler/reliability/telemetry）
- 协议冻结（Envelope、topic、error code、versioning）
- 状态机冻结（state + transition + queue policy）

### Exit Criteria
- trait 与文档一致并通过代码评审
- `message-protocol.md`、`runtime-state-machine.md` 完整
- 关键字段与错误码无冲突

## M2: Runtime Skeleton

### Scope
- `AgentLoop` 骨架可运行
- `SessionScheduler` 可插拔接口接入
- InMemory transport mock（用于本地回归）

### Exit Criteria
- 单条消息可走通 `inbound -> runtime -> outbound`
- 会话串行策略在 mock 环境可验证
- 核心路径有基础单测

## M3: Reliability Closure

### Scope
- 幂等存储接入
- retry/backoff/DLQ 完整接入
- timeout/circuit breaker/budget guard 生效

### Exit Criteria
- 故障注入场景（provider down/tool timeout/transport flap）可恢复或可观测失败
- 重放消息不会重复写会话
- DLQ 负载可用于手动 replay

## M4: Adapter Integration + Benchmark

### Scope
- 首个真实 MQ 适配器实现（按业务选择）
- 监控面板与告警规则接入
- 基准压测与容量评估

### Exit Criteria
- 达成目标吞吐与延迟 SLA
- 关键指标告警可触发与恢复
- 发布手册与回滚手册可执行

## Suggested Execution Order

1. 先保证 M1 文档与代码接口一致
2. 用 M2 mock 跑通端到端链路
3. 在 M3 再引入故障和边界场景
4. M4 最后绑定具体 MQ，降低早期耦合风险
