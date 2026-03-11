# Agent Core Baseline (MQ-first)

本目录是 Rust Agent 基座（MQ 抽象优先）的落地规范：

- `message-protocol.md`: Envelope、topic、schema 演进、错误码
- `runtime-state-machine.md`: AgentLoop 状态机与会话调度策略
- `reliability-controls.md`: 重试、幂等、熔断、预算、DLQ
- `observability-audit.md`: metrics/tracing/audit/health 最小闭环
- `roadmap-m1-m4.md`: 分阶段交付与验收标准

对应代码骨架位于 `crates/core/src`。

## Local stdio run

可直接本地运行（不依赖外部 MQ）：

`klaw stdio`

- 输入任意文本并回车
- 输入 `/exit` 退出
