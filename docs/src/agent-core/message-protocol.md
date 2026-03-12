# Message Protocol Specification

## 1. Envelope

所有跨模块消息统一包裹为 `Envelope<T>`：

- `header.message_id`: 全局唯一消息 ID（UUID）
- `header.trace_id`: 链路追踪 ID（UUID）
- `header.session_key`: 会话串行键，格式建议 `"{source}:{chat_id}"`
- `header.timestamp`: 消息创建时间
- `header.attempt`: 当前处理尝试次数，从 1 递增
- `header.schema_version`: 协议版本
- `header.tenant_id`: 多租户隔离（可选）
- `header.namespace`: 环境/业务命名空间（可选）
- `header.priority`: 优先级（可选，范围建议 0-9）
- `header.ttl_ms`: 生存时间（可选）
- `header.routing_hints`: 路由 Hint（可选，面向适配器）
- `metadata`: 非路由业务扩展信息（provider/model override 等）
- `payload`: 具体业务消息（Inbound/Outbound/Event）

## 2. Logical Topics

- `agent.inbound`: 进入 Agent 运行时的入站消息
- `agent.outbound`: Agent 最终回复消息
- `agent.events`: 中间事件（stream chunk/tool progress/degraded）
- `agent.dlq`: 达到死信条件的消息

## 3. Versioning & Compatibility

协议遵守“同主版本向后兼容”：

1. 非破坏性变更：仅允许新增 optional 字段（minor +1）
2. 破坏性变更：字段语义变化、字段移除、必填新增（major +1）
3. 消费方必须忽略未知字段，保证滚动升级安全
4. 降级读取策略：无法识别新字段时，按默认值执行

## 4. Error Codes

| Code | Type | Retry | Description |
|---|---|---|---|
| `InvalidSchema` | Validation | No | Envelope 与 schema 不匹配 |
| `ValidationFailed` | Validation | No | 业务字段校验失败 |
| `DuplicateMessage` | Idempotency | No | 命中去重键 |
| `SessionBusy` | Scheduling | Yes | 会话忙，进入重试或排队 |
| `AgentTimeout` | Runtime | Yes | Agent run 达到总超时 |
| `ToolTimeout` | Runtime | Yes | 单工具执行超时 |
| `ProviderUnavailable` | Dependency | Yes | 模型服务暂时不可用 |
| `ProviderResponseInvalid` | Dependency | No | 模型响应不合法 |
| `TransportUnavailable` | Infrastructure | Yes | MQ 或网络不可用 |
| `RetryExhausted` | Reliability | No | 达到最大重试次数 |
| `SentToDeadLetter` | Reliability | No | 已写入死信队列 |

## 5. Idempotency Key

默认去重键：

`{message_id}:{session_key}:{stage}`

- `stage` 建议值：`ingress`, `agent_run`, `egress`
- 对于可重放链路，`message_id` 不变，`attempt` 增长
