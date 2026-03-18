# Observability and Audit Baseline

## 1. Metrics（最小闭环）

必须覆盖的指标：

- `agent_inbound_consumed_total`
- `agent_outbound_published_total`
- `agent_run_duration_ms`（P50/P95/P99）
- `agent_tool_success_total`
- `agent_tool_failure_total`
- `agent_retry_total`
- `agent_deadletter_total`
- `agent_session_queue_depth`

建议标签：

- `session_key`
- `provider`
- `tool_name`
- `error_code`

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

## 3. Audit Events

审计事件必须结构化输出，至少包含：

- `event_name`
- `trace_id`
- `session_key`
- `error_code`（可选）
- `payload`

关键事件列表：

- `inbound_received`
- `validation_failed`
- `session_queued`
- `tool_called`
- `tool_failed`
- `provider_fallback`
- `final_response_published`
- `message_sent_dlq`

## 4. Health Model

- `readiness`：依赖可用（transport/provider/store）
- `liveness`：主循环存活（poll loop alive）
- `degraded`：功能可用但性能或依赖异常

建议输出：

- `/health/live`
- `/health/ready`
- `/health/status`（返回 `Ready|Live|Degraded|Unavailable`）
