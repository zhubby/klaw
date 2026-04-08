# Heartbeat 模块设计

## 背景

这里的 heartbeat 参考 OpenClaw 的 autonomous heartbeat 语义，不是 WebSocket、HTTP 或 channel 连接层的 ping/pong 保活，而是系统按固定周期向某个 agent session 注入一条特殊消息，让 agent 自主检查当前上下文并决定是否需要继续行动。

对 `klaw` 来说，这种 heartbeat 本质上是一种“周期性 agent turn”。它需要复用现有消息处理链路、会话串行调度和 session 存储能力，但又不应把 heartbeat 的业务语义直接污染到通用 cron 模块。

## 设计结论

首版 heartbeat 采用以下方案：

- 新增独立 crate：`klaw-heartbeat`
- heartbeat 作为独立领域模块存在，底层调度与持久化复用现有 `klaw-cron` 和 `CronStorage`
- 不新增 heartbeat 专属数据库表
- heartbeat 粒度按 `session_key` 管理
- v1 只支持显式配置的 session heartbeat
- v1 目标是 agent 自驱 heartbeat，不包含 gateway/channel 的 transport keepalive

## 模块放置

### `klaw-heartbeat`

负责 heartbeat 的领域语义，包括：

- `HeartbeatSpec` 配置模型
- `HeartbeatScheduler` 抽象
- heartbeat 与 `CronJob` 的双向映射
- heartbeat 专用 metadata 约定
- silent ack 判定逻辑

### `klaw-cron`

继续保持为通用定时调度模块，只负责：

- 扫描到期任务
- claim 下一次触发
- 发布标准化 `InboundMessage` 到 `agent.inbound`

`klaw-cron` 不理解 heartbeat 的业务语义。

### `klaw-storage`

继续通过 `CronStorage` 提供通用 cron 持久化能力，不为 v1 heartbeat 增加新表。

### `klaw-cli runtime`

heartbeat 的系统集成点放在 runtime bootstrap 和 runtime 出口：

- 启动时根据配置 reconcile heartbeat cron
- agent 执行完成后，判断 heartbeat 回复是否应静默丢弃

## 实现方案

### 领域模型

```rust
pub struct HeartbeatSpec {
    pub session_key: String,
    pub channel: String,
    pub chat_id: String,
    pub enabled: bool,
    pub every: String,
    pub prompt: String,
    pub silent_ack_token: String,
    pub timezone: String,
}
```

```rust
pub trait HeartbeatScheduler {
    async fn reconcile(&self, specs: &[HeartbeatSpec]) -> Result<(), HeartbeatError>;
}
```

### 与 cron 的映射

每个 heartbeat 映射为一个保留前缀的 cron job：

- `cron.id = "heartbeat:<session_key>"`
- `cron.name = "heartbeat:<session_key>"`
- `schedule_kind = every`
- `schedule_expr = spec.every`

`payload_json` 存储标准 `InboundMessage` JSON，用于被 `CronWorker` 直接发布。

### heartbeat 入站消息约定

heartbeat 触发时发布的 `InboundMessage` 建议包含：

- `channel`: 可保留原 channel
- `sender_id = "system-heartbeat"`
- `chat_id = spec.chat_id`
- `session_key = spec.session_key`
- `content = spec.prompt`

metadata 约定：

- `trigger.kind = "heartbeat"`
- `heartbeat.session_key = <session_key>`
- `heartbeat.silent_ack_token = "HEARTBEAT_OK"`

### silent ack 语义

首版约定 heartbeat 的静默回复 token 为：

```text
HEARTBEAT_OK
```

如果 agent 对 heartbeat 的最终输出在 `trim()` 后精确等于该 token，则视为“本次 heartbeat 已确认，但没有用户可见动作”。

此时系统行为为：

- 不发布 outbound
- 不写 assistant chat record
- 不增加 session turn 计数

如果 heartbeat 输出不是该 token，则按正常 assistant 消息处理，继续：

- 发布 outbound
- 追加 session chat record
- 完成 turn 计数

## 配置设计

建议在 `klaw-config` 中新增 heartbeat 配置：

```toml
[heartbeat.defaults]
enabled = true
every = "30m"
prompt = "Review the session state. If no user-visible action is needed, reply with exactly HEARTBEAT_OK and nothing else."
silent_ack_token = "HEARTBEAT_OK"
timezone = "Asia/Shanghai"

[[heartbeat.sessions]]
session_key = "terminal:main"
chat_id = "main"
channel = "terminal"
```

规则：

- `defaults` 提供默认值
- `sessions` 按 `session_key` 显式列出需要开启 heartbeat 的会话
- session 级配置可以覆盖默认值
- v1 不做 session 自动发现

## 运行链路

heartbeat 的完整链路如下：

1. runtime 启动并加载 heartbeat 配置
2. `klaw-heartbeat` 将配置转换为 `HeartbeatSpec`
3. bootstrap 阶段执行 reconcile
4. heartbeat 被落为持久化 cron job
5. `CronWorker` 定时扫描到期 heartbeat
6. worker 发布 heartbeat `InboundMessage` 到 `agent.inbound`
7. `AgentLoop` 按普通消息流程执行
8. runtime 出口判断本次输出是否为 silent ack
9. silent ack 则静默丢弃，否则作为正常 assistant 输出处理

## 为什么不直接做成 cron 用例

虽然 heartbeat 在调度层与 cron 很像，但它并不只是“定时发一条消息”。heartbeat 还带有以下专有语义：

- 固定的 heartbeat metadata
- silent ack 语义
- 按 session 生命周期管理
- 后续可能扩展 quiet hours、活跃条件、失败补偿等策略

这些都不是通用 cron 的职责。把 heartbeat 直接塞进 `klaw-cron`，会让 cron 模块承担业务语义，后续很容易变脏。

因此更合理的做法是：heartbeat 拥有独立领域层，底层仅复用 cron 作为触发机制。

## 测试方案

需要覆盖以下场景：

1. `HeartbeatSpec` 正确映射为 `CronJob`
2. 启动时缺失的 heartbeat cron 能被创建
3. heartbeat 配置更新后可正确同步到已有 cron
4. heartbeat 禁用后对应 cron 会被禁用
5. `CronWorker` 触发 heartbeat 时发布的 inbound 包含完整 metadata
6. heartbeat 输出为 `HEARTBEAT_OK` 时不会产生 outbound
7. heartbeat 输出为 `HEARTBEAT_OK` 时不会写 session chat record，也不会增加 turn
8. heartbeat 输出真实文本时按正常 assistant 消息处理
9. heartbeat 与用户消息共用 `session_key` 时仍遵守现有会话串行调度
10. 系统重启后 heartbeat 可通过持久化 cron 自动恢复

## 后续演进

后续如果 heartbeat 成为核心能力，可以继续扩展：

- quiet hours / 活跃时段
- 连续失败后的重试与告警
- heartbeat 运行统计与审计
- 独立 heartbeat 运行表
- transport keepalive 子系统（与 agent heartbeat 分离设计）
