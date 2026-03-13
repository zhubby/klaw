# 存储模块

`klaw-storage` 与相关领域模块共同构成 Klaw 的本地存储层：`klaw-storage` 负责路径与通用持久化抽象，`klaw-memory` 与 `klaw-archive` 在其之上实现各自领域服务。

## 设计目标

- **后端可替换**：同一套接口支持 `turso` 与 `sqlx` 后端
- **边界清晰**：模块职责分离，上层不感知后端细节
- **语义稳定**：业务规则不因后端变化而变化

## 模块结构

```
klaw-storage/src/
├── lib.rs          # 对外导出、feature 入口
├── types.rs        # SessionIndex、ChatRecord 等公共模型
├── traits.rs       # SessionStorage、CronStorage trait
├── error.rs        # StorageError
├── paths.rs        # 路径管理（~/.klaw）
├── jsonl.rs        # 聊天记录 JSONL 追加写
├── util.rs         # 工具函数
└── backend/
    ├── mod.rs      # 按 feature 导出后端
    ├── turso.rs    # libSQL 后端
    └── sqlx.rs     # SQLx 后端
```

## Feature 策略

```toml
# 默认使用 turso
[dependencies]
klaw-storage = { path = "../klaw-storage" }

# 或切换为 sqlx
klaw-storage = { path = "../klaw-storage", default-features = false, features = ["sqlx"] }
```

`turso` 与 `sqlx` 互斥，同时开启会导致编译失败。

## 数据目录

```
~/.klaw/
├── config.toml       # 配置文件
├── klaw.db          # SQLite 索引数据库
├── memory.db        # 记忆数据库（可选）
├── archive.db       # 媒体归档索引数据库
├── sessions/        # 会话 JSONL 文件
│   └── <session_id>.jsonl
└── archives/        # 归档媒体文件
    └── <YYYY-MM-DD>/<uuid>.<ext>
```

## 核心 API

### Session 存储

```rust
pub trait SessionStorage {
    fn touch_session(&self, session_key: &str, ...) -> Result<()>;
    fn complete_turn(&self, session_key: &str, ...) -> Result<()>;
    fn append_chat_record(&self, session_key: &str, record: &ChatRecord) -> Result<()>;
    fn get_session(&self, session_key: &str) -> Result<Option<SessionIndex>>;
    fn list_sessions(&self, limit: u32, offset: u32) -> Result<Vec<SessionIndex>>;
}
```

### Cron 存储

```rust
pub trait CronStorage {
    fn claim_next_run(&self, cron_id: &str, ...) -> Result<Option<CronClaim>>;
    fn append_task_run(&self, cron_id: &str, ...) -> Result<CronTaskRun>;
    fn mark_task_running(&self, task_id: &str, ...) -> Result<()>;
    fn mark_task_result(&self, task_id: &str, status: CronTaskStatus, ...) -> Result<()>;
}
```

### Archive 存储

`klaw-archive` 在 `klaw-storage` 提供的 `DefaultArchiveDb` 和 `StoragePaths` 之上实现媒体归档服务：

```rust
pub trait ArchiveService {
    async fn ingest_path(&self, input: ArchiveIngestInput, source_path: &Path) -> Result<ArchiveRecord>;
    async fn ingest_bytes(&self, input: ArchiveIngestInput, bytes: &[u8]) -> Result<ArchiveRecord>;
    async fn find(&self, query: ArchiveQuery) -> Result<Vec<ArchiveRecord>>;
    async fn get(&self, archive_id: &str) -> Result<ArchiveRecord>;
}
```

## Session 语义

- `turn_count` 表示"已完成轮次"（用户请求 + agent 响应）
- 聊天内容写入 JSONL，索引入库
- 文件名使用 `session_id`（`session_key` 中 `:` 后半段）

## Cron 语义

- 支持 `cron` 和 `every` 两种调度类型
- CAS 并发防重（条件更新 `next_run_at_ms`）
- 任务状态流转：`pending` → `running` → `success/failed`

详细文档：
- [Session 存储](./session.md)
- [Cron 存储](./cron.md)
- [Archive 存储](./archive.md)
