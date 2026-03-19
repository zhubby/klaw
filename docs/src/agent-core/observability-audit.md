# Observability and Audit Baseline

## 实现

可观测性功能由 `klaw-observability` 模块提供,基于 OpenTelemetry 实现。

### 配置示例

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

## 1. Metrics（最小闭环）

必须覆盖的指标：

| 指标名 | 类型 | 标签 | 描述 |
|--------|------|------|------|
| `agent_inbound_consumed_total` | Counter | session_key, provider | 入站消息消费计数 |
| `agent_outbound_published_total` | Counter | session_key, provider | 出站消息发布计数 |
| `agent_run_duration_ms` | Histogram | session_key, stage | 运行时长分布 (P50/P95/P99) |
| `agent_tool_success_total` | Counter | session_key, tool_name | 工具成功计数 |
| `agent_tool_failure_total` | Counter | session_key, tool_name, error_code | 工具失败计数 |
| `agent_retry_total` | Counter | session_key, error_code | 重试计数 |
| `agent_deadletter_total` | Counter | session_key | 死信计数 |
| `agent_session_queue_depth` | Gauge | session_key | 会话队列深度 |

## 2. Tracing（端到端）

- 统一使用 `trace_id` 串联：
  - ingress consume
  - runtime stages
  - tool calls
  - provider calls
  - egress publish
- 每个 stage 记录 span：
  - `validate`
  - `schedule`
  - `context_build`
  - `model_call`
  - `tool_loop`
  - `publish`

### 采样策略

使用概率采样,默认采样率 10% (`sample_rate = 0.1`),可在配置中调整。

## 3. Audit Events

审计事件必须结构化输出,至少包含：

| 字段 | 类型 | 描述 |
|------|------|------|
| `event_name` | String | 事件名称 |
| `trace_id` | UUID | 追踪 ID |
| `timestamp` | String (RFC3339) | 事件时间戳 |
| `session_key` | String (可选) | 会话键 |
| `error_code` | String (可选) | 错误码 |
| `payload` | JSON | 扩展负载 |

关键事件列表：

| 事件名 | 触发时机 |
|--------|----------|
| `inbound_received` | 收到入站消息 |
| `validation_failed` | 消息校验失败 |
| `session_queued` | 会话入队 |
| `tool_called` | 工具调用 |
| `tool_failed` | 工具调用失败 |
| `provider_fallback` | Provider 切换 |
| `final_response_published` | 最终响应发布 |
| `message_sent_dlq` | 消息发送到死信队列 |

## 4. Health Model

健康状态枚举：

| 状态 | 含义 |
|------|------|
| `Ready` | 依赖就绪,可接流量 |
| `Live` | 进程存活 |
| `Degraded` | 可用但功能受限 |
| `Unavailable` | 不可用 |

组件注册：

- `provider` - LLM Provider 状态
- `transport` - 传输层状态
- `store` - 存储层状态

## 5. API 使用

```rust
use klaw_observability::{init_observability, OtelAgentTelemetry};

// 初始化
let handle = init_observability(&config)?;
let telemetry = OtelAgentTelemetry::from_handle(&handle, "klaw");

// 记录指标
telemetry.incr_counter(
    "agent_inbound_consumed_total",
    &[("session_key", "session:123"), ("provider", "openai")],
    1
).await;

// 记录时长
telemetry.observe_histogram(
    "agent_run_duration_ms",
    &[("session_key", "session:123"), ("stage", "process")],
    std::time::Duration::from_millis(150)
).await;

// 发送审计事件
telemetry.emit_audit_event(
    "tool_called",
    uuid::Uuid::new_v4(),
    serde_json::json!({"tool_name": "shell", "session_key": "session:123"})
).await;

// 设置健康状态
telemetry.set_health("provider", klaw_core::HealthStatus::Ready).await;
```

## 6. 导出器

### OTLP

支持通过 gRPC 导出到 OTLP 兼容后端 (如 Jaeger, Prometheus, Grafana Tempo)。

### Prometheus

在 Gateway 中暴露 `/metrics` 端点,支持 Prometheus 拉取。

## 7. Gateway 端点

| 路径 | 方法 | 描述 |
|------|------|------|
| `/metrics` | GET | Prometheus 格式指标 |
| `/health/live` | GET | Liveness probe |
| `/health/ready` | GET | Readiness probe |
| `/health/status` | GET | 返回 JSON `{ "status": "Ready|Live|Degraded|Unavailable" }` |