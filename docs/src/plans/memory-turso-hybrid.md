# Memory 模块实施计划（Turso/libSQL 优先）

## Summary

- 在 `docs/src/plans` 新增计划文档，明确 memory 的目标架构：`BM25(FTS5) + Vector(libSQL native)`，并保留本地 SQLite 回退路径。
- 立即进入实现阶段：完成 `klaw-config`、`klaw-storage`、`klaw-memory` 三个 crate 的接口与实现，并补齐测试。
- 关键约束：Turso 路径不依赖 `sqlite-vec`；向量能力使用 `F32_BLOB + libsql_vector_idx + vector_top_k`。

## Implementation Changes

1. 计划文档落盘

- 新建 `docs/src/plans/memory-turso-hybrid.md`。
- 更新 `docs/src/SUMMARY.md` 挂载该页面。

2. 配置层（`klaw-config`）

- `AppConfig` 增加 `memory: MemoryConfig`。
- `MemoryConfig.embedding` 增加：
  - `provider`（映射到 `model_providers.<id>`）
  - `model`
- `validate()` 增加：
  - `memory.embedding.provider` 非空且存在于 `model_providers`
  - `memory.embedding.model` 非空
- 默认模板生成对应 `memory.embedding.*` 字段。

3. storage 抽象层（`klaw-storage`）

- `StoragePaths` 增加 `memory_db_path`（`~/.klaw/memory.db`）。
- 新增 memory DB 打开/初始化抽象（仅连接与 schema 管理能力，不放检索策略）。
- 保持现有 session store 行为不变。

4. memory 服务层（`klaw-memory`）

- 新增统一 trait：
  - `MemoryService`：`upsert`, `search`, `get`, `delete`, `pin/unpin`
  - `EmbeddingProvider`：`embed_texts`
- 新增 provider 工厂：从 `memory.embedding.provider/model` + `model_providers` 构建 embedding provider。
- 存储设计（Turso/libSQL）：
  - `memories` 主表（内容、scope、metadata、pinned、timestamps）
  - `memories_fts`（FTS5）
  - 向量列（`F32_BLOB`）与向量索引（`libsql_vector_idx`）
- 检索流程：
  - FTS5 top-k
  - `vector_top_k` top-k
  - RRF 融合输出统一 `MemoryHit`

5. 后端策略

- `turso` feature：使用 libSQL SQL 方言与向量函数。
- `sqlx` feature：先支持 BM25；vector 若环境无等价能力则返回明确 `CapabilityUnavailable`（不静默降级）。

## Test Plan

- `klaw-config`：
  - 默认模板含 `memory.embedding.*`
  - provider/model 缺失报错
- `klaw-memory`：
  - upsert 后 FTS 命中
  - upsert 后 vector 命中（turso 后端）
  - RRF 融合顺序稳定
  - pin/unpin 与 delete 一致性
- `klaw-storage`：
  - `memory_db_path` 正确生成
  - init 幂等
- 全仓回归：
  - `cargo test --workspace`
  - `cargo check --workspace`

## Assumptions

- 当前轮仅完成模块能力与测试，不把 memory 注入 `AgentLoop::BuildingContext`。
- Turso 侧向量与 FTS 能力按 libSQL 文档可用特性实现；若运行环境缺少能力，显式返回错误。
- 计划文档命名使用 `memory-turso-hybrid.md`，后续可按需要重命名。
