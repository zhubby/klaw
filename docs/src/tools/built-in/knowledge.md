# Knowledge Tool 设计与实现

本文档记录 `knowledge` 工具的当前语义、配置入口，以及它与 `memory` / `local_search` / `file_read` 的边界。

## 当前定位

`knowledge` 是一个面向外部知识库的检索工具，当前也支持受限写回到 Obsidian vault。

它面向：

- Obsidian vault 等用户已有知识源
- 项目文档、研究笔记、个人知识库
- 基于 chunk / link / context bundle 的检索增强

它不承担：

- 长期记忆写入或事实治理
- session 记忆召回
- 任意文件系统遍历或源码 grep

## 与其他工具的边界

### `knowledge` vs `memory`

- `memory.add` 写入 agent 自身长期记忆
- `memory.search` 检索 session 记忆视图
- `knowledge` 可显式写回用户知识库，但不写 `memory.db`
- `knowledge` 不进入 runtime 的长期记忆 prompt 注入链路

### `knowledge` vs `local_search`

- `local_search` 面向工作区源码/文本的广义本地搜索
- `knowledge` 面向结构化知识源，支持 note / chunk / link / context 语义

### `knowledge` vs `file_read`

- `file_read` 读取已知路径
- `knowledge` 从未知位置的知识库中先检索、再返回候选内容

## 当前动作

`knowledge` 当前暴露五个动作：

- `list_sources`
- `search`
- `get`
- `context`
- `create_note`

### `list_sources`

返回当前已连接的知识源列表、provider 名称和 entry 数量。

### `search`

输入自然语言查询，返回命中的知识条目摘要。

支持参数：

- `query`
- `tags`
- `limit`
- `source`
- `mode`

### `get`

按 `id` 或 `uri` 获取完整条目内容。

### `context`

先执行检索，再组装 token/字符预算内的 `ContextBundle`，供 agent 直接消费。

支持参数：

- `query`
- `limit`
- `budget_chars`

### `create_note`

按 vault 内相对路径创建一篇新的 Markdown 笔记，并在写入后立即做单文件增量索引。

支持参数：

- `path`
- `content`
- `source`

首期约束：

- 仅支持 Obsidian provider
- `path` 必须是 vault 内相对路径，且以 `.md` 结尾
- 不允许绝对路径或 `..`
- 若目标笔记已存在，调用失败，不覆盖、不追加

## 配置

`knowledge` 默认关闭；只有显式配置 knowledge source 后才建议启用。`auto_index`
开启后会在首次手动同步完成后监听 vault 中的 Markdown 变化并自动更新索引；
空库首次建索引仍需在 GUI 中手动执行 `Sync Index & Vectors`。

```toml
[knowledge]
enabled = true
provider = "obsidian"

[knowledge.obsidian]
vault_path = "/Users/me/Knowledge"
auto_index = true
max_excerpt_length = 400
exclude_folders = [".obsidian", "node_modules", "templates"]

[knowledge.retrieval]
top_k = 5
rerank_candidates = 20
graph_hops = 1
temporal_decay = 0.85

[knowledge.models]
embedding_model_id = "Qwen__Qwen3-Embedding-0.6B-GGUF--main"
reranker_model_id = "Qwen__Qwen3-Reranker-0.6B-GGUF--main"

[models]
enabled = true
default_embedding_model_id = "Qwen__Qwen3-Embedding-0.6B-GGUF--main"
default_reranker_model_id = "Qwen__Qwen3-Reranker-0.6B-GGUF--main"

[models.llama_cpp]
command = "llama-cli"

[tools.knowledge]
enabled = true
search_limit = 5
context_limit = 3
include_explain = true
```

关键约束：

- `knowledge.enabled=true` 时必须配置 `knowledge.obsidian.vault_path`
- `tools.knowledge.enabled=true` 需要 `knowledge.enabled=true`
- `knowledge.models.*_model_id` 优先于 `models.default_*_model_id`
- `knowledge` 通过 `klaw-model` 暴露的本地 runtime trait 消费本地模型，不再直接持有裸文件路径
- 若配置了 `knowledge.models.orchestrator_model_id`，搜索前会先通过本地 orchestrator 产出 query expansion 与 intent，再驱动 lane 权重
- 若配置了 `knowledge.models.embedding_model_id`，索引时会为 chunk 写入本地 embedding，查询时启用 semantic lane
- 若配置了 `knowledge.models.reranker_model_id`，会在 RRF 初筛后执行本地 rerank 二阶段重排

## Runtime 接入路径

- 配置结构：`klaw-config`
- 数据库存储：`knowledge.db`（由 `klaw-storage` 路径层统一管理）
- 本地模型资产：`klaw-model`
- 知识实现：`klaw-knowledge`
- 工具实现：`klaw-tool/src/knowledge.rs`
- runtime 注册：`klaw-runtime` 的 `register_configured_tools`

## 当前实现摘要

当前 V1 已落地：

- `klaw-knowledge` 独立 crate
- Obsidian frontmatter / wikilink / inline tag 解析
- markdown 智能分块：标题按层级 100-50 分、代码围栏 80 分、主题分隔符 60 分、空行 20 分，并保护代码块不被切开
- 链接发现会在已有 wikilinks 之外识别精确名称、alias、Levenshtein 0.92 模糊匹配和 People 首名唯一匹配；自动应用仅写入 `knowledge_links` 图边，不改写用户 Markdown 文件
- `knowledge_entries` / `knowledge_chunks` / `knowledge_links` / `knowledge_fts` 索引表
- FTS5 可用时走 `MATCH`，不可用时自动回退为普通表 + token scoring
- 默认 Turso/libSQL 后端会在写入 embedding 时把 chunk 向量列初始化为原生 `F32_BLOB`，并尝试创建 `libsql_vector_idx`
- semantic lane 优先使用 `vector_top_k` 查询 Turso 向量索引；当当前本地后端不支持 ANN 索引时，退到数据库内 `vector_distance_cos ... ORDER BY ... LIMIT`，最后才使用 Rust 余弦 fallback
- semantic / FTS / graph / temporal / rerank 五路检索与 weighted RRF fusion
- 可选本地 orchestrator query expansion + intent classification（由 `klaw-model` 驱动）
- `ContextBundle` 组装
- `create_note` 受限写回：原子创建 Markdown 笔记并立即索引，供后续 `search/get/context` 消费

本期仍保持为内部 runtime/tool 能力，不暴露 HTTP/REST 或外部 MCP 服务。
