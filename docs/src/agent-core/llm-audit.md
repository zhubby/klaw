# LLM Audit Trail 审计跟踪

本文档记录 `klaw-llm` 和 `klaw-storage` 中 LLM 审计跟踪功能的设计目标、数据模型、持久化机制以及 GUI 查看面板的实现细节。

## 目标

LLM Audit Trail 提供完整的 LLM 请求/响应审计跟踪能力：

- **调试**：查看完整的请求/响应内容，定位问题
- **审计**：记录所有 LLM 调用，满足合规要求
- **性能分析**：分析响应时间、token 使用等指标
- **错误排查**：记录失败请求的详细信息

审计数据由 LLM Provider 层自动生成，通过后台线程异步持久化，不影响主流程性能。

> 说明: `llm_audit` 继续作为请求/响应明细审计来源; provider/model 聚合分析、token 结构、tool success rate 和 turn 效率图表已经迁移到 `klaw-observability` 的本地 analysis store，并由 GUI `Analyze Dashboard` 的 `Models` 视图消费。

## 代码位置

| 模块 | 路径 | 职责 |
|------|------|------|
| 数据模型 | `klaw-storage/src/types.rs` | LlmAuditRecord、LlmAuditQuery |
| 存储接口 | `klaw-storage/src/traits.rs` | SessionStorage 审计方法 |
| 表结构 | `klaw-storage/src/backend/turso.rs` | llm_audit 表 |
| Payload 生成 | `klaw-llm/src/lib.rs` | LlmAuditPayload |
| Provider 实现 | `klaw-llm/src/providers/openai_compatible.rs` | OpenAI Provider |
| GUI 面板 | `klaw-gui/src/panels/llm.rs` | LlmPanel |
| JSON 组件 | `klaw-gui/src/widgets/json_tree.rs` | JSON 树形展示 |
| 后台写入 | `klaw-cli/src/runtime/mod.rs` | klaw-llm-audit-writer 线程 |

## 数据模型

### LlmAuditRecord

审计记录完整结构：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAuditRecord {
    pub id: String,                       // 审计记录唯一 ID
    pub session_key: String,              // 会话标识
    pub chat_id: String,                  // 聊天 ID
    pub turn_index: i64,                  // 对话轮次索引
    pub request_seq: i64,                 // 请求序号
    pub provider: String,                 // LLM 提供商名称
    pub model: String,                    // 模型名称
    pub wire_api: String,                 // 底层 API 类型
    pub status: LlmAuditStatus,           // 状态: Success | Failed
    pub error_code: Option<String>,       // 错误码
    pub error_message: Option<String>,    // 错误消息
    pub provider_request_id: Option<String>,   // 提供商请求 ID
    pub provider_response_id: Option<String>,  // 提供商响应 ID
    pub request_body_json: String,        // 请求体 JSON
    pub response_body_json: Option<String>,    // 响应体 JSON
    pub requested_at_ms: i64,             // 请求时间戳
    pub responded_at_ms: Option<i64>,     // 响应时间戳
    pub created_at_ms: i64,               // 记录创建时间
}
```

### LlmAuditStatus

```rust
pub enum LlmAuditStatus {
    Success,
    Failed,
}
```

### LlmAuditQuery

查询参数：

```rust
pub struct LlmAuditQuery {
    pub session_key: Option<String>,      // 按会话过滤
    pub provider: Option<String>,         // 按提供商过滤
    pub requested_from_ms: Option<i64>,   // 时间范围起点
    pub requested_to_ms: Option<i64>,     // 时间范围终点
    pub limit: i64,                       // 分页限制
    pub offset: i64,                      // 分页偏移
    pub sort_order: LlmAuditSortOrder,   // 排序方向
}

pub enum LlmAuditSortOrder {
    RequestedAtAsc,   // 时间升序
    RequestedAtDesc,  // 时间降序 (默认)
}
```

## 存储结构

### llm_audit 表

```sql
CREATE TABLE llm_audit (
    id TEXT PRIMARY KEY,
    session_key TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    turn_index INTEGER NOT NULL,
    request_seq INTEGER NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    wire_api TEXT NOT NULL,
    status TEXT NOT NULL,
    error_code TEXT,
    error_message TEXT,
    provider_request_id TEXT,
    provider_response_id TEXT,
    request_body_json TEXT NOT NULL,
    response_body_json TEXT,
    requested_at_ms INTEGER NOT NULL,
    responded_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    FOREIGN KEY (session_key) REFERENCES sessions(session_key) ON DELETE CASCADE
)
```

### 索引设计

```sql
CREATE INDEX idx_llm_audit_session_requested 
    ON llm_audit(session_key, requested_at_ms DESC);

CREATE INDEX idx_llm_audit_provider_requested 
    ON llm_audit(provider, requested_at_ms DESC);

CREATE INDEX idx_llm_audit_requested 
    ON llm_audit(requested_at_ms DESC);

CREATE INDEX idx_llm_audit_session_turn 
    ON llm_audit(session_key, turn_index, request_seq);
```

**索引用途**：

- `idx_llm_audit_session_requested`: 按会话查询历史记录
- `idx_llm_audit_provider_requested`: 按提供商统计和分析
- `idx_llm_audit_requested`: 全局时间范围查询
- `idx_llm_audit_session_turn`: 按会话轮次定位

## 持久化机制

### 数据生成 (Provider 层)

审计数据在 LLM Provider 调用时自动生成。每个 Provider 实现 `build_audit()` 方法：

```rust
pub struct LlmAuditPayload {
    pub provider: String,
    pub model: String,
    pub wire_api: String,
    pub status: LlmAuditStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub request_body: serde_json::Value,      // 完整请求体
    pub response_body: Option<serde_json::Value>, // 完整响应体
    pub requested_at_ms: i64,
    pub responded_at_ms: Option<i64>,
}
```

### 生成时机

**成功请求**：

```rust
// openai_compatible.rs
response.audit = Some(
    self.build_audit(
        model.unwrap_or(&self.config.default_model),
        LlmAuditStatus::Success,
        request_json,
        Some(payload_json),
        requested_at_ms,
        Some(now_ms()),
        None,
        None,
        response.usage.as_ref().and_then(|u| u.provider_request_id.clone()),
        response.usage.as_ref().and_then(|u| u.provider_response_id.clone()),
    ),
);
```

**失败请求**：

```rust
// HTTP 错误、解析失败、流错误时
let audit = self.build_audit(
    model,
    LlmAuditStatus::Failed,
    request_json,
    None,
    requested_at_ms,
    None,
    Some(error_code),
    Some(error_message),
    None,
    None,
);
```

### 传递链路

```
┌─────────────────────────────────────────────────────┐
│  klaw-llm (LlmResponse.audit)                       │
│  - Provider 自动生成 LlmAuditPayload                │
└───────────────────┬─────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────────┐
│  klaw-core (ProcessOutcome.llm_audits)              │
│  - 收集本轮所有 LLM 调用的审计数据                  │
└───────────────────┬─────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────────┐
│  klaw-cli (enqueue_llm_audit_records_from_outcome)  │
│  - 转换为 NewLlmAuditRecord                         │
│  - 发送到 sync_channel                              │
└───────────────────┬─────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────────┐
│  klaw-llm-audit-writer (后台线程)                   │
│  - 逐条调用 append_llm_audit                        │
│  - 错误记录到日志                                   │
└─────────────────────────────────────────────────────┘
```

### 后台写入线程

```rust
fn spawn_llm_audit_writer(session_store: DefaultSessionStore) 
    -> std::sync::mpsc::SyncSender<NewLlmAuditRecord> 
{
    let (tx, rx) = std::sync::mpsc::sync_channel::<NewLlmAuditRecord>(
        LLM_AUDIT_QUEUE_CAPACITY
    );
    
    std::thread::Builder::new()
        .name("klaw-llm-audit-writer".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("...");
            
            let manager = SqliteSessionManager::from_store(session_store);
            
            for record in rx {
                if let Err(err) = runtime.block_on(manager.append_llm_audit(&record)) {
                    warn!(
                        error = %err, 
                        audit_id = record.id.as_str(), 
                        "failed to persist llm audit record"
                    );
                }
            }
        })
        .expect("llm audit writer should start");
    
    tx
}
```

**关键特性**：

- **同步队列**：使用 `sync_channel` 避免主线程阻塞
- **独立线程**：后台写入不影响主流程性能
- **错误处理**：写入失败记录日志，不影响应用运行

## GUI LLM 面板

### 功能特性

| 功能 | 描述 |
|------|------|
| **记录列表** | 表格形式显示审计记录，显示时间、Session、Provider、Model、Status 等 |
| **过滤条件** | Session Key、Provider、日期范围 |
| **排序功能** | 按请求时间升序/降序切换 |
| **分页控制** | 可调整 limit 和 offset |
| **详情查看** | Request/Response 标签页切换，JSON 树形展示 |
| **快捷操作** | 复制 Session Key、复制 Request ID |

### 面板结构

```rust
pub struct LlmPanel {
    loaded: bool,
    rows: Vec<LlmAuditRecord>,
    
    // 过滤条件
    session_filter: String,
    provider_filter: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    
    // 分页
    limit_text: String,
    offset_text: String,
    
    // 排序
    sort_order: LlmAuditSortOrder,
    
    // 选择状态
    selected_id: Option<String>,
    
    // 详情窗口
    detail_record: Option<LlmAuditRecord>,
    detail_tab: DetailTab,  // Request | Response
}
```

### 表格列

| 列名 | 字段 | 说明 |
|------|------|------|
| Time | `requested_at_ms` | 格式化为本地时间 |
| Session | `session_key` | 会话标识（可截断） |
| Provider | `provider` | 提供商名称 |
| Model | `model` | 模型名称 |
| Wire API | `wire_api` | 底层 API 类型 |
| Turn | `turn_index` | 对话轮次 |
| Seq | `request_seq` | 请求序号 |
| Status | `status` | Success/Failed（颜色标识） |
| Req ID | `provider_request_id` | 提供商请求 ID |
| Resp ID | `provider_response_id` | 提供商响应 ID |

### 右键菜单

- View Details - 打开详情窗口
- Copy Session Key
- Copy Request ID

## JSON Viewer Widget

### 功能特性

- **递归树形展示**：对象显示为 `{}` 并标注字段数，数组显示为 `[]` 并标注元素数
- **可折叠**：使用 `egui::CollapsingHeader` 实现
- **路径标识**：每个节点有唯一路径 ID，确保折叠状态稳定
- **类型识别**：支持 Object、Array、String、Number、Bool、Null

### 实现原理

```rust
pub fn show_json_tree(ui: &mut egui::Ui, value: &serde_json::Value) {
    show_json_value(ui, "root", value, "$");
}

fn show_json_value(ui: &mut egui::Ui, label: &str, value: &serde_json::Value, path: &str) {
    match value {
        serde_json::Value::Object(map) => {
            let header = format!("{} {{}} ({})", label, map.len());
            egui::CollapsingHeader::new(header)
                .id_salt(path)
                .default_open(false)
                .show(ui, |ui| {
                    for (key, child) in map {
                        show_json_value(ui, key, child, &format!("{}.{}", path, key));
                    }
                });
        }
        serde_json::Value::Array(items) => {
            let header = format!("{} [] ({})", label, items.len());
            egui::CollapsingHeader::new(header)
                .id_salt(path)
                .default_open(false)
                .show(ui, |ui| {
                    for (index, child) in items.iter().enumerate() {
                        show_json_value(ui, &format!("[{}]", index), child, &format!("{}[{}]", path, index));
                    }
                });
        }
        serde_json::Value::String(s) => {
            ui.label(format!("{}: \"{}\"", label, s));
        }
        serde_json::Value::Number(n) => {
            ui.label(format!("{}: {}", label, n));
        }
        serde_json::Value::Bool(b) => {
            ui.label(format!("{}: {}", label, b));
        }
        serde_json::Value::Null => {
            ui.label(format!("{}: null", label));
        }
    }
}
```

## 查询能力

### 存储接口

```rust
#[async_trait]
pub trait SessionStorage: Send + Sync {
    async fn append_llm_audit(
        &self, 
        input: &NewLlmAuditRecord
    ) -> Result<LlmAuditRecord, StorageError>;
    
    async fn list_llm_audit(
        &self, 
        query: &LlmAuditQuery
    ) -> Result<Vec<LlmAuditRecord>, StorageError>;
}
```

### SQL 查询实现

```rust
async fn list_llm_audit(&self, query: &LlmAuditQuery) 
    -> Result<Vec<LlmAuditRecord>, StorageError> 
{
    // 排序方向
    let sort_order = match query.sort_order {
        LlmAuditSortOrder::RequestedAtAsc => "requested_at_ms ASC, created_at_ms ASC",
        LlmAuditSortOrder::RequestedAtDesc => "requested_at_ms DESC, created_at_ms DESC",
    };
    
    // 构建条件
    let mut conditions = Vec::new();
    
    if let Some(session_key) = query.session_key.as_deref().filter(|v| !v.is_empty()) {
        conditions.push(format!("session_key = '{}'", escape_sql_text(session_key)));
    }
    
    if let Some(provider) = query.provider.as_deref().filter(|v| !v.is_empty()) {
        conditions.push(format!("provider = '{}'", escape_sql_text(provider)));
    }
    
    if let Some(from_ms) = query.requested_from_ms {
        conditions.push(format!("requested_at_ms >= {}", from_ms));
    }
    
    if let Some(to_ms) = query.requested_to_ms {
        conditions.push(format!("requested_at_ms <= {}", to_ms));
    }
    
    // 构建完整 SQL
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    
    let sql = format!(
        "SELECT * FROM llm_audit {} ORDER BY {} LIMIT {} OFFSET {}",
        where_clause,
        sort_order,
        query.limit,
        query.offset
    );
    
    // 执行查询...
}
```

### 查询参数说明

| 参数 | 类型 | 说明 | 示例 |
|------|------|------|------|
| `session_key` | `Option<String>` | 按会话过滤 | `Some("stdio:main")` |
| `provider` | `Option<String>` | 按提供商过滤 | `Some("openai")` |
| `requested_from_ms` | `Option<i64>` | 时间范围起点（毫秒时间戳） | `Some(1700000000000)` |
| `requested_to_ms` | `Option<i64>` | 时间范围终点（毫秒时间戳） | `Some(1700086400000)` |
| `limit` | `i64` | 分页限制 | `50` |
| `offset` | `i64` | 分页偏移 | `0` |
| `sort_order` | `LlmAuditSortOrder` | 排序方向 | `RequestedAtDesc` |

## 使用场景

### 场景 1: 调试 LLM 请求

当 LLM 响应不符合预期时：

1. 打开 LLM 面板
2. 按 Session 过滤
3. 找到对应时间的请求
4. 查看 Request 确认 prompt 正确
5. 查看 Response 确认返回内容

### 场景 2: 审计与合规

记录所有 LLM 调用：

- 完整的请求/响应内容
- 时间戳精确到毫秒
- 提供商和模型信息
- 成功/失败状态

### 场景 3: 性能分析

分析响应时间：

- `requested_at_ms` 到 `responded_at_ms` 的差值
- 按提供商分组统计
- 识别慢请求

### 场景 4: 错误排查

失败请求记录：

- `status = Failed`
- `error_code` 和 `error_message` 详细信息
- 完整的请求体便于复现

## 配置说明

LLM 审计功能默认启用，无需额外配置。数据自动持久化到本地 SQLite 数据库。

如果需要禁用，可以在 Provider 配置中设置（当前版本暂不支持禁用）。

## 相关工具

### GUI 查看面板

在 GUI 中打开 **LLM** 面板即可查看所有审计记录。

### RuntimeBridge API

```rust
use klaw_gui::RuntimeBridge;

// 查询审计记录
let query = LlmAuditQuery {
    session_key: Some("stdio:main".to_string()),
    provider: None,
    requested_from_ms: None,
    requested_to_ms: None,
    limit: 50,
    offset: 0,
    sort_order: LlmAuditSortOrder::RequestedAtDesc,
};

let records = runtime_bridge.list_llm_audit(query).await?;
```

## 架构图

```
┌─────────────────────────────────────────────────────────────┐
│                      klaw-llm                               │
│  ┌─────────────────┐  ┌─────────────────┐                  │
│  │ OpenAI Provider │  │ Anthropic       │                  │
│  │ - chat()        │  │ Provider        │                  │
│  │ - chat_stream() │  │ - chat()        │                  │
│  └────────┬────────┘  └────────┬────────┘                  │
│           │ build_audit()      │                            │
│           └──────────┬──────────┘                            │
│                      ▼                                      │
│          LlmResponse.audit: Option<LlmAuditPayload>         │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                      klaw-core                              │
│  ProcessOutcome.llm_audits: Vec<LlmAuditPayload>           │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                      klaw-cli                               │
│  enqueue_llm_audit_records_from_outcome()                   │
│                      │                                      │
│                      ▼                                      │
│  ┌───────────────────────────────────────────┐              │
│  │  klaw-llm-audit-writer (后台线程)          │              │
│  │  - sync_channel 接收记录                  │              │
│  │  - 逐条调用 append_llm_audit 持久化       │              │
│  └───────────────────────────────────────────┘              │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                    klaw-storage                             │
│  SessionStorage::append_llm_audit()                         │
│  SessionStorage::list_llm_audit()                           │
│                      │                                      │
│                      ▼                                      │
│  ┌───────────────────────────────────────────┐              │
│  │         SQLite (llm_audit 表)             │              │
│  │  - 支持按 session/provider/时间过滤       │              │
│  │  - 支持分页和排序                         │              │
│  └───────────────────────────────────────────┘              │
└──────────────────────┬──────────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                      klaw-gui                               │
│  LlmPanel                                                    │
│  - 表格展示审计记录                                         │
│  - 过滤/排序/分页                                           │
│  - JSON Tree Widget 查看请求/响应详情                       │
└─────────────────────────────────────────────────────────────┘
```

## 测试覆盖

LLM 审计功能已覆盖以下测试场景：

- 成功请求生成审计记录
- 失败请求生成审计记录
- 审计记录正确持久化
- 按会话过滤查询
- 按提供商过滤查询
- 时间范围过滤
- 分页和排序

## 相关文档

- [可观测性审计](./observability-audit.md) - 整体可观测性架构
- [Session 存储](../storage/session.md) - 会话状态管理
- [配置概述](../configration/overview.md) - 配置模型
