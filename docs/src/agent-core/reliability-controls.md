# Reliability Controls

## 1. Baseline Guarantees

- 交付语义：`at_least_once`
- 一致性手段：幂等去重 + 会话串行
- 恢复目标：可重试错误自动恢复，不可恢复错误明确失败并可审计

## 2. Retry Pipeline

1. 失败分类（validation/dependency/infrastructure/budget）
2. 根据 `RetryPolicy` 生成决策：
   - `RetryNow`
   - `RetryAfter(backoff)`
   - `SendToDeadLetter`
   - `Abort`
3. 达到 `max_attempts` 后强制转 `agent.dlq`

指数退避建议：

- `base_delay = 200ms`
- `max_delay = 30s`
- `jitter_ratio = 0.2`

## 3. Idempotency

- 去重键：`{message_id}:{session_key}:{stage}`
- `stage`：`ingress | agent_run | egress`
- 写路径：
  - 开始处理前先 `seen(key)`，命中则短路
  - 处理成功后 `mark_seen(key, ttl)`
- TTL 建议：不小于最大消息保留时长

## 4. Circuit Breaker

- provider/tool/transport 独立熔断器
- 打开条件：`failure_threshold` 连续失败
- 恢复策略：
  - `open_interval` 内快速失败
  - 半开阶段仅允许 `half_open_max_requests` 探测

## 5. Budget Guards

- `max_tool_iterations`：工具轮数上限
- `max_tool_calls`：工具调用总量上限
- `token_budget`：单次 run token 上限
- 超限动作：
  - 优先降级到无工具回答
  - 无法降级则标记 `RetryExhausted` 或 `Failed`

## 6. DLQ Contract

- 死信主题：`agent.dlq`
- 死信负载应包含：
  - 原始 `Envelope`
  - 最终错误码与错误摘要
  - 最后 attempt 与失败时间
  - 推荐补偿动作（manual_replay/drop/inspect）
