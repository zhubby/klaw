# klaw-observability

提供可观测性基础设施,包括指标、追踪、审计和健康检查。

## 功能

- **Metrics**: 支持 OTLP 和 Prometheus 双导出
- **Tracing**: 基于 OpenTelemetry 的分布式追踪,支持概率采样
- **Audit**: 结构化审计事件记录
- **Health**: 组件健康状态管理

## 使用

```rust
use klaw_observability::{init_observability, OtelAgentTelemetry};

let config = ObservabilityConfig {
    enabled: true,
    service_name: "klaw".to_string(),
    ..Default::default()
};

let handle = init_observability(&config)?;
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
```

## 指标

| 指标名 | 类型 | 描述 |
|--------|------|------|
| `agent_inbound_consumed_total` | Counter | 入站消息消费计数 |
| `agent_outbound_published_total` | Counter | 出站消息发布计数 |
| `agent_run_duration_ms` | Histogram | 运行时长分布 |
| `agent_tool_success_total` | Counter | 工具成功计数 |
| `agent_tool_failure_total` | Counter | 工具失败计数 |
| `agent_retry_total` | Counter | 重试计数 |
| `agent_deadletter_total` | Counter | 死信计数 |
| `agent_session_queue_depth` | Gauge | 会话队列深度 |