# Cron 存储语义

`klaw-cron` 与 `klaw-storage` 配合实现定时任务的持久化调度。触发目标是发布到 `agent.inbound`，执行链路复用现有 runtime。

## 调度类型

Cron 支持两种调度方式：

| 类型 | 表达式示例 | 说明 |
|------|-----------|------|
| **Cron** | `0 8 * * *` | 标准 cron 表达式（5-7 字段） |
| **Every** | `30m`, `1h`, `24h` | 固定间隔（humantime 格式） |

### 表达式格式

**Cron 表达式**：

- 5 字段：`分 时 日 月 周`（自动补秒为 `0`）
- 6 字段：`秒 分 时 日 月 周`
- 7 字段：`秒 分 时 日 月 周 年`

```text
0 8 * * *        # 每天 8:00 (自动转为 0 0 8 * * *)
0 0 8 * * *      # 每天 8:00:00
0 30 9 * * 1-5   # 周一到周五 9:30
```

**Every 表达式**：

```text
30s              # 每 30 秒
5m               # 每 5 分钟
1h               # 每 1 小时
24h              # 每 24 小时
every 30m        # 每 30 分钟（带 every 前缀）
```

**时间简写**：

```text
8:00             # 每天 8:00 (转为 0 0 8 * * *)
14:30            # 每天 14:30
```

### ScheduleSpec 解析

```rust
pub enum ScheduleSpec {
    Cron(cron::Schedule),      // 6-7 字段 cron 表达式
    Every(Duration),           // humantime 解析的间隔
}
```

解析顺序：

1. 尝试解析为时间简写（如 `8:00`）
2. 尝试解析为 humantime 间隔（如 `30m`）
3. 尝试解析为 cron 表达式

## 表结构

### `cron`（任务定义）

- `id`：任务主键
- `name`：任务名称
- `schedule_kind`：调度类型（`cron` 或 `every`）
- `schedule_expr`：表达式原文
- `payload_json`：入站消息 JSON 模板（可反序列化为 `InboundMessage`）
- `enabled`：是否启用
- `timezone`：时区（当前默认 `UTC`）
- `next_run_at_ms`：下一次触发时间
- `last_run_at_ms`：最近一次已被 claim 的计划时间
- `created_at_ms` / `updated_at_ms`：审计字段

## `cron_task`（运行记录）

- `id`：运行记录主键
- `cron_id`：关联任务 ID
- `scheduled_at_ms`：该次计划触发时间
- `started_at_ms` / `finished_at_ms`：执行起止时间
- `status`：`pending` / `running` / `success` / `failed`
- `attempt`：重试计数（当前首版默认 0）
- `error_message`：失败原因
- `published_message_id`：成功发布后的消息 ID
- `created_at_ms`：记录创建时间

## 索引

- `cron(enabled, next_run_at_ms)`：加速扫描到期任务
- `cron_task(cron_id, created_at_ms DESC)`：按任务查询历史
- `cron_task(status, scheduled_at_ms)`：按状态和计划时间过滤

## 并发防重（CAS）

worker 处理到期任务时不会直接执行，而是先调用：

- `claim_next_run(cron_id, expected_next_run_at_ms, new_next_run_at_ms, now_ms)`

该操作在数据库侧执行条件更新（`WHERE next_run_at_ms = expected...`），只有一个 worker 能成功 claim，避免同一触发时刻重复执行。

## 状态流转

单次运行典型过程：

1. `append_task_run(..., status=pending)`
2. `mark_task_running(...)`
3. 发布到 `agent.inbound`
4. 成功：`mark_task_result(..., status=success, published_message_id=...)`
5. 失败：`mark_task_result(..., status=failed, error_message=...)`

## 后端一致性

`turso` 与 `sqlx` 两个后端都实现了相同的 `CronStorage` trait，并在初始化时自动建表与建索引，保持上层行为一致。

## CronWorker 调度流程

### 核心调度逻辑

```rust
pub async fn run_tick(&self) -> Result<usize, CronError> {
    let now = now_ms();
    
    // 1. 查询到期的 cron 任务
    let due_jobs = self.storage
        .list_due_crons(now, self.config.batch_limit)
        .await?;
    
    let mut executed = 0usize;
    
    for job in due_jobs {
        // 2. 计算下一次运行时间
        let schedule = ScheduleSpec::from_job(&job)?;
        let next_run_at_ms = schedule.next_run_after_ms(job.next_run_at_ms)?;
        
        // 3. CAS 抢占式领取任务 (防止并发重复执行)
        let claimed = self.storage.claim_next_run(
            &job.id,
            job.next_run_at_ms,
            next_run_at_ms,
            now
        ).await?;
        
        if !claimed { continue; }
        
        // 4. 执行任务
        self.execute_job_run(&job, job.next_run_at_ms).await?;
        executed += 1;
    }
    
    Ok(executed)
}
```

### 关键特性

| 特性 | 说明 |
|------|------|
| **轮询模式** | 默认每 1 秒轮询一次 (`poll_interval`) |
| **批量处理** | 支持配置 `batch_limit` (默认 64) |
| **CAS 抢占** | 使用乐观锁机制，多个 worker 实例不会重复执行同一任务 |
| **手动触发** | `run_job_now(cron_id)` 不修改下次调度时间 |

### CronWorkerConfig

```rust
pub struct CronWorkerConfig {
    pub poll_interval: Duration,  // 默认 1 秒
    pub batch_limit: i64,         // 默认 64
}
```

## Telegram 出站分发

### 背景出站分发

Cron 任务执行后，如果目标会话是 Telegram 渠道，会自动通过 Telegram API 发送响应：

```rust
async fn dispatch_telegram_outbound_message(
    msg: &Envelope<OutboundMessage>,
    config: &BackgroundServiceConfig,
) -> Result<(), String> {
    // 1. 从 session_key 推断 account_id
    let Some(account_id) = infer_account_id(&msg.header.session_key, "telegram") else {
        return Ok(());
    };
    
    // 2. 获取 Telegram 配置
    let Some(telegram_config) = config.telegram_configs.get(account_id) else {
        return Ok(());
    };
    
    // 3. 调用 Telegram 出站分发 (带超时)
    timeout(
        OUTBOUND_DISPATCH_TIMEOUT,
        dispatch_telegram_background_outbound(telegram_config, &msg.payload),
    ).await?
}
```

### 会话继承机制

Cron 任务可以配置 `cron.base_session_key` 元数据，用于 Telegram/DingTalk 会话继承：

```rust
async fn resolve_active_session_key(&self, payload: &InboundMessage) 
    -> Result<Option<String>, CronError> 
{
    let Some(base_session_key) = infer_base_session_key(payload) else {
        return Ok(None);
    };
    
    // 查询会话的 active_session_key (用于子会话继承)
    match self.storage.get_session(&base_session_key).await {
        Ok(session) => Ok(session.active_session_key.filter(|v| !v.trim().is_empty())),
        Err(_) => Ok(None),
    }
}
```

**应用场景**：当用户在 Telegram 中发起对话后创建了一个子会话，cron 任务可以通过元数据找到这个子会话，使定时任务在正确的上下文中执行。

## GUI Cron 面板

### 功能特性

| 功能 | 描述 |
|------|------|
| **任务列表** | 分页展示所有 cron 任务 |
| **添加任务** | 弹窗表单，支持设置 ID、名称、调度表达式、时区、Payload JSON |
| **编辑任务** | 修改现有任务配置 |
| **启用/禁用** | 一键切换任务状态 |
| **删除任务** | 带确认对话框 |
| **立即执行** | `Run Now` 手动触发任务 |
| **执行记录** | 查看任务的运行历史 |

### 表格列

| 列名 | 字段 | 说明 |
|------|------|------|
| ID | `id` | 任务 ID |
| Name | `name` | 任务名称 |
| Kind | `schedule_kind` | Cron / Every |
| Expression | `schedule_expr` | 调度表达式 |
| Enabled | `enabled` | 启用状态 |
| Next Run | `next_run_at_ms` | 下次执行时间 |
| Last Run | `last_run_at_ms` | 上次执行时间 |

### 右键菜单操作

- Runs - 查看执行记录
- Run Now - 立即执行
- Edit - 编辑
- Enable/Disable - 启用/禁用
- Delete - 删除
- Copy ID - 复制 ID

## cron_manager 工具

### 工具接口

`cron_manager` 工具允许 Agent 通过工具调用管理 cron 任务。

**支持的操作 (`action`)**：

| Action | 描述 | 必需参数 |
|--------|------|----------|
| `create` | 创建任务 | `name`, `schedule_expr`, `message` 或 `payload_json` |
| `update` | 更新任务 | `id`, 可选字段 |
| `delete` | 删除任务 | `id` |
| `get` | 获取单个任务 | `id` |
| `list` | 列出任务 | - |
| `list_due` | 列出到期任务 | - |
| `list_runs` | 列出执行记录 | `id` |
| `set_enabled` | 设置启用状态 | `id`, `enabled` |

### `message` 快捷方式

创建任务时，可以使用 `message` 字段简化配置：

```json
{
  "action": "create",
  "name": "daily weather",
  "schedule_expr": "8:00",
  "message": "查询今天的天气情况"
}
```

工具会自动：

- 从当前会话推断 `channel`, `chat_id`, `session_key`
- 设置 `cron.base_session_key` 元数据用于会话继承

### 工具调用示例

**创建每日提醒**：

```json
{
  "action": "create",
  "name": "morning briefing",
  "schedule_expr": "9:00",
  "message": "总结今天的待办事项"
}
```

**创建固定间隔任务**：

```json
{
  "action": "create",
  "name": "health check",
  "schedule_expr": "5m",
  "message": "检查系统状态"
}
```

**使用完整 payload**：

```json
{
  "action": "create",
  "name": "custom task",
  "schedule_expr": "0 8 * * *",
  "payload_json": "{\"channel\":\"stdio\",\"chat_id\":\"main\",\"session_key\":\"stdio:main\",\"content\":\"执行每日任务\",\"metadata\":{}}"
}
```

**列出所有任务**：

```json
{
  "action": "list"
}
```

**删除任务**：

```json
{
  "action": "delete",
  "id": "cron-123"
}
```

## 代码位置

| 模块 | 路径 | 职责 |
|------|------|------|
| klaw-cron | `klaw-cron/src/lib.rs` | 核心模块导出 |
| 任务管理器 | `klaw-cron/src/manager.rs` | SqliteCronManager |
| 调度执行器 | `klaw-cron/src/worker.rs` | CronWorker |
| 调度表达式解析 | `klaw-cron/src/schedule.rs` | ScheduleSpec |
| 存储类型定义 | `klaw-storage/src/types.rs` (行 427-515) | CronJob、CronTaskRun |
| 存储接口 | `klaw-storage/src/traits.rs` (行 172-219) | CronStorage trait |
| Telegram Channel | `klaw-channel/src/telegram/mod.rs` | 出站分发 |
| GUI Cron 面板 | `klaw-gui/src/panels/cron.rs` | CronPanel |
| cron_manager 工具 | `klaw-tool/src/cron_manager.rs` | 工具实现 |

## 相关文档

- [Heartbeat 调度器](../tools/advanced/heartbeat.md) - Session-bound 心跳任务
- [Session 存储](./session.md) - 会话状态管理
- [Telegram 渠道](../channels/telegram.md) - Telegram 集成
