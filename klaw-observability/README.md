# klaw-observability

提供可观测性基础设施,包括指标、追踪、审计、健康检查和本地分析存储。

## 功能

- **Metrics**: 支持 OTLP 和 Prometheus 双导出
- **Tracing**: 基于 OpenTelemetry 的分布式追踪,支持概率采样
- **Audit**: 结构化审计事件记录
- **Health**: 组件健康状态管理
- **Local Analysis Store**: 本地 SQLite 工具调用、模型请求、模型归因工具结果与 turn 效率统计,供 GUI `Analyze Dashboard` 直接查询

## 使用

```rust
use klaw_observability::{init_observability, OtelAgentTelemetry};

let config = ObservabilityConfig {
    enabled: true,
    service_name: "klaw".to_string(),
    ..Default::default()
};

let handle = init_observability(&config, None).await?;
let telemetry = OtelAgentTelemetry::from_handle(&handle, "klaw");

// 使用 telemetry 进行埋点
telemetry.incr_counter("agent_inbound_consumed_total", &[("session_key", "session:123")], 1).await;
```

## 配置

```toml
[observability]
enabled = true
service_name = "klaw"
service_version = "0.1.0"

[observability.metrics]
enabled = true
export_interval_seconds = 30

[observability.traces]
enabled = true
sample_rate = 0.1

[observability.otlp]
enabled = true
endpoint = "http://localhost:4317"

[observability.prometheus]
enabled = true
listen_port = 9090
path = "/metrics"

[observability.audit]
enabled = true
output_path = "/var/log/klaw/audit.log"

[observability.local_store]
enabled = true
retention_days = 7
flush_interval_seconds = 5
```

## 指标

| 指标名 | 类型 | 描述 |
|--------|------|------|
| `agent_inbound_consumed_total` | Counter | 入站消息消费计数 |
| `agent_outbound_published_total` | Counter | 出站消息发布计数 |
| `agent_run_duration_ms` | Histogram | 运行时长分布 |
| `agent_tool_success_total` | Counter | 工具成功计数 |
| `agent_tool_failure_total` | Counter | 工具失败计数 |
| `agent_llm_request_total` | Counter | 模型请求计数 |
| `agent_llm_request_duration_ms` | Histogram | 模型请求时延分布 |
| `agent_llm_tokens_total` | Counter | 模型 token 消耗计数 |
| `agent_model_tool_success_total` | Counter | 按模型归因的工具成功计数 |
| `agent_model_tool_failure_total` | Counter | 按模型归因的工具失败计数 |
| `agent_turn_completed_total` | Counter | 成功完成 turn 计数 |
| `agent_turn_degraded_total` | Counter | 降级 turn 计数 |
| `agent_retry_total` | Counter | 重试计数 |
| `agent_deadletter_total` | Counter | 死信计数 |
| `agent_session_queue_depth` | Gauge | 会话队列深度 |
