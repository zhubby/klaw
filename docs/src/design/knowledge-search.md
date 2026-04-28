# Knowledge 搜索系统设计文档

本文详细描述 `klaw-knowledge` 模块的架构、索引管线、五路检索算法与融合策略，涵盖从 Obsidian vault 到最终搜索结果的完整链路。

## 系统架构总览

Knowledge 搜索系统由四个子系统组成：

```
┌─────────────────────────────────────────────────────────────────┐
│                        调用层                                   │
│  klaw-tool (KnowledgeTool)                                     │
│  klaw-runtime (KnowledgeRuntimeService)                        │
│  klaw-gui (Knowledge 面板)                                     │
└──────────────────────┬──────────────────────────────────────────┤
│                      ▼                                         │
│  ┌──────────────────────────────────────────────────┐          │
│  │  ObsidianKnowledgeProvider                        │          │
│  │  ┌─────────────────┐  ┌──────────────────────┐   │          │
│  │  │ 搜索管线         │  │ 索引管线             │   │          │
│  │  │ 5-lane + RRF     │  │ parse → chunk → embed │   │          │
│  │  │ + rerank + fuse  │  │ → FTS → links → DB   │   │          │
│  │  └─────────────────┘  └──────────────────────┘   │          │
│  └──────────┬───────────────┬────────────────────────┘          │
│             ▼               ▼                                    │
│  ┌──────────────┐  ┌──────────────────┐                         │
│  │ SQLite/      │  │ klaw-model       │                         │
│  │ libSQL DB    │  │ Embedding/Rerank  │                         │
│  │ knowledge.db │  │ /Orchestrator     │                         │
│  └──────────────┘  └──────────────────┘                         │
└─────────────────────────────────────────────────────────────────┘
```

## 索引管线

### 1. Vault 扫描与文件发现

`index_vault` 使用 `ignore` crate 递归扫描 vault 目录，收集所有 `.md` 文件：

```text
vault_root/
  ├─ 排除 exclude_folders (.obsidian, node_modules, templates 等)
  ├─ 递归遍历所有 .md 文件
  ├─ 构建全局 name_index（标题/别名/路径 → NameEntry）
  └─ 移除 DB 中已不存在文件的旧索引（remove_missing_entries）
```

### 2. Note 解析

`parse_note` 将 Markdown 解析为结构化 `ParsedNote`：

```text
输入: Markdown 文件内容
    │
    ├─ split_frontmatter → 提取 YAML frontmatter（--- ... ---）
    │   ├─ title（frontmatter.title || 首个 # 标题）
    │   ├─ tags（frontmatter.tags: [rust, async]）
    │   ├─ aliases（frontmatter aliases 列表）
    │   └─ date（frontmatter date，仅接受 YYYY-MM-DD 格式）
    │
    ├─ parse_wikilinks → 正则提取 [[NoteName|#anchor|display]]
    │
    ├─ parse_inline_tags → 正则提取 #tag-name
    │
    └─ body → 剩余 Markdown 正文
```

### 3. 智能分块（Smart Chunking）

`chunk_markdown` → `smart_chunk` 是 Knowledge 的核心分块算法，目标是 **保持语义连贯性** 的前提下将长文档拆为 ~512 token 的块。

#### 断点评分系统

`find_break_points` 为每个行计算断点分数：

| 行类型 | 分数 | 说明 |
|-------|------|------|
| `# 标题` | 100 | 一级标题，最强断点 |
| `## 标题` | 90 | 二级标题 |
| `### 标题` | 80 | 三级标题 |
| `---` / `***` / `___`（主题分隔线） | 60 | 语义分隔 |
| 代码围栏边界 ` ``` ` | 80 | 代码块边界 |
| 空行 | 20 | 段落分隔 |
| 列表项 `- ` / `* ` / `1.` | 5 | 列表内断点 |
| 普通文本 | 1 | 最弱断点 |

#### 切块策略

`smart_chunk(content, target_tokens=512, overlap_pct=15)`：

```text
1. 若整篇文档 ≤ target_tokens → 不切分，返回单块
2. 否则循环切分：
   a. 从 start_offset 开始，目标长度 target_chars = target_tokens × 4
   b. 在 start_offset..start_offset + target_chars × 2 范围内
      寻找最佳断点：
      weighted_score = 断点分数 × (1 / (1 + |断点偏移 - 理想偏移| / 500))
   c. 取 weighted_score 最高的断点作为切分位置
   d. 若无合适断点 → fallback 到目标长度处的最后一个换行符
   e. 下一块的 start_offset = cut_offset - overlap_chars（15% 重叠）
   f. 重叠区域不在代码围栏内时才启用
```

#### 代码围栏保护

- 代码围栏行标记为 `inside_code_fence=true`，断点分数仅 1
- 代码围栏边界（``` 本身）标记为分数 80 的断点
- 切块时 `filter(!inside_code_fence)` 禁止在代码块内部切分
- 重叠偏移若落在代码围栏内则跳过重叠，直接从 cut_offset 开始下一块

#### 超大块二次切分

若某个块超过 `max_tokens`，`split_oversized_chunks` 按句子边界（`.` + 空格 或换行符）进一步切分，后续子块标题标注 `(cont.)`，重叠由 `build_token_overlap` 反向构建。

### 4. Embedding 写入

每个 chunk 的 embedding 写入流程：

```text
chunk.heading + "\n\n" + chunk.content
    → embedder.embed(text)
    → Vec<f32> (如 EmbeddingGemma-300M 输出)
    → ensure_vector_index(db, dimensions) → F32_BLOB(dimensions) 列类型
    → serialize_embedding → Blob 存储
```

`embed_missing_chunks` 查询所有 `embedding IS NULL` 的 chunk，逐一嵌入并写入。

### 5. FTS5 索引

```text
CREATE VIRTUAL TABLE knowledge_fts USING fts5(
    chunk_id UNINDEXED,
    entry_id UNINDEXED,
    title,
    aliases,
    tags,
    content
);
```

- 若 SQLite 不支持 FTS5（`no such module: fts5`），自动回退为普通表
- 搜索时 FTS5 可用走 `MATCH + bm25()`，不可用走 token scoring

### 6. 链接发现与图构建

`discover_links` 在已有 wikilinks 之外自动识别四种隐式链接：

| 匹配类型 | 说明 | confidence |
|---------|------|-----------|
| **ExactName** | 文本中出现另一篇笔记的标题 | 高 |
| **Alias** | 文本中出现另一篇笔记的 aliases | 中 |
| **FuzzyName** | Levenshtein 相似度 ≥ 0.92 | 低 |
| **FirstName** | People/人物文件夹下的唯一首名 | 中 |

链接写入 `knowledge_links` 表，存储 `source_entry_id → target_entry_id / target_title / match_type / confidence_bp`。

链接发现 **不改写用户 Markdown 文件**，仅在数据库中构建图边。

### 7. 数据库 Schema

```sql
-- 笔记条目
knowledge_entries (id, title, uri, tags_json, aliases_json, metadata_json, content, note_date, created_at_ms, updated_at_ms)

-- 分块
knowledge_chunks (id, entry_id, heading, content, snippet, embedding F32_BLOB(N))

-- 全文搜索
knowledge_fts (chunk_id, entry_id, title, aliases, tags, content)  -- FTS5 或普通表

-- 链接图
knowledge_links (source_entry_id, target_title, target_entry_id, matched_text, match_type, confidence_bp)

-- 元数据
knowledge_metadata (key, value)  -- 存储 embedding 维度、vector index 状态等
```

## 搜索管线

### 整体流程

一次搜索请求的完整执行路径：

```text
query.text
    │
    ├─ 1. orchestration_plan → intent + expansions
    │     ├─ 有 orchestrator → 模型推理 → QueryIntent + query expansions
    │     └─ 无 orchestrator → heuristic → Exploratory + [原始查询]
    │
    ├─ 2. 四路并行检索
    │     ├─ semantic_lane(expanded_queries, limit×4)
    │     ├─ fts_lane(expanded_queries, limit×3)
    │     ├─ graph_lane(merge_seed_hits(semantic, fts), limit)
    │     ├─ temporal_lane(query, limit)
    │
    ├─ 3. Pass1 融合（4路 WRRF）
    │     weighted_reciprocal_rank_fuse(semantic, fts, graph, temporal, k=60)
    │
    ├─ 4. Rerank 精排
    │     ├─ 有 reranker → rerank_lane(query, pass1, limit×4)
    │     └─ 无 reranker → 返回空 Vec
    │
    ├─ 5. 最终融合（5路 WRRF）
    │     weighted_reciprocal_rank_fuse(semantic, fts, graph, temporal, rerank, k=60)
    │
    └─ 6. 截断 + 元数据加载
         take(limit) → load_entry_metadata → KnowledgeHit[]
```

### Lane 1: Semantic（语义检索）

```text
embedder.embed(query) → query_vector
    │
    ├─ vector_index_enabled=true?
    │   ├─ YES → semantic_lane_vector_top_k
    │   │   SELECT ... FROM vector_top_k(idx, query_vector, limit)
    │   │   score = 1.0 - distance
    │   │
    │   ├─ 失败 (no such function) → 降级到 SQL distance
    │   │
    │   └─ NO → semantic_lane_sql_distance
    │   │   SELECT ... WHERE embedding IS NOT NULL
    │   │   ORDER BY vector_distance_cos(embedding, query_vector) ASC
    │   │   score = 1.0 - distance
    │   │
    │   └─ SQL distance 也失败 → semantic_lane_fallback
    │      SELECT ... WHERE embedding IS NOT NULL
    │      Rust 端 cosine_similarity(query_vector, chunk_embedding)
    │      排序取 top-K
```

**关键特性：**
- 无 embedder → 返回空 Vec，语义管线跳过
- 三级降级策略：ANN 索引 → SQL 函数 → Rust fallback
- query expansion 后每个 expansion 都做一次语义搜索，结果合并去重

### Lane 2: FTS（全文检索）

```text
fts_virtual=true?  (FTS5 可用)
    ├─ YES → MATCH + bm25() 排序
    │   score = 1.0 / (|bm25_rank| + 1.0)
    │
    └─ NO → 普通表全量扫描 + token scoring
       tokenize_query → 分词
       haystack.contains(token) → 匹配计数
       score = matches_count
```

**关键特性：**
- FTS5 可用时性能高（索引级 MATCH），bm25 自然处理词频和文档长度归一化
- FTS5 不可用时用 Rust 端 token scoring 模拟，性能较差但保证可用
- query expansion 后每个 expansion 都做一次 FTS 搜索，结果合并去重

### Lane 3: Graph（图关联检索）

```text
输入: merge_seed_hits(semantic_top, fts_top) → seed entries
    │
    ├─ 对每个 seed entry:
    │   SELECT target.id, target.title, substr(target.content, 1, 400)
    │   FROM knowledge_links
    │   JOIN knowledge_entries target ON (target.id = link.target_entry_id
    │       OR (target_entry_id IS NULL AND lower(target.title) = lower(link.target_title)))
    │   WHERE source_entry_id = seed.id
    │
    └─ 合并去重 → 所有 seed 的出边目标
```

**关键特性：**
- 图检索依赖 semantic 和 fts 的种子结果，形成"先检索再跳转"的两步模式
- 支持 `target_entry_id` 直接匹配和 `target_title` 模糊匹配两种路径
- 默认 score = 0.5（较低，表示间接相关性）

### Lane 4: Temporal（时间检索）

```text
temporal_pattern(query) → SQL LIKE pattern
    │
    ├─ 无法提取时间模式 → 返回空 Vec
    │
    └─ 有模式:
       SELECT ... WHERE note_date LIKE pattern
       ORDER BY updated_at_ms DESC
       score = 0.4
```

**时间模式提取 (`temporal_pattern`)：**

仅当查询包含时间关键词时激活：
- `today` / `yesterday` → 当日/前一日日期
- `recent` / `latest` / `last week` / `this month` → 近期模式
- `2024` / `2025` → 年份模式

### Lane 5: Rerank（二次精排）

```text
输入: pass1 融合结果 + 原始查询
    │
    ├─ 无 reranker → 返回空 Vec（管线跳过）
    │
    └─ 有 reranker:
       ├─ 取 pass1 前 limit×4 个候选
       ├─ 对每个候选调用 reranker.rerank(query, candidate_excerpt)
       │   → Qwen3-Reranker Yes/No softmax → P(Yes) = score
       ├─ 按 score 降序排列
       └─ 返回 RankedHit[]
```

**关键特性：**
- reranker 对第一次融合结果做二次判断，权重最高 (4.0)
- 无 reranker 时返回空 Vec，不参与最终融合，其他 4 路权重不变
- reranker 评分 [0, 1] 区间，天然适配 RRF 融合

## 融合算法：Weighted Reciprocal Rank Fusion (WRRF)

### 核心公式

```
对于每个候选文档 d，来自 lane L_i 的贡献：

score(d, L_i) = weight_i / (k + rank_i(d) + 1)

最终分数：
final_score(d) = Σ_i score(d, L_i)

其中：
- k = 60（常数，控制排名衰减速度）
- rank_i(d) = 文档 d 在 lane L_i 中的排名（0-based）
- weight_i = lane 权重（由 QueryIntent 决定）
```

### 算法实现

```text
1. 遍历每个 lane 的 hits
2. 对每个 hit：
   - 若首次出现 → 创建 FusedHit，lane_score = weight / (k + rank + 1)
   - 若已存在 → 累加 lane_score
   - 记录来源 lane 名称到 lanes[]
3. 按 score 降序排列（score 相等时按 title 字典序）
4. 返回 FusedHit[]（每个 hit 记录来自哪些 lane）
```

### Lane 权重表

| QueryIntent | semantic | fts | graph | temporal | rerank | 说明 |
|-------------|---------|-----|-------|----------|--------|------|
| **Temporal** | 1.1 | 1.0 | 0.8 | **1.6** | 4.0 | 时间相关查询突出 temporal |
| **Relationship** | 0.9 | 0.9 | **1.6** | 0.5 | 4.0 | 关联查询突出 graph |
| **Exact** | 0.8 | **1.6** | 0.8 | 0.5 | 4.0 | 精确查询突出 fts |
| **Conceptual** | **1.5** | 0.9 | 1.1 | 0.5 | 4.0 | 概念查询突出 semantic |
| **Exploratory** | **1.4** | 1.1 | 0.9 | 0.5 | 4.0 | 开放探索突出 semantic |

**设计原则：**

- **rerank 权重恒为 4.0**，远高于其他管线，因为 rerank 是在融合结果之上的二次判断，信号质量最高
- **每种 intent 突出对应管线**，降低不相关管线
- **temporal 在非时间型 intent 下统一为 0.5**（最低），大部分查询不关心时间
- **无 reranker 时 rerank 返回空 Vec**，权重 4.0 无 hits 可作用，等效跳过，权重不重新分配

### 融合示例

假设搜索 "how does async work"，intent = Conceptual，权重：semantic=1.5, fts=0.9, graph=1.1, temporal=0.5

```text
semantic lane: [async_futures(rank=0), tokio_runtime(rank=1), async_await(rank=2)]
fts lane:      [async_futures(rank=0), rust_async(rank=1)]
graph lane:    [tokio_runtime(rank=0)]
temporal lane: [] (无时间关键词)

"async_futures" 的融合分数:
  semantic: 1.5 / (60 + 0 + 1) = 0.0246
  fts:      0.9  / (60 + 0 + 1) = 0.0148
  total:    0.0394  (来自 2 个 lane)

"tokio_runtime" 的融合分数:
  semantic: 1.5 / (60 + 1 + 1) = 0.0242
  graph:    1.1  / (60 + 0 + 1) = 0.0180
  total:    0.0422  (来自 2 个 lane)
```

即使 `async_futures` 在两个 lane 中都排名第一，`tokio_runtime` 因为在 graph lane 排名更高仍可能胜出——WRRF 体现了"多路信号共识"优于"单路高分"的理念。

## Orchestrator 子系统

### 模型驱动 Orchestrator

有 orchestrator 模型时：

```text
query → format_orchestrator_prompt(query)
       → ModelChatRequest → 本地 GGUF 推理
       → parse_orchestrator_response
          ├─ QueryIntent (exact/conceptual/relationship/exploratory/temporal)
          ├─ expansions (2-4 个扩展查询，原始查询必须为第一个)
          └─ 失败 → heuristic_orchestrator_response 回退
```

### 启发式 Orchestrator

无 orchestrator 模型时自动回退：

**Intent 启发式：**

| 触发条件 | Intent |
|---------|--------|
| 包含 `today`/`recent`/`latest`/`yesterday`/`last week`/`this month`/`202` | `Temporal` |
| 包含 `[[...]]`/`relationship`/`related`/`link`/`backlink`/`depends on` | `Relationship` |
| 包含引号`"`/`exact`/`quote`/`id:` | `Exact` |
| 包含 `how`/`why`/`architecture`/`design`/`concept` | `Conceptual` |
| 其他 | `Exploratory` |

**Expansion 启发式：**

- 原始查询作为第一个 expansion
- 超过 2 词时去掉停用词（how/does/the/a/an/...）保留长词
- 去重返回

## Query Expansion 机制

`expanded_semantic_lane` 和 `expanded_fts_lane` 对每个 expansion 都执行一次检索：

```text
expansions = ["how does async work", "async", "work"]
    │
    ├─ semantic: for each expansion → semantic_lane(expansion, limit) → 合并去重
    ├─ fts:      for each expansion → fts_lane(expansion, limit) → 合并去重
    │
    └─ 最终结果包含多个角度的命中，降低遗漏概率
```

expansion 列表始终以原始查询为首，且去重后传递。

## 降级与容错

### Semantic 三级降级

| 级别 | 条件 | 实现 |
|------|------|------|
| 1 | `vector_top_k` 索引可用 | SQL `vector_top_k(idx, vec, N)` ANN 查询 |
| 2 | `vector_distance_cos` 函数可用 | SQL `ORDER BY vector_distance_cos` 线性扫描 |
| 3 | 两者均不可用 | Rust 端全量读取 + `cosine_similarity` 排序 |

级别 1 失败时自动标记 `vector_index_enabled=false`，后续查询直接从级别 2 开始。

### FTS5 降级

| 级别 | 条件 | 实现 |
|------|------|------|
| 1 | FTS5 模块可用 | `MATCH ? + bm25()` 索引搜索 |
| 2 | FTS5 不可用 | 普通表全量扫描 + Rust token scoring |

### Rerank 降级

无 reranker → 返回空 Vec → rerank lane 在融合中贡献 0 分 → 不影响其他 4 路权重比例。

### Orchestrator 降级

推理失败或无模型 → `heuristic_orchestrator_response` → Exploratory intent + 基本扩展。

## Context Bundle 组装

`assemble_context_bundle` 在搜索结果之上组装 token 预算内的上下文包：

```text
hits → 按 score 顺序遍历
    │
    ├─ 每个 section 开销: SECTION_OVERHEAD = 80 chars
    ├─ 预算内可用: budget - total_used - SECTION_OVERHEAD
    ├─ excerpt 超出可用空间 → 截断 + "... [truncated]" 标记
    └─ total_chars 超出 budget → truncated=true, 剩余 section 跳过
```

输出 `ContextBundle` 结构：

```json
{
  "topic": "async rust",
  "sections": [
    {
      "label": "Direct match",
      "title": "Async Futures",
      "uri": "rust/async.md",
      "content": "...",
      "relevance": "score 0.89"
    }
  ],
  "total_chars": 350,
  "budget_chars": 500,
  "truncated": false
}
```

## Runtime 生命周期

`KnowledgeRuntimeService` 管理 provider 的加载、重载和关闭：

```text
start_configured(config)
    │
    ├─ 若 knowledge.enabled=true → load_provider(config)
    │   ├─ 打开 obsidian provider (db + vault)
    │   ├─ 绑定 embedding/reranker/orchestrator 模型
    │   ├─ set_snapshot(Ready)
    │   └─ 若 auto_index=true → start_auto_index_watcher
    │
    ├─ 若 knowledge.enabled=false → set_snapshot(Disabled)
    │
    └─ reload(config) → 重新加载 provider
       ├─ 停止旧 auto_index watcher
       ├─ 释放旧 provider
       └─ 创建新 provider
```

## 配置体系

### 完整配置示例（含测试模型）

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
top_k = 5              # 最终返回条目数
rerank_candidates = 20 # rerank 前候选数量
graph_hops = 1         # 图跳转深度
temporal_decay = 0.85  # 时间衰减因子

[knowledge.models]
embedding_provider = "local"
embedding_model_id = "unsloth__embeddinggemma-300m-GGUF--main"
orchestrator_model_id = ""   # 可选，留空则启发式
reranker_model_id = "ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main"

[models]
enabled = true
default_embedding_model_id = "unsloth__embeddinggemma-300m-GGUF--main"
default_reranker_model_id = "ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main"

[models.huggingface]
endpoint = "https://huggingface.co"

[models.llama_cpp]
command = "llama-cli"
default_ctx_size = 4096

[tools.knowledge]
enabled = true
search_limit = 5
context_limit = 3
include_explain = true
```

### 模型绑定优先级

```text
knowledge.models.embedding_model_id  → 若存在则使用
                                    → 否则回退到 models.default_embedding_model_id

knowledge.models.reranker_model_id   → 若存在则使用
                                    → 否则回退到 models.default_reranker_model_id

knowledge.models.orchestrator_model_id → 仅 knowledge 级配置，无全局回退
```

### 验证约束

- `knowledge.enabled=true` 时必须配置 `knowledge.obsidian.vault_path`
- `tools.knowledge.enabled=true` 需要 `knowledge.enabled=true`
- `knowledge.retrieval.rerank_candidates` 必须 > 0（启用时）
- `knowledge.retrieval.top_k` 必须 > 0（启用时）

## 测试模型推荐

| 模型 | repo_id | 用途 | 推荐量化 |
|------|---------|------|---------|
| **EmbeddingGemma-300M** | `unsloth/embeddinggemma-300m-GGUF` | chunk embedding + 语义检索 | 多种量化可选，300M 参数轻量 |
| **Qwen3-Reranker-0.6B-Q8_0** | `ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF` | 二次精排（Yes/No softmax） | Q8_0 精度保证 logit 质量 |

### 安装步骤

```bash
# 1. 安装 embedding 模型
klaw model install unsloth/embeddinggemma-300m-GGUF main

# 2. 安装 reranker 模型
klaw model install ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF main

# 3. 配置 config.toml（见上方示例）

# 4. 启动 klaw → GUI 中 Sync Index & Vectors

# 5. 测试搜索
klaw agent --input "how does async work in Rust"
```

## Crate 依赖关系

```text
klaw-config → 配置结构、验证、默认值
klaw-storage → SQLite/libSQL DatabaseExecutor
klaw-model   → ModelService, Embedding/Rerank/Orchestrator Runtime
klaw-knowledge → ObsidianKnowledgeProvider, 索引/搜索/融合
klaw-tool    → KnowledgeTool (Tool trait 实现)
klaw-runtime → KnowledgeRuntimeService, provider 注册与生命周期
```