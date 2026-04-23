# Memory 存储

`klaw-memory` 提供长期记忆的 CRUD、混合检索（FTS + vector）、写入治理、prompt 渲染和统计聚合，是 Klaw 记忆系统的核心模块。

## 设计目标

- **事实替换而非日志累积**：长期记忆的写入目标是维护一组稳定、可追溯的事实，而不是简单堆叠每轮对话日志
- **混合检索最优排序**：同时利用 BM25 全文检索和向量语义检索，通过 RRF 融合取得稳定排名
- **治理驱动的写入**：长期记忆的 add 操作经过规范化、重复检测和冲突替换后才入库
- **受控的 prompt 注入**：长期记忆以受预算控制的章节形式注入 system prompt，避免 prompt 膨胀
- **后端可替换**：同一套 `MemoryService` trait 支持 turso（libSQL）和 sqlx 后端

## 模块结构

```text
klaw-memory/src/
├── lib.rs          # 对外导出、re-export
├── types.rs        # MemoryService trait、EmbeddingProvider trait、数据模型
├── service.rs      # SqliteMemoryService 实现
├── provider.rs     # OpenAiEmbeddingProvider、配置工厂
├── governance.rs   # 长期记忆写入治理规则
├── prompt.rs       # 长期记忆 prompt 渲染
├── stats.rs        # SqliteMemoryStatsService、统计聚合
├── error.rs        # MemoryError 错误枚举
├── util.rs         # RRF 融合、行解析、类型转换
└── tests.rs        # 集成测试
```

## 数据模型

### 数据库表

长期记忆保存在独立的 `memory.db`（路径 `~/.klaw/memory.db`），通过 `klaw-storage` 提供的 `DefaultMemoryDb` 打开。

#### memories（主表）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 记录 ID（UUID v4 或用户指定） |
| `scope` | TEXT NOT NULL | 作用域（`long_term`、`session:xxx` 等） |
| `content` | TEXT NOT NULL | 记忆内容 |
| `metadata_json` | TEXT NOT NULL | 结构化元数据 JSON（`kind`、`status`、`topic`、`supersedes` 等） |
| `pinned` | INTEGER NOT NULL DEFAULT 0 | 是否置顶 |
| `embedding` | BLOB | 向量嵌入（`f32` 数组的小端字节序 blob） |
| `created_at_ms` | INTEGER NOT NULL | 创建时间（毫秒 epoch） |
| `updated_at_ms` | INTEGER NOT NULL | 更新时间（毫秒 epoch） |

#### memories_fts（FTS5 全文检索虚拟表）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT UNINDEXED | 关联 `memories.id` |
| `content` | TEXT | 全文检索内容 |

#### 向量索引

```text
idx_memories_embedding — libsql_vector_idx(embedding)
```

需要 Turso/libSQL 后端 + embedding provider 配置才能启用。

### Rust 类型

#### MemoryRecord

```rust
pub struct MemoryRecord {
    pub id: String,
    pub scope: String,
    pub content: String,
    pub metadata: Value,       // serde_json::Value
    pub pinned: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
```

#### UpsertMemoryInput

```rust
pub struct UpsertMemoryInput {
    pub id: Option<String>,    // None 时自动生成 UUID v4
    pub scope: String,
    pub content: String,
    pub metadata: Value,
    pub pinned: bool,
}
```

#### MemorySearchQuery

```rust
pub struct MemorySearchQuery {
    pub scope: Option<String>,
    pub text: String,
    pub limit: usize,          // 最终返回上限（默认 8）
    pub fts_limit: usize,      // FTS 候选池大小（默认 20）
    pub vector_limit: usize,   // 向量候选池大小（默认 20）
    pub use_vector: bool,      // 是否启用向量检索（默认 true）
}
```

#### MemoryHit

```rust
pub struct MemoryHit {
    pub record: MemoryRecord,
    pub fused_score: f64,      // RRF 融合分数
    pub bm25_rank: Option<usize>,
    pub vector_rank: Option<usize>,
}
```

## 核心 trait

### MemoryService

```rust
#[async_trait]
pub trait MemoryService: Send + Sync {
    async fn upsert(&self, input: UpsertMemoryInput) -> Result<MemoryRecord, MemoryError>;
    async fn list_scope_records(&self, scope: &str) -> Result<Vec<MemoryRecord>, MemoryError>;
    async fn search(&self, query: MemorySearchQuery) -> Result<Vec<MemoryHit>, MemoryError>;
    async fn get(&self, id: &str) -> Result<Option<MemoryRecord>, MemoryError>;
    async fn delete(&self, id: &str) -> Result<bool, MemoryError>;
    async fn pin(&self, id: &str, pinned: bool) -> Result<Option<MemoryRecord>, MemoryError>;
}
```

设计考量：

- `upsert` 使用 `INSERT ... ON CONFLICT DO UPDATE` 实现，新记录自动生成 ID，已有记录按 ID 更新
- `search` 内部执行 FTS + vector 双通道检索，RRF 融合后再按 `pinned > fused_score > updated_at_ms` 排序
- `delete` 同步清理主表和 FTS 表中的记录
- `pin` 更新 `pinned` 标记并刷新 `updated_at_ms`

### EmbeddingProvider

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    fn model(&self) -> &str;
    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, MemoryError>;
}
```

设计考量：

- `embed_texts` 支持批量请求，减少 API 调用次数
- 返回的向量以 `Vec<f32>` 表示，入库时通过 `f32_vec_to_blob` 转为小端字节序 blob
- 空输入直接返回空数组，不做 API 调用

## 服务实现

### SqliteMemoryService

`SqliteMemoryService` 是 `MemoryService` 的默认实现，构造流程：

1. 打开 `memory.db`（通过 `open_default_memory_db` 或传入 `Arc<dyn MemoryDb>`）
2. 根据 `memory.embedding.enabled` 判断是否构建 embedding provider
3. 执行 `init_schema()`：创建 `memories` 主表
4. 尝试启用 FTS5（`try_enable_fts`）：创建 `memories_fts` 虚拟表
5. 尝试启用向量索引（`try_enable_vector_index`）：创建 `idx_memories_embedding`

```rust
pub async fn open_default(config: &AppConfig) -> Result<Self, MemoryError>
pub async fn new(
    db: Arc<dyn MemoryDb>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
) -> Result<Self, MemoryError>
```

初始化结果保存在 `fts_enabled` 和 `vector_enabled` 两个布尔字段中，检索时根据这两个标记选择通道。

### Upsert 流程

1. 验证 `scope` 和 `content` 非空
2. 生成 ID（若未提供则 `Uuid::new_v4()`）
3. 序列化 `metadata` 为 JSON
4. 可选：调用 `try_embed_one` 生成 embedding blob
5. 执行 `INSERT ... ON CONFLICT DO UPDATE` 写入主表
6. 若 FTS 启用：先删除旧 FTS 行，再插入新 FTS 行（确保内容变更时 FTS 索引同步更新）
7. 回读并返回完整 `MemoryRecord`

设计考量：

- FTS 行采用"删除旧 + 插入新"策略而非 `INSERT OR REPLACE`，避免 FTS5 内部状态残留
- embedding 在写入时即时计算，后续检索无需重复调用 API
- `ON CONFLICT DO UPDATE` 确保同一 ID 的记录只保留一份，metadata 和 embedding 均随内容更新

### Search 流程

检索采用多通道候选 → RRF 融合 → 排序裁剪的三阶段流程：

**第一阶段：FTS/BM25 候选**

- 若 `fts_enabled`：通过 `memories_fts MATCH ?1` 查询，使用 `bm25(memories_fts)` 排名
- 若 FTS 不可用：退回 `content LIKE ?1` 模式匹配
- 支持 `scope` 过滤
- 候选池大小由 `fts_limit` 控制（默认 20）

**第二阶段：Vector 候选**

- 若 `use_vector && vector_enabled`：将查询文本 embedding 后调用 `vector_top_k` 查询
- 候选池大小由 `vector_limit` 控制（默认 20）
- 向量候选中不在 FTS 结果集内的记录会单独回读并过滤 scope
- 若 embedding 计算失败或向量查询无结果，该通道静默跳过

**第三阶段：RRF 融合**

- 对每个候选记录，计算 RRF 分数：
  ```rust
  rrf_score(bm25_rank, vector_rank) = Σ 1/(RRF_K + rank)
  ```
  其中 `RRF_K = 60.0`（经验常数）
- 双通道命中的记录获得更高分数，单通道命中的记录分数较低
- 排序优先级：`pinned DESC > fused_score DESC > updated_at_ms DESC`
- 最终裁剪到 `limit` 条记录返回

设计考量：

- RRF 融合不需要原始 BM25/vector 分数，只需要排名序号，因此跨通道分数尺度不一致的问题被自然规避
- `RRF_K = 60.0` 是文献中常用的参数，在 top-k 融合场景下表现稳定
- FTS 不可用时退回 LIKE 模式而非静默返回空结果，保证基础检索能力始终可用

### Delete 流程

1. 从 `memories` 主表删除记录
2. 若 FTS 启用：从 `memories_fts` 同步删除对应行
3. 返回是否删除成功（affected rows > 0）

### Pin 流程

1. 更新 `memories.pinned` 和 `updated_at_ms`
2. 若记录不存在，返回 `None`

## Embedding Provider

### OpenAiEmbeddingProvider

基于 OpenAI-compatible embedding API 的默认 provider：

```rust
pub struct OpenAiEmbeddingProvider {
    provider_name: String,
    base_url: String,
    model: String,
    api_key: String,
    client: Client,            // reqwest::Client
}
```

- 构造时从 `model_providers` 配置中读取 `base_url` 和 `api_key`
- API 密钥优先使用 `api_key` 直接值，其次从 `env_key` 环境变量读取
- 发送 `POST {base_url}/embeddings` 请求，格式兼容 OpenAI API
- 批量文本一次性请求，返回对应数量的 embedding 向量

### 配置工厂

```rust
pub fn build_embedding_provider_from_config(
    config: &AppConfig,
) -> Result<Arc<dyn EmbeddingProvider>, MemoryError>
```

校验规则：

- `memory.embedding.provider` 非空，且存在于 `model_providers`
- `memory.embedding.model` 非空
- 对应 provider 必须有可用的 `api_key` 或 `env_key`

配置示例：

```toml
[memory.embedding]
enabled = true
provider = "openai"
model = "text-embedding-3-small"

[model_providers.openai]
base_url = "https://api.openai.com/v1"
wire_api = "responses"
default_model = "gpt-4o-mini"
env_key = "OPENAI_API_KEY"
```

## 长期记忆治理

长期记忆写入通过 `govern_long_term_write` 函数进行治理，该函数在 `klaw-memory/src/governance.rs` 中实现。

### 治理流程

```
输入 draft (UpsertMemoryInput, scope="long_term")
  │
  ├─ 1. 规范化 content：去除多余空白，合并为一个空格分隔的字符串
  ├─ 2. 规范化 kind：从 metadata.kind 解析，默认为 fact
  ├─ 3. 校验 status：新写入只接受 active，拒绝系统托管状态
  ├─ 4. 规范化 topic：可选字符串字段，用于冲突替换
  ├─ 5. 规范化 supersedes：字符串或字符串数组，去重后保留
  ├─ 6. 精确重复检测：同一 kind + 规范化 content 的 active 记录视为重复
  ├─ 7. 冲突替换：同一 kind + topic 的其他 active 记录视为被替代
  │
  └─ 输出 GovernedLongTermWrite
```

### Kind（记忆类型）

| Kind | 优先级 | 说明 |
|------|--------|------|
| `identity` | 5 | 身份信息 |
| `project_rule` | 4 | 项目规则 |
| `constraint` | 3 | 工作约束 |
| `preference` | 2 | 用户偏好 |
| `workflow` | 1 | 流程规范 |
| `fact` | 0 | 可复用事实（默认） |

优先级用于 prompt 渲染排序：高优先级 kind 在同等 pinned 状态下排在前面。

### Status（状态生命周期）

| Status | 说明 | 是否可由外部写入 |
|--------|------|-----------------|
| `active` | 当前生效 | ✅（唯一可写入状态） |
| `superseded` | 已被新记录替代 | ❌（系统自动管理） |
| `archived` | 已归档 | ❌（系统自动管理） |
| `rejected` | 已拒绝 | ❌（系统自动管理） |

新写入强制 `status = active`。`superseded / archived / rejected` 是系统托管状态，不能由模型或用户直接创建。

### 冲突替换逻辑

当新记录的 `kind + topic` 与现有 active 记录冲突时：

1. 旧记录自动标记为 `status = superseded`
2. 旧记录补上 `superseded_by = <new_id>`
3. 新记录补上 `supersedes = [old_id, ...]`
4. 被替代的旧记录不再出现在 prompt 渲染中

示例：

- 旧记录：`kind=preference, topic=reply_language, content="默认使用英文回复"`
- 新记录：`kind=preference, topic=reply_language, content="默认使用中文回复"`
- 结果：旧记录变为 `superseded`，新记录为 `active`，prompt 中只出现新记录

### 重复检测

若新写入内容的规范化形式与同一 `kind` 下现有 `active` 记录完全一致：

- 复用原记录 ID（不产生新的 UUID）
- 更新 metadata 和 pinned 标记
- 不创建重复记录

规范化规则：将 `content` 按 whitespace 拆分后用单个空格重新拼接（`split_whitespace().join(" ")`），实现大小写保留但空白差异归零的去重。

### GovernedLongTermWrite 输出

```rust
pub struct GovernedLongTermWrite {
    pub primary: UpsertMemoryInput,           // 主记录（含规范化后的 metadata）
    pub superseded_updates: Vec<UpsertMemoryInput>, // 需要标记为 superseded 的旧记录
    pub reused_existing_id: Option<String>,   // 若复用了已有记录 ID
    pub supersedes_ids: Vec<String>,          // 被替代的记录 ID 列表
    pub kind: LongTermMemoryKind,             // 解析后的 kind
}
```

调用方（通常是 `memory` tool）需将 `primary` 和 `superseded_updates` 都写入 `SqliteMemoryService`，确保治理结果完整持久化。

## Prompt 渲染

长期记忆在 runtime 每轮执行前通过 `render_long_term_memory_section` 渲染为一段受控文本，拼入 system prompt。

### LongTermMemoryPromptOptions

```rust
pub struct LongTermMemoryPromptOptions {
    pub max_items: usize,       // 最大条目数（默认 12）
    pub max_chars: usize,       // 总字符预算（默认 1800）
    pub max_item_chars: usize,  // 单条截断长度（默认 240）
}
```

### 渲染流程

1. 收集所有 `status = active` 的长期记忆记录
2. 排序：`pinned DESC > kind.priority DESC > updated_at_ms DESC > id ASC`
3. 跳过 `superseded / archived / rejected` 记录
4. 规范化 content 后做大小写不敏感去重（`to_ascii_lowercase()`）
5. 每条内容截断到 `max_item_chars`，超出部分加 `...`
6. 格式化：`- [kind] content` 或 `- content`（无 kind 时）
7. 累加字符数，达到 `max_chars` 预算时停止
8. 条目数达到 `max_items` 时停止
9. 输出为多行文本块

设计考量：

- budget 机制确保长期记忆不会无限膨胀 system prompt
- `max_item_chars` 防止单条过长记忆挤占整个预算
- 大小写不敏感去重避免同一事实因大小写差异被渲染两次
- `max_items` 和 `max_chars` 是双重约束，同时生效

## 检索统计

### SqliteMemoryStatsService

```rust
pub struct SqliteMemoryStatsService {
    db: Arc<dyn MemoryDb>,
}
```

提供 memory 数据库的统计聚合，主要服务 GUI 面板的数据需求。

### MemoryStats

```rust
pub struct MemoryStats {
    pub total_records: i64,           // 总记录数
    pub pinned_records: i64,          // 置顶记录数
    pub embedded_records: i64,        // 有 embedding 的记录数
    pub distinct_scopes: i64,         // 不同 scope 数
    pub created_min_ms: Option<i64>,  // 最早创建时间
    pub created_max_ms: Option<i64>,  // 最晚创建时间
    pub updated_max_ms: Option<i64>,  // 最近更新时间
    pub avg_content_len: Option<f64>, // 平均内容长度
    pub updated_last_24h: i64,        // 24小时内更新的记录数
    pub updated_last_7d: i64,         // 7天内更新的记录数
    pub fts_enabled: bool,            // FTS5 是否可用
    pub vector_index_enabled: bool,   // 向量索引是否可用
    pub top_scopes: Vec<ScopeStat>,   // 按 count DESC 排列的 scope 分布
}
```

### ScopeStat

```rust
pub struct ScopeStat {
    pub scope: String,
    pub count: i64,
}
```

### 关键方法

- `collect(scope_limit)`：一次性聚合所有统计数据，包括通过 `sqlite_master` 检测 FTS/vector 索引可用性
- `list_scope_records(scope)`：返回指定 scope 的完整记录明细（按 `pinned DESC, updated_at_ms DESC, created_at_ms DESC, id ASC` 排序）

设计考量：

- `collect` 使用单条聚合 SQL 读取大部分统计字段（`COUNT`, `SUM`, `MIN`, `MAX`, `AVG`），减少查询次数
- 时间窗口统计（`updated_last_24h` / `updated_last_7d`）通过 `now_ms() - window_ms` 计算阈值，不需要外部时间源
- FTS/vector 可用性通过查询 `sqlite_master` 检测表/索引是否存在，不依赖运行时标记

## 错误处理

```rust
pub enum MemoryError {
    InvalidConfig(String),         // 配置校验失败
    InvalidQuery(String),          // 查询参数校验失败
    Provider(String),              // embedding provider API 错误
    Storage(StorageError),         // 底层存储错误
    Serialization(serde_json::Error), // JSON 序列化/反序列化错误
    CapabilityUnavailable(String), // 功能不可用（如 sqlx 后端无 vector）
}
```

设计考量：

- 使用 `thiserror` 拒绝 `unwrap()`，与 workspace lints 保持一致
- `StorageError` 通过 `#[from]` 自动转换，保持错误链可追溯
- `CapabilityUnavailable` 区分"功能不支持"和"功能配置错误"，避免静默降级

## 与 runtime 的集成

### 工具层

`memory` tool（`klaw-tool/src/memory.rs`）是模型侧唯一的记忆操作入口：

- `add`：只写长期记忆，经过 `govern_long_term_write` 治理后调用 `MemoryService::upsert`
- `search`：只查 session 记忆，复用 `SessionStorage` 的 chat JSONL，不走 `MemoryService::search`

### Prompt 注入

runtime（`klaw-cli/src/runtime/mod.rs`）在每轮构建上下文时：

1. 调用 `SqliteMemoryService::list_scope_records("long_term")` 读取所有长期记忆
2. 通过 `render_long_term_memory_section` 渲染为受控文本
3. 拼入 system prompt 的 `Memory` 章节

长期记忆不通过工具检索给模型，而是由 runtime 在每轮开始前自动注入。这避免了模型需要主动搜索长期记忆的不确定性，也确保所有长期记忆在 prompt 中受预算控制。

### 统计面板

GUI Memory 面板通过 `SqliteMemoryStatsService::collect` 和 `list_scope_records` 获取展示数据：

- 总览统计：总记录数、scope 分布、时间窗口活跃度
- 明细视图：按 scope 查看完整记录列表

## 后端策略

| 后端 | feature | 检索能力 | 适用场景 |
|------|---------|---------|---------|
| turso（libSQL） | `turso` | BM25(FTS5) + vector | 需要 embedding 或远程 Turso |
| sqlx（标准 SQLite） | `sqlx` | BM25(FTS5) + LIKE 回退 | 本地单进程，无向量需求 |

- `DefaultMemoryDb` 根据编译 feature 选择后端驱动
- sqlx 后端下 `vector_enabled = false`，`search` 仅走 FTS/LIKE 通道
- 若 `use_vector = true` 但 `vector_enabled = false`，向量通道静默跳过（不报错）

## 设计考量总结

1. **独立数据库**：`memory.db` 与 `klaw.db` 分离，避免 memory 操作影响 session 索引性能
2. **即时 embedding**：写入时计算 embedding 而非检索时计算，减少检索延迟和 API 调用
3. **治理优先**：长期记忆写入强制经过治理流程，确保事实稳定性和可追溯性
4. **Budget 控制**：prompt 渲染使用双重预算（条目数 + 字符数），防止无限膨胀
5. **RRF 融合**：检索不需要原始分数尺度对齐，只需排名序号，实现简洁且效果稳定
6. **FTS 回退**：即使 FTS5 创建失败，退回 LIKE 模式保证基础检索始终可用
7. **状态生命周期管理**：`superseded / archived / rejected` 由系统自动管理，外部只能写入 `active`，避免状态混乱
8. **session 记忆复用**：session 检索复用现有 JSONL 存储，不做双写，避免一致性和性能问题

## 测试覆盖

当前集成测试覆盖以下核心路径：

- upsert + get 读写一致性
- FTS 检索命中（无 vector）
- pin + delete 状态一致性
- 无 embedding provider 下的基础检索
- scope detail 查询排序（pinned > newer > older）
- RRF 融合：双通道命中优先于单通道命中
- embedding provider 配置工厂校验

治理层（`governance.rs`）的单元测试覆盖：

- 默认 kind 和 active status
- 精确重复检测和 ID 复用
- 同 kind + topic 冲突替换
- 拒绝系统托管 status 的外部写入

prompt 渲染层（`prompt.rs`）的单元测试覆盖：

- 格式化和去重
- 跳过 inactive 记录
- 预算裁剪和截断

## 相关文档

- [Memory Tool 设计与实现](../tools/built-in/memory.md) — 工具语义、参数、返回结构
- [存储概述](./overview.md) — memory.db 表结构和索引概览
- [两层 Memory 设计](../plans/two-layer-memory-design.md) — 架构决策与设计目标
- [Memory Turso Hybrid](../plans/memory-turso-hybrid.md) — Turso/libSQL 实施计划
