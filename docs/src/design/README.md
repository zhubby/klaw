# 详细设计

本章节记录 klaw 核心功能模块的深度设计文档，涵盖架构、算法、数据结构与配置体系。

- [本地模型系统](./local-models.md) — `klaw-model` 的存储-服务-运行时三层架构，GGUF 模型下载、推理引擎（Embedding/Rerank/Chat/Orchestrator）核心算法，以及模型绑定与配置体系
- [Knowledge 搜索系统](./knowledge-search.md) — `klaw-knowledge` 的 Obsidian 索引管线、五路检索算法（semantic/fts/graph/temporal/rerank）、Weighted Reciprocal Rank Fusion 融合策略，以及 orchestrator 与降级容错机制