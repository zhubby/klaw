# Storage 模块设计

`klaw-storage` 是 `klaw` 的本地存储抽象层，目标是：

- 以统一 trait 对外暴露存储能力；
- 使用 feature 切换不同 SQLite 后端实现；
- 默认使用 `turso`，可选 `sqlx`；
- 保持上层（CLI/runtime）不感知后端细节。

## 设计目标

- **后端可替换**：同一套接口支持 `turso` 与 `sqlx`。
- **边界清晰**：`lib.rs` 只做模块导出和 feature 入口。
- **语义稳定**：会话轮次 `turn_count` 的业务规则不因后端变化而变化。
- **文件分层**：路径管理、JSONL 写入、错误模型、后端实现分离。

## 模块结构

`klaw-storage/src/` 当前拆分为：

- `lib.rs`：对外导出、默认后端选择、feature 互斥检查。
- `types.rs`：`SessionIndex`、`ChatRecord` 等公共模型。
- `traits.rs`：`SessionStorage` trait（统一对外接口）。
- `error.rs`：`StorageError`。
- `paths.rs`：`StoragePaths`（`.klaw` 根目录、`klaw.db`、`sessions/`）。
- `jsonl.rs`：聊天记录 JSONL 追加写。
- `util.rs`：时间戳、session_key 编码、相对路径转换工具函数。
- `backend/sqlx.rs`：`sqlx` 后端实现。
- `backend/turso.rs`：`turso` 后端实现。
- `backend/mod.rs`：按 feature 导出后端模块。

## Feature 策略

`klaw-storage/Cargo.toml`：

- 默认 feature：`turso`
- 可选 feature：`sqlx`
- 规则：`turso` 与 `sqlx` 互斥，不允许同时开启。

在 `lib.rs` 中通过 `compile_error!` 强制约束：

- 同时开启 `turso` 和 `sqlx` => 编译失败；
- 两者都不开启 => 编译失败。

## 对外 API 约定

默认后端类型别名：

- `DefaultSessionStore`

统一构造入口：

- `open_default_store() -> Result<DefaultSessionStore, StorageError>`

上层只依赖：

- `SessionStorage` trait
- `ChatRecord` / `SessionIndex` 等公共类型

同时支持定时任务相关接口：

- `CronStorage` trait
- `CronJob` / `CronTaskRun` / `CronTaskStatus` 等类型

## 数据目录约定

- 根目录：`~/.klaw`
- 索引数据库：`~/.klaw/klaw.db`
- 聊天记录目录：`~/.klaw/sessions/`
- 聊天记录文件：`<session_id>.jsonl`

## 定时任务存储

`klaw-storage` 现在在同一个 `klaw.db` 中维护 cron 相关数据：

- `cron`：任务定义（支持 `cron` 与 `every` 两种调度形态）
- `cron_task`：每次运行记录（`pending/running/success/failed`）

详细语义见 [Cron 存储语义](./cron.md)。
