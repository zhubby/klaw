# Knowledge Capability Roadmap

本文档记录 `klaw-knowledge` 的当前实现边界，以及后续演进路线。

## 当前状态

当前版本已经具备以下基础能力：

- `klaw-knowledge` 独立 crate 与 provider router
- Obsidian V1 parser / chunker / indexer / provider
- `knowledge.db` 索引存储
- `knowledge` 内建工具：`list_sources/search/get/context`
- `ContextBundle` 裁剪与预算控制
- FTS / graph / temporal lane 检索与 RRF 融合

## Context Bundle

当前 `context` 动作会：

1. 先执行知识检索
2. 按相关性选取若干 `KnowledgeHit`
3. 在字符预算内组装 `ContextBundle`
4. 返回结构化 section，供 agent 直接消费

后续增强方向：

- 更细粒度的 chunk 合并策略
- “主题摘要 + 证据片段” 双层 bundle
- 按 tool budget / model window 自适应裁剪

## Watcher 与增量索引

当前索引器已支持基于 `updated_at_ms` 的跳过式重建：文件未变化时不会重复写入 entry/chunk/link 记录。

下一阶段演进：

1. 持续完善 `auto_index` 文件 watcher，监听 vault 内 `.md` 新增、修改、删除
2. 将首次全量同步保持为 GUI 手动操作，后台 watcher 只处理已有索引的补偿和后续变化
3. 记录 tombstone / delete 事件，及时清理已删除 note 的索引残留

## Hybrid Retrieval

当前已落地的 lane：

- `fts`
- `graph`
- `temporal`

已经落地的检索编排基础：

- provider 级搜索入口
- RRF fusion
- `ContextBundle`
- lane metadata 回传

下一阶段演进：

1. 接入本地 `llama.cpp` embedding lane
2. 为 top candidates 增加 rerank lane
3. 引入 orchestrator，对 query expansion、lane 权重和时间意图做自适应调度
4. 将 explain/trace 输出显式暴露给 `tools.knowledge.include_explain`

## 多 Provider 平台化

当前只启用 `obsidian` provider。

平台化阶段建议：

1. 抽象 provider lifecycle：`open/index/search/get/list_sources`
2. 统一 entry/chunk/link 元模型，避免 provider 各自定义 tool 协议
3. 新增 provider 路由层的聚合搜索能力
4. 按 source capability 暴露 `semantic/fts/graph/temporal/rerank` 可用性

候选 provider：

- Notion
- Google Drive
- Web capture / saved pages
- 团队文档仓库

## 本地模型路线

当前 `klaw-knowledge` 已预留：

- `EmbeddingModel`
- `RerankModel`
- `OrchestratorModel`

建议下一阶段直接补齐本地 `llama.cpp` 模型适配层，并将：

- embedding
- orchestrator
- reranker

统一收敛到 `knowledge.models.*` 配置。
