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
| `agent_llm_request_total` | Counter | session_key, provider, model, status | 模型请求计数 |
| `agent_llm_request_duration_ms` | Histogram | session_key, provider, model, status | 模型请求时延分布 |
| `agent_llm_tokens_total` | Counter | session_key, provider, model, token_type | 模型 token 消耗计数 |
| `agent_model_tool_success_total` | Counter | session_key, provider, model, tool_name | 模型归因工具成功计数 |
| `agent_model_tool_failure_total` | Counter | session_key, provider, model, tool_name, error_code | 模型归因工具失败计数 |
| `agent_turn_completed_total` | Counter | session_key, provider, model | 成功完成 turn 计数 |
| `agent_turn_degraded_total` | Counter | session_key, provider, model | 降级 turn 计数 |
| `agent_retry_total` | Counter | session_key, error_code | 重试计数 |
| `agent_deadletter_total` | Counter | session_key | 死信计数 |
| `agent_session_queue_depth` | Gauge | session_key | 会话队列深度 |

## 本地分析存储扩展

除工具层统计外,本地 SQLite analysis store 现在还会保存:

- `llm_metric_events` / `llm_metric_minute_rollups`: provider/model 请求成功率、时延、token 结构、超时率、空响应率
- `model_tool_metric_events` / `model_tool_metric_minute_rollups`: 按发起模型归因的工具成功率、审批率、耗时
- `turn_metric_events` / `turn_metric_minute_rollups`: 每轮请求数、tool iterations、完成率、降级率、budget/tool loop 命中率

GUI `Analyze Dashboard` 的 `Models` 视图直接消费这些聚合数据。

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

## 8. Local Analysis Store

`klaw-observability` 提供本地 SQLite 分析存储，用于持久化工具调用统计，供 GUI Analyze Dashboard 查询。

### 功能特性

- **本地 SQLite 存储** - 工具调用统计持久化
- **保留期配置** - 可配置数据保留天数
- **定期刷新** - 可配置刷新间隔
- **GUI 集成** - Analyze Dashboard 直接查询

### 配置

```toml
[observability.local_store]
enabled = true
retention_days = 7
flush_interval_seconds = 5
```

### 数据模型

**ToolCallStat 结构**：

```rust
pub struct ToolCallStat {
    pub tool_name: String,
    pub call_count: i64,
    pub success_count: i64,
    pub failure_count: i64,
    pub avg_duration_ms: f64,
    pub last_called_at: i64,
}
```

### API 使用

```rust
use klaw_observability::LocalAnalysisStore;

let store = LocalAnalysisStore::new(&config)?;

// 记录工具调用
store.record_tool_call(
    "shell",
    std::time::Duration::from_millis(150),
    true,  // success
).await?;

// 查询统计数据
let stats = store.list_tool_stats().await?;
for stat in stats {
    println!("{}: {} calls, {:.2}ms avg",
        stat.tool_name,
        stat.call_count,
        stat.avg_duration_ms
    );
}
```

### GUI Analyze Dashboard

**功能特性**：

| 功能 | 描述 |
|------|------|
| **工具统计列表** | 显示所有工具的调用统计 |
| **成功率可视化** | 进度条显示成功率百分比 |
| **平均耗时** | 显示每个工具的平均执行时间 |
| **调用次数** | 显示调用次数和成功/失败数 |
| **排序功能** | 按调用次数、成功率、平均耗时排序 |

**面板字段**：

| 字段 | 说明 |
|------|------|
| Tool Name | 工具名称 |
| Calls | 总调用次数 (成功/失败) |
| Success Rate | 成功率百分比 + 进度条 |
| Avg Duration | 平均执行时间 |
| Last Called | 最后调用时间 |

### 数据保留

- 默认保留 7 天数据
- 后台任务定期清理过期数据
- 保留期可通过 `retention_days` 配置

### 刷新机制

- 默认每 5 秒刷新一次内存统计到 SQLite
- 刷新间隔可通过 `flush_interval_seconds` 配置
- 应用退出时会强制刷新

## 9. 相关文档

- [LLM 审计跟踪](./llm-audit.md) - LLM 请求/响应审计
- [配置概述](../configration/overview.md) - 配置模型
