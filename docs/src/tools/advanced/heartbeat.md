# Heartbeat 调度器设计与实现

本文档记录 `klaw-heartbeat` 模块的设计目标、数据模型、调度机制、持久化策略以及 GUI 管理面板的实现细节。

## 目标

Heartbeat Scheduler 实现了 **session-bound autonomous heartbeat** 语义，这是一种按固定周期向 Agent Session 注入特殊消息的机制，让 Agent 自主检查当前上下文并决定是否需要继续行动。

与传统的心跳机制不同：

| 类型 | 语义 | 目的 |
|------|------|------|
| WebSocket/HTTP ping/pong | 传输层保活 | 检测连接是否存活 |
| Agent Heartbeat | 业务层自主驱动 | 让 Agent 在空闲时主动检查和行动 |

Heartbeat 本质上是一种**周期性 Agent Turn**，它复用现有消息处理链路、会话串行调度和 Session 存储能力，但作为独立领域模块存在，不污染通用 Cron 模块。

## 代码位置

| 模块 | 路径 | 职责 |
|------|------|------|
| klaw-heartbeat | `klaw-heartbeat/src/lib.rs` | 核心 HeartbeatWorker、HeartbeatManager |
| 存储模型 | `klaw-storage/src/types.rs` (行 518-532) | HeartbeatJob、HeartbeatTaskRun |
| 存储接口 | `klaw-storage/src/traits.rs` (行 222-295) | HeartbeatStorage trait |
| GUI 面板 | `klaw-gui/src/panels/heartbeat.rs` | HeartbeatPanel |
| 工具接口 | `klaw-tool/src/heartbeat_manager.rs` | heartbeat_manager 工具 |
| 运行时集成 | `klaw-cli/src/runtime/service_loop.rs` | BackgroundServices |

## 数据模型

### HeartbeatJob

心跳任务定义：

```rust
pub struct HeartbeatJob {
    pub id: String,                    // 唯一标识符
    pub session_key: String,           // 绑定的会话 key (UNIQUE)
    pub channel: String,               // 渠道 (如 "stdio", "dingtalk")
    pub chat_id: String,               // 聊天 ID
    pub enabled: bool,                 // 是否启用
    pub every: String,                 // 执行间隔 (如 "10m", "1h", "24h")
    pub prompt: String,                // 每次执行的提示词
    pub silent_ack_token: String,      // 静默确认令牌
    pub timezone: String,              // 时区 (默认 "UTC")
    pub next_run_at_ms: i64,           // 下次执行时间戳 (毫秒)
    pub last_run_at_ms: Option<i64>,   // 上次执行时间戳
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
```

**关键约束**：

- `session_key` 字段有 `UNIQUE` 约束，确保每个 Session 最多只有一个心跳任务
- `every` 字段支持 humantime 格式：`30s`、`5m`、`1h`、`24h` 等

### HeartbeatTaskRun

执行记录：

```rust
pub struct HeartbeatTaskRun {
    pub id: String,
    pub heartbeat_id: String,
    pub scheduled_at_ms: i64,          // 计划执行时间
    pub started_at_ms: Option<i64>,    // 实际开始时间
    pub finished_at_ms: Option<i64>,   // 完成时间
    pub status: HeartbeatTaskStatus,   // Pending/Running/Success/Failed
    pub attempt: i64,                  // 重试次数
    pub error_message: Option<String>,
    pub published_message_id: Option<String>,
    pub created_at_ms: i64,
}
```

## 调度机制

### Worker 核心逻辑

`HeartbeatWorker` 使用轮询模式调度：

```rust
pub async fn run_tick(&self) -> Result<usize, HeartbeatError> {
    let now = now_ms();
    
    // 1. 查询到期的心跳任务
    let due_jobs = self.storage
        .list_due_heartbeats(now, self.config.batch_limit)
        .await?;
    
    let mut executed = 0usize;
    
    for job in due_jobs {
        // 2. 计算下次执行时间
        let next_run_at_ms = next_run_after(&job.every, job.next_run_at_ms)?;
        
        // 3. 原子性抢占任务 (CAS)
        let claimed = self.storage.claim_next_heartbeat_run(
            &job.id,
            job.next_run_at_ms,
            next_run_at_ms,
            now
        ).await?;
        
        if !claimed { continue; }
        
        // 4. 执行心跳任务
        if self.execute_job_run(&job, job.next_run_at_ms).await.is_ok() {
            executed += 1;
        }
    }
    
    Ok(executed)
}
```

### 关键特性

| 特性 | 说明 |
|------|------|
| **轮询模式** | 默认每 1 秒轮询一次 (`poll_interval`) |
| **批量处理** | 每次最多处理 64 个到期任务 (`batch_limit`) |
| **乐观锁抢占** | 使用 `claim_next_heartbeat_run` 原子性更新 `next_run_at_ms`，防止多实例竞争 |
| **humantime 解析** | 支持 `10m`、`1h`、`24h` 等人类可读的时间间隔格式 |

### HeartbeatWorkerConfig

```rust
pub struct HeartbeatWorkerConfig {
    pub poll_interval: Duration,  // 默认 1 秒
    pub batch_limit: i64,         // 默认 64
}
```

## Session 绑定与路由

### 一对一绑定

心跳任务通过 `session_key` 字段与 Session 绑定：

- 数据库中 `session_key` 字段有 `UNIQUE` 约束
- 每个会话最多只有一个心跳任务

### Active Session 解析

当心跳触发时，会解析活跃子会话：

```rust
async fn resolve_active_session_key(&self, session_key: &str) 
    -> Result<Option<String>, HeartbeatError> 
{
    match self.storage.get_session(session_key).await {
        Ok(session) => Ok(session
            .active_session_key
            .filter(|value| !value.trim().is_empty())),
        Err(_) => Ok(None),
    }
}
```

**工作流程**：

1. 心跳触发时，查找 `session_key` 对应的 Session
2. 如果 Session 有 `active_session_key`（活跃子会话），则路由到该子会话
3. 否则使用原始 `session_key`

这支持了**会话委托**场景：父会话的心跳可以自动路由到当前活跃的子会话。

### 消息元数据标记

心跳消息包含特殊元数据：

```rust
pub fn build_inbound_message(job: &HeartbeatJob) -> InboundMessage {
    InboundMessage {
        channel: job.channel.clone(),
        sender_id: "system-heartbeat".to_string(),
        chat_id: job.chat_id.clone(),
        session_key: job.session_key.clone(),
        content: job.prompt.clone(),
        metadata: BTreeMap::from([
            (TRIGGER_KIND_KEY.to_string(), Value::String(TRIGGER_KIND_HEARTBEAT.to_string())),
            (HEARTBEAT_SESSION_KEY.to_string(), Value::String(job.session_key.clone())),
            (HEARTBEAT_SILENT_ACK_TOKEN_KEY.to_string(), Value::String(job.silent_ack_token.clone())),
        ]),
        media_references: Vec::new(),
    }
}
```

**元数据常量**：

```rust
pub const TRIGGER_KIND_KEY: &str = "trigger.kind";
pub const TRIGGER_KIND_HEARTBEAT: &str = "heartbeat";
pub const HEARTBEAT_SESSION_KEY: &str = "heartbeat.session_key";
pub const HEARTBEAT_SILENT_ACK_TOKEN_KEY: &str = "heartbeat.silent_ack_token";
```

## 持久化

### 数据库表结构

**heartbeat 表**：

```sql
CREATE TABLE IF NOT EXISTS heartbeat (
    id TEXT PRIMARY KEY,
    session_key TEXT NOT NULL UNIQUE,
    channel TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    every TEXT NOT NULL,
    prompt TEXT NOT NULL,
    silent_ack_token TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    next_run_at_ms INTEGER NOT NULL,
    last_run_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
)
```

**heartbeat_task 表**：

```sql
CREATE TABLE IF NOT EXISTS heartbeat_task (
    id TEXT PRIMARY KEY,
    heartbeat_id TEXT NOT NULL,
    scheduled_at_ms INTEGER NOT NULL,
    started_at_ms INTEGER,
    finished_at_ms INTEGER,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    published_message_id TEXT,
    created_at_ms INTEGER NOT NULL,
    FOREIGN KEY (heartbeat_id) REFERENCES heartbeat(id) ON DELETE CASCADE
)
```

### HeartbeatStorage Trait

```rust
#[async_trait]
pub trait HeartbeatStorage: Send + Sync {
    // CRUD 操作
    async fn create_heartbeat(&self, input: &NewHeartbeatJob) -> Result<HeartbeatJob, StorageError>;
    async fn update_heartbeat(&self, id: &str, patch: &UpdateHeartbeatJobPatch) -> Result<HeartbeatJob, StorageError>;
    async fn delete_heartbeat(&self, id: &str) -> Result<(), StorageError>;
    async fn get_heartbeat(&self, id: &str) -> Result<HeartbeatJob, StorageError>;
    async fn get_heartbeat_by_session_key(&self, session_key: &str) -> Result<HeartbeatJob, StorageError>;
    async fn list_heartbeats(&self, limit: i64, offset: i64) -> Result<Vec<HeartbeatJob>, StorageError>;
    
    // 调度相关
    async fn list_due_heartbeats(&self, now_ms: i64, limit: i64) -> Result<Vec<HeartbeatJob>, StorageError>;
    async fn claim_next_heartbeat_run(
        &self,
        heartbeat_id: &str,
        expected_next_run_at_ms: i64,
        new_next_run_at_ms: i64,
        now_ms: i64
    ) -> Result<bool, StorageError>;
    
    // 执行记录
    async fn append_heartbeat_task_run(&self, input: &NewHeartbeatTaskRun) -> Result<HeartbeatTaskRun, StorageError>;
    async fn mark_heartbeat_task_running(&self, run_id: &str, started_at_ms: i64) -> Result<(), StorageError>;
    async fn mark_heartbeat_task_result(
        &self,
        run_id: &str,
        status: &str,
        finished_at_ms: i64,
        error_message: Option<&str>
    ) -> Result<(), StorageError>;
    async fn list_heartbeat_task_runs(&self, heartbeat_id: &str, limit: i64, offset: i64) 
        -> Result<Vec<HeartbeatTaskRun>, StorageError>;
}
```

## GUI Heartbeat 面板

### 功能特性

| 功能 | 描述 |
|------|------|
| **任务列表** | 显示所有心跳任务，包含 ID、Session、Channel、Enabled、Every、Next Run、Last Run、Updated At |
| **添加任务** | 弹出表单窗口，选择 Session 并配置参数 |
| **编辑任务** | 修改现有任务配置 |
| **启用/禁用** | 快速切换任务状态 |
| **删除任务** | 带确认对话框的删除操作 |
| **查看执行记录** | 弹出窗口显示任务的执行历史 |
| **立即执行** | 手动触发一次心跳执行 |
| **静默确认令牌** | 用于识别"无需操作"的响应 |

### 表单字段

```rust
struct HeartbeatForm {
    id: String,              // 任务 ID (创建时可编辑)
    session_key: String,     // 目标会话 (下拉选择)
    channel: String,         // 渠道 (自动填充，只读)
    chat_id: String,         // 聊天 ID (自动填充，只读)
    enabled: bool,           // 启用状态
    every: String,           // 执行间隔 (默认 "30m")
    prompt: String,          // 提示词 (默认包含 HEARTBEAT_OK 说明)
    silent_ack_token: String,// 静默令牌 (默认 "HEARTBEAT_OK")
    timezone: String,        // 时区 (默认 "UTC")
}
```

### 右键上下文菜单

- 查看执行记录
- 立即执行
- 编辑
- 启用/禁用
- 删除
- 复制 ID

### 默认提示词

```text
Review the session state. If no user-visible action is needed, reply with exactly HEARTBEAT_OK and nothing else.
```

## heartbeat_manager 工具

### 工具描述

```rust
fn description(&self) -> &str {
    "Get or update the heartbeat bound to the current conversation. Use `get` to inspect the current session heartbeat, or `update` to change only the custom prompt that is prepended before the fixed heartbeat instruction."
}
```

### 支持的操作

| Action | 描述 | 必需参数 |
|--------|------|----------|
| `get` | 获取当前会话绑定的 heartbeat | - |
| `update` | 更新当前会话 heartbeat 的自定义 prompt | `prompt` |

### 工具调用示例

```json
{
  "action": "get"
}
```

```json
{
  "action": "update",
  "prompt": "Check unread mentions and recent follow-ups before deciding whether to stay silent."
}
```

```json
{
  "action": "update",
  "prompt": ""
}
```

## 配置

### HeartbeatManagerConfig

```toml
[tools.heartbeat_manager]
enabled = true  # 是否启用 heartbeat_manager 工具
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatManagerConfig {
    #[serde(default = "default_heartbeat_manager_enabled")]
    pub enabled: bool,
}

impl Default for HeartbeatManagerConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}
```

### HeartbeatWorkerConfig

```rust
pub struct HeartbeatWorkerConfig {
    pub poll_interval: Duration,  // 默认 1 秒
    pub batch_limit: i64,         // 默认 64
}
```

## 静默确认机制

当心跳任务执行后，如果 Agent 响应的内容完全匹配 `silent_ack_token`（去除空白后），系统会识别这是一个"无需用户关注"的响应，可以静默处理。

### 判定逻辑

```rust
pub fn should_suppress_output(content: &str, metadata: &BTreeMap<String, Value>) -> bool {
    // 1. 检查是否为心跳消息
    if !is_heartbeat_metadata(metadata) {
        return false;
    }
    
    // 2. 提取静默令牌
    let Some(token) = metadata
        .get(HEARTBEAT_SILENT_ACK_TOKEN_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };

    // 3. 比较响应内容
    content.trim() == token
}
```

### 静默处理行为

当响应匹配静默令牌时：

- 不发布 outbound
- 不写 assistant chat record
- 不增加 session turn 计数

### 使用场景

```text
Prompt: "Review the session state. If no action needed, reply HEARTBEAT_OK"

Agent Response: "HEARTBEAT_OK"

→ 系统识别为静默确认，不产生用户可见输出
```

```text
Prompt: "Check for pending tasks"

Agent Response: "You have 3 unread emails and a meeting in 30 minutes."

→ 正常输出，推送给用户
```

## 使用示例

### 通过 GUI 创建心跳任务

1. 打开 Heartbeat 面板
2. 点击 "Add" 按钮
3. 选择目标 Session
4. 配置执行间隔（如 `30m`）
5. 编写提示词
6. 点击 "Create"

### 通过工具调用创建

```json
{
  "action": "create",
  "session_key": "telegram:bot:chat-123",
  "every": "1h",
  "prompt": "Summarize recent activity and notify user if anything important happened.",
  "silent_ack_token": "ALL_GOOD"
}
```

### 立即执行心跳

通过 GUI 右键菜单选择 "Run Now"，或通过工具调用：

```rust
// 运行时 API
pub async fn run_heartbeat_now(&self, heartbeat_id: &str) -> Result<String, String> {
    self.heartbeat_worker
        .run_job_now(heartbeat_id)
        .await
        .map_err(|err| err.to_string())
}
```

## 运行时集成

### BackgroundServices 集成

```rust
pub struct BackgroundServices {
    cron_worker: StdioCronWorker,
    heartbeat_worker: StdioHeartbeatWorker,
    // ...
}

// 初始化
let heartbeat_worker = HeartbeatWorker::new(
    Arc::new(runtime.session_store.clone()),
    Arc::new(runtime.inbound_transport.clone()),
    HeartbeatWorkerConfig {
        poll_interval: Duration::from_secs(1),
        batch_limit: config.cron_batch_limit,
    },
);

// 定时触发
pub async fn on_cron_tick(&self) {
    if let Err(err) = self.heartbeat_worker.run_tick().await {
        warn!(error = %err, "heartbeat tick failed");
    }
}
```

## 测试覆盖

`klaw-heartbeat` 已覆盖以下测试场景：

- HeartbeatJob 正确映射
- 启动时缺失的心跳任务能被创建
- 配置更新后可正确同步
- 禁用后任务停止调度
- 触发时发布的 inbound 包含完整 metadata
- 输出为静默令牌时不产生 outbound
- 输出为静默令牌时不写 session chat record
- 与用户消息共用 session_key 时遵守串行调度
- 系统重启后通过持久化自动恢复

## 后续演进

- Quiet hours / 活跃时段配置
- 连续失败后的重试与告警
- 心跳运行统计与审计
- 独立 heartbeat 运行表（如需更复杂的调度策略）
- Transport keepalive 子系统（与 Agent Heartbeat 分离设计）

## 相关文档

- [Cron 存储](../../storage/cron.md) - 定时任务持久化
- [Session 存储](../../storage/session.md) - 会话状态管理
- [配置概述](../../configration/overview.md) - 配置模型