# Knowledge 存储

`klaw-knowledge` 提供本地知识库（Obsidian vault）的索引、搜索、上下文组装和自动监控，是 Klaw 知识检索系统的核心模块。它支持 FTS5 全文检索与向量语义检索的多通道融合，并通过意图感知的权重分配实现最优排序。

## 设计目标

- **本地知识索引与搜索**：支持 Obsidian vault 的 Markdown 笔记索引，通过混合检索（FTS5 + vector + graph + temporal + rerank）实现高质量知识召回
- **后端可替换**：同一套 `KnowledgeProvider` trait 支持 Turso（libSQL）和 SQLite 后端，FTS5 和 vector 索引能力根据后端自动适配
- **智能分块**：基于语义断点评分的 Smart Chunking 算法，保护代码围栏完整性，避免在代码块内部切割
- **意图感知检索**：通过 Orchestrator 对查询意图分类，动态调整各检索通道权重，实现精确、概念、关系、时间等不同意图的最优召回策略
- **多通道融合**：五通道并行检索后通过 Weighted Reciprocal Rank Fusion（WRRF）融合，跨通道命中获得更高分数
- **三级语义降级**：向量检索支持 `vector_top_k` → `vector_distance_cos` SQL → Rust cosine fallback 三级降级，保证语义检索在所有后端可用
- **链接图谱构建**：自动发现 Obsidian wikilink、别名链接、模糊匹配和人名首名匹配，构建知识间的关系图谱并用于图遍历检索
- **受控上下文注入**：`assemble_context_bundle` 基于字符预算组装上下文，防止 prompt 膨胀
- **自动增量索引**：通过文件系统 watcher 监控 vault 变更，实现增量索引和实时同步

## 模块结构

```text
klaw-knowledge/src/
├── lib.rs              # 对外导出、open_configured_obsidian_provider、sync、status 入口
├── types.rs            # KnowledgeProvider trait、数据模型、KnowledgeRuntimeState/Snapshot
├── context.rs          # ContextBundle 组装、字符预算截断
├── error.rs            # KnowledgeError 错误枚举
├── provider_router.rs  # KnowledgeProviderRouter 多 provider 路由
├── models/
│   └── mod.rs          # EmbeddingModel/RerankModel/OrchestratorModel trait、模型绑定解析、本地模型构建
├── obsidian/
│   ├── mod.rs          # 子模块导出
│   ├── parser.rs       # Obsidian Markdown 解析（frontmatter、wikilink、inline tag）
│   ├── chunker.rs      # Smart Chunking 算法（断点评分、代码围栏保护、oversized 拆分）
│   ├── indexer.rs      # Vault 扫描、数据库 schema 初始化、笔记索引、embedding 写入、链接入库
│   ├── links.rs        # 链接发现（exact、alias、fuzzy、first-name 匹配）
│   ├── provider.rs     # ObsidianKnowledgeProvider 实现（五通道检索、融合、重排）
│   └── watcher.rs      # AutoIndexWatcher 文件监控与增量索引
└── retrieval/
    ├── mod.rs          # 子模块导出
    └── fusion.rs       # RankedHit、FusedHit、RRF/WRRF 融合算法
```

## 数据模型

### 数据库表

知识库保存在独立的 `knowledge.db`（路径 `~/.klaw/knowledge.db`），通过 `klaw-storage` 提供的 `open_default_knowledge_db` 打开。

#### knowledge_entries（笔记主表）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 笔记 ID（使用 vault 相对路径，如 `projects/rust-async.md`） |
| `title` | TEXT NOT NULL | 笔记标题（来自 frontmatter `title` 或首个 `#` 标题） |
| `uri` | TEXT NOT NULL UNIQUE | vault 相对路径（与 `id` 相同值，供外部引用） |
| `tags_json` | TEXT NOT NULL | 标签列表 JSON（来自 frontmatter `tags` 和 inline `#tag`） |
| `aliases_json` | TEXT NOT NULL | 别名列表 JSON（来自 frontmatter `aliases`） |
| `metadata_json` | TEXT NOT NULL | 结构化元数据 JSON（`inline_tags`、`wikilinks`、`note_date`） |
| `content` | TEXT NOT NULL | 笔记完整 Markdown 内容 |
| `note_date` | TEXT | 笔记日期（来自 frontmatter `date`，格式 `YYYY-MM-DD`） |
| `created_at_ms` | INTEGER NOT NULL | 创建时间（文件 mtime 毫秒 epoch） |
| `updated_at_ms` | INTEGER NOT NULL | 更新时间（文件 mtime 毫秒 epoch） |

#### knowledge_chunks（分块表）

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | TEXT PK | 分块 ID（格式 `{entry_id}::{chunk_index}`） |
| `entry_id` | TEXT NOT NULL | 关联 `knowledge_entries.id` |
| `heading` | TEXT | 分块所属标题（最近的上游 `#` 标题文本） |
| `content` | TEXT NOT NULL | 分块完整文本 |
| `snippet` | TEXT NOT NULL | 分块摘要（前 200 字符 + `...`） |
| `embedding` | BLOB | 向量嵌入（`f32` 数组的小端字节序 blob，或 Turso `F32_BLOB(N)`） |

#### knowledge_fts（FTS5 全文检索虚拟表 / 普通表回退）

当 FTS5 模块可用时，创建为虚拟表：

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
    chunk_id UNINDEXED,
    entry_id UNINDEXED,
    title,
    aliases,
    tags,
    content
);
```

当 FTS5 模块不可用时（如某些 sqlx SQLite 编译），回退为普通表：

```sql
CREATE TABLE IF NOT EXISTS knowledge_fts (
    chunk_id TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    title TEXT NOT NULL,
    aliases TEXT NOT NULL,
    tags TEXT NOT NULL,
    content TEXT NOT NULL
);
```

普通表模式下，FTS 通道使用 token 匹配评分而非 BM25。

#### knowledge_links（链接关系表）

| 字段 | 类型 | 说明 |
|------|------|------|
| `source_entry_id` | TEXT NOT NULL | 源笔记 ID |
| `target_title` | TEXT NOT NULL | 目标标题文本 |
| `target_entry_id` | TEXT | 目标笔记 ID（解析成功时填充，未解析时为 NULL） |
| `matched_text` | TEXT | 源文中被匹配的文本片段 |
| `match_type` | TEXT | 匹配类型（`exact_name`、`alias`、`fuzzy_name`、`first_name`） |
| `confidence_bp` | INTEGER | 匹配置信度（基点，0–1000，仅 fuzzy/first-name 有值） |

#### knowledge_metadata（元数据键值表）

| 字段 | 类型 | 说明 |
|------|------|------|
| `key` | TEXT PK | 元数据键名 |
| `value` | TEXT NOT NULL | 元数据值（文本形式） |

已知键：

| 键 | 说明 |
|----|------|
| `knowledge_embedding_dimensions` | embedding 向量维度数（如 `384`、`768`） |
| `knowledge_vector_index_enabled` | 向量索引是否已启用（`true` / `false`） |

#### 向量索引

```text
idx_knowledge_chunks_embedding — libsql_vector_idx(embedding)
```

需要 Turso/libSQL 后端 + embedding model 配置才能启用。创建时根据 embedding 维度自动推断 `F32_BLOB(N)` 类型。

### Rust 类型

#### KnowledgeHit

```rust
pub struct KnowledgeHit {
    pub id: String,          // 笔记 ID（vault 相对路径）
    pub title: String,       // 笔记标题
    pub excerpt: String,     // 摘要文本（受 max_excerpt_length 截断）
    pub score: f64,          // WRRF 融合分数
    pub tags: Vec<String>,   // 标签列表
    pub uri: String,         // vault 相对路径
    pub source: String,      // 来源 provider（如 "obsidian"）
    pub metadata: Value,     // serde_json::Value，含 lanes 等融合元信息
}
```

#### KnowledgeEntry

```rust
pub struct KnowledgeEntry {
    pub id: String,
    pub title: String,
    pub content: String,     // 笔记完整 Markdown 内容
    pub tags: Vec<String>,
    pub uri: String,
    pub source: String,
    pub metadata: Value,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
```

#### CreateKnowledgeNoteInput

```rust
pub struct CreateKnowledgeNoteInput {
    pub path: String,        // vault 内相对路径（如 "notes/new.md"）
    pub content: String,     // Markdown 内容
}
```

#### KnowledgeSourceInfo

```rust
pub struct KnowledgeSourceInfo {
    pub provider: String,    // provider 名称
    pub name: String,        // 来源显示名
    pub description: String, // 来源描述
    pub entry_count: usize,  // 笔记数量
}
```

#### KnowledgeStatus

```rust
pub struct KnowledgeStatus {
    pub enabled: bool,
    pub provider: String,
    pub source_name: String,
    pub vault_path: Option<String>,
    pub entry_count: usize,
    pub chunk_count: usize,
    pub embedded_chunk_count: usize,
    pub missing_embedding_count: usize,  // chunk_count - embedded_chunk_count
}
```

#### KnowledgeSyncResult

```rust
pub struct KnowledgeSyncResult {
    pub indexed_notes: usize,     // 本次索引的笔记数
    pub embedded_chunks: usize,   // 本次嵌入的分块数
    pub status: KnowledgeStatus,  // 同步后的状态快照
}
```

#### KnowledgeSyncProgress / KnowledgeSyncProgressStage

```rust
pub enum KnowledgeSyncProgressStage {
    IndexingNotes,   // 正在索引笔记
    EmbeddingChunks, // 正在嵌入分块
}

pub struct KnowledgeSyncProgress {
    pub stage: KnowledgeSyncProgressStage,
    pub completed: usize,
    pub total: Option<usize>,
    pub current_item: Option<String>,  // 当前处理的文件名
}
```

#### KnowledgeSearchQuery

```rust
pub struct KnowledgeSearchQuery {
    pub text: String,               // 查询文本
    pub tags: Option<Vec<String>>,   // 标签过滤
    pub source: Option<String>,      // 来源过滤
    pub limit: usize,                // 返回上限（默认 5）
    pub mode: Option<String>,        // 搜索模式提示
}
```

#### KnowledgeRuntimeState / KnowledgeRuntimeSnapshot

```rust
pub enum KnowledgeRuntimeState {
    Disabled,     // 知识功能未启用
    Unconfigured, // 未配置 vault 路径
    Loading,      // 正在加载 provider
    Ready,        // 就绪，可检索
    Syncing,      // 正在同步索引
    Error,        // 加载或运行出错
}

pub struct KnowledgeRuntimeSnapshot {
    pub state: KnowledgeRuntimeState,
    pub status: Option<KnowledgeStatus>,
    pub error: Option<String>,
}
```

## 核心 trait

### KnowledgeProvider

```rust
#[async_trait]
pub trait KnowledgeProvider: Send + Sync {
    fn provider_name(&self) -> &str;

    async fn search(
        &self,
        query: KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeHit>, KnowledgeError>;

    async fn get(&self, id: &str) -> Result<Option<KnowledgeEntry>, KnowledgeError>;

    async fn list_sources(&self) -> Result<Vec<KnowledgeSourceInfo>, KnowledgeError>;

    async fn create_note(
        &self,
        input: CreateKnowledgeNoteInput,
    ) -> Result<KnowledgeEntry, KnowledgeError>;
}
```

设计考量：

- `search` 内部执行五通道并行检索 + WRRF 融合 + rerank，返回去重排序后的 `KnowledgeHit` 列表
- `get` 通过 `id OR uri` 双路径查找，支持按路径或 ID 获取完整笔记
- `create_note` 在 vault 中创建新 Markdown 文件并即时索引（含分块、FTS、链接）
- `list_sources` 返回当前 provider 下所有可检索的来源信息

### EmbeddingModel

```rust
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, KnowledgeError>;
}
```

### RerankModel

```rust
#[async_trait]
pub trait RerankModel: Send + Sync {
    async fn rerank(&self, query: &str, candidates: &[String]) -> Result<Vec<f32>, KnowledgeError>;
}
```

### OrchestratorModel

```rust
#[async_trait]
pub trait OrchestratorModel: Send + Sync {
    async fn orchestrate(&self, query: &str) -> Result<KnowledgeOrchestration, KnowledgeError>;
}
```

### KnowledgeOrchestration

```rust
pub struct KnowledgeOrchestration {
    pub intent: QueryIntent,       // 查询意图分类
    pub expansions: Vec<String>,   // 查询扩展词列表
}
```

`QueryIntent` 由 `klaw-model` 定义，包含以下枚举值：

| Intent | 说明 | 适用场景 |
|--------|------|---------|
| `Temporal` | 时间相关 | 查询包含日期模式 |
| `Relationship` | 关系探索 | 查询关注实体间关联 |
| `Exact` | 精确匹配 | 查询包含明确关键词 |
| `Conceptual` | 概念理解 | 查询需要语义扩展 |
| `Exploratory` | 探索性搜索 | 默认意图，宽泛召回 |

### KnowledgeAutoIndexHandle

```rust
#[async_trait]
pub trait KnowledgeAutoIndexHandle: Send + Sync {
    async fn stop(self: Box<Self>);
}
```

用于停止自动索引 watcher 的生命周期管理接口。

## 模型绑定与构建

### KnowledgeModelBindings

```rust
pub struct KnowledgeModelBindings {
    pub embedding_model_id: Option<String>,
    pub orchestrator_model_id: Option<String>,
    pub reranker_model_id: Option<String>,
}
```

模型绑定优先级：

| 模型类型 | 优先级 1（knowledge 专用） | 优先级 2（全局默认） |
|----------|--------------------------|---------------------|
| embedding | `knowledge.models.embedding_model_id` | `models.default_embedding_model_id` |
| orchestrator | `knowledge.models.orchestrator_model_id` | 无全局默认 |
| reranker | `knowledge.models.reranker_model_id` | `models.default_reranker_model_id` |

```rust
pub fn resolve_model_bindings(config: &AppConfig) -> KnowledgeModelBindings
```

knowledge 专用配置优先，若未配置则回退到全局默认值。orchestrator 无全局默认，未配置时使用 `HeuristicOrchestrator`。

### 本地模型构建

```rust
pub fn build_local_embedding_model(config: &AppConfig)
    -> Result<Option<ModelBackedEmbedding>, KnowledgeError>

pub fn build_local_reranker(config: &AppConfig)
    -> Result<Option<ModelBackedReranker>, KnowledgeError>

pub fn build_local_orchestrator(config: &AppConfig)
    -> Result<Option<ModelBackedOrchestrator>, KnowledgeError>
```

构建流程：

1. 通过 `resolve_model_bindings` 获取模型 ID
2. 若模型 ID 为 `None`，返回 `Ok(None)`（该通道静默禁用）
3. 通过 `ModelService::open_default` 打开模型存储
4. 创建 `ModelLlamaRuntime`（基于 `LlamaCppRsBackend`）
5. 包装为对应的 `ModelBacked*` 结构体

### HeuristicOrchestrator（启发式回退）

```rust
pub struct HeuristicOrchestrator;

#[async_trait]
impl OrchestratorModel for HeuristicOrchestrator {
    async fn orchestrate(&self, query: &str) -> Result<KnowledgeOrchestration, KnowledgeError> {
        Ok(KnowledgeOrchestration {
            intent: QueryIntent::Exploratory,
            expansions: vec![query.to_string()],
        })
    }
}
```

当 orchestrator 模型未配置或加载失败时，回退到启发式策略：意图默认为 `Exploratory`，扩展词仅包含原始查询。

## 索引管线

索引管线由 `index_vault_with_progress` 函数驱动，完整流程如下：

```
Vault 扫描（collect_markdown_files）
  │
  ├─ 1. 初始化数据库 schema（init_schema）
  ├─ 2. 构建名称索引（build_vault_name_index）— 用于链接发现
  ├─ 3. 清理已删除文件（remove_missing_entries）
  │
  └─ 对每个 Markdown 文件：
      │
      ├─ 4. 检查是否需要更新（is_entry_up_to_date — 比较 mtime）
      ├─ 5. Note 解析（parse_note）
      │     ├─ frontmatter 分割（title, tags, aliases, date）
      │     ├─ wikilink 提取（[[target]]）
      │     └─ inline tag 提取（#tag）
      ├─ 6. Smart Chunking（chunk_markdown）
      │     ├─ 断点评分（find_break_points）
      │     ├─ 加权分块（smart_chunk）
      │     └─ 过大分块拆分（split_oversized_chunks）
      ├─ 7. 写入 knowledge_entries 主表
      ├─ 8. 写入 knowledge_chunks 分块表（含即时 embedding）
      ├─ 9. 写入 knowledge_fts（FTS5 或普通表）
      ├─ 10. 链接发现（discover_links）与入库
      │
      └─ 返回索引计数 + 进度回调
```

### Vault 扫描

`collect_markdown_files` 使用 `ignore::WalkBuilder` 递归遍历 vault 目录，收集所有 `.md` 文件，排除 `exclude_folders` 中指定的目录（如 `.obsidian`、`templates`）。

### Note 解析

`parse_note` 负责解析 Obsidian Markdown 笔记的结构化信息：

```rust
pub struct ParsedNote {
    pub title: Option<String>,      // frontmatter title 或首个 # 标题
    pub tags: Vec<String>,          // frontmatter tags 列表
    pub aliases: Vec<String>,       // frontmatter aliases 列表
    pub wikilinks: Vec<String>,     // [[target]] 链接目标列表
    pub inline_tags: Vec<String>,   # 正文中的 #tag 标签
    pub note_date: Option<String>,  // frontmatter date（YYYY-MM-DD 格式校验）
    pub body: String,               // frontmatter 之后的正文
}
```

解析规则：

- frontmatter 使用 `---` 分隔符识别，支持 YAML 列表（`[a, b]` 和 `- item` 格式）
- `title` 优先取 frontmatter `title` 字段，其次取正文首个 `# ` 标题
- `date` 严格校验 `YYYY-MM-DD` 格式（使用 `time` crate 解析），无效日期被过滤
- `wikilinks` 提取 `[[target]]`、`[[target#section]]`、`[[target|display]]` 三种格式，只保留 target 部分
- `inline_tags` 提取 `#tag` 格式（支持 `/` 和 `-` 分隔的多级标签），排除 wikilink 中的 `#section`

### Smart Chunking

Smart Chunking 算法将长 Markdown 文本拆分为语义连贯的分块，核心流程：

**第一阶段：断点评分（find_break_points）**

对每行计算语义断点分数：

| 行类型 | 分数 | 说明 |
|--------|------|------|
| `# 标题` | 100 | 一级标题，最强断点 |
| `## 标题` | 90 | 二级标题 |
| `### 标题` | 80 | 三级标题 / 代码围栏边界 |
| `#### 标题` | 70 | 四级标题 |
| `##### 标题` | 60 | 五级标题 / thematic break |
| `###### 标题` | 50 | 六级标题 |
| thematic break (`---`, `***`) | 60 | 水平分隔线 |
| 空行 | 20 | 段落间空白 |
| 列表项 (`- `, `* `, `1.`) | 5 | 列表项边界 |
| 代码围栏内行 | 1 | 代码围栏内部，极低分数（避免切割） |
| 普通文本 | 1 | 不作为断点 |

代码围栏（` ``` ` 或 `~~~`）的边界行分数为 80，内部行分数为 1 且标记 `inside_code_fence = true`，确保分块不会在代码块内部切割。

```rust
pub struct BreakPoint {
    pub byte_offset: usize,
    pub line_number: usize,
    pub score: u32,
    pub inside_code_fence: bool,
}
```

**第二阶段：加权分块（smart_chunk）**

```rust
pub fn smart_chunk(
    content: &str,
    target_tokens: usize,   // 目标分块 token 数（默认 512）
    overlap_pct: usize,     // 重叠百分比（默认 15）
) -> Vec<Chunk>
```

算法流程：

1. 估算总 token 数（`len / 4`），若不超目标则整体作为一个分块
2. 计算理想切割位置 `ideal_end = start + target_chars`
3. 在 `[start, start + 2*target_chars]` 范围内寻找非代码围栏内的最佳断点
4. 加权评分公式：`weighted_score = score * 1/(1 + |offset - ideal|/500)` — 优先选择靠近目标长度的高分断点
5. 无合适断点时回退到换行符切割（`fallback_cut_offset`）
6. 分块间保留 `overlap_pct` 百比的字符重叠，且重叠位置不在代码围栏内

**第三阶段：过大分块拆分（split_oversized_chunks）**

```rust
pub fn split_oversized_chunks(
    chunks: Vec<Chunk>,
    token_count: &dyn Fn(&str) -> usize,
    max_tokens: usize,
    overlap_tokens: usize,
) -> Vec<Chunk>
```

对超过 `max_tokens` 的分块按句子边界（`\n` 和 `. `）二次拆分：

- 拆分后的后续分块标题添加 `(cont.)` 后缀
- 各子分块间保留 `overlap_tokens` 的词级重叠
- 保留原始 heading 语义连贯性

### Embedding 写入

`insert_chunk` 在写入分块时同步计算 embedding：

1. 若 embedder 可用，即时调用 `embed(chunk.text)` 获取向量
2. 将 `Vec<f32>` 序列化为小端字节序 blob（`serialize_embedding`）
3. 写入 `knowledge_chunks.embedding` 列
4. 若 embedder 不可用，`embedding` 列为 NULL，后续可通过 `embed_missing_chunks` 补填

### FTS5 索引

每个分块同步写入 `knowledge_fts` 表，包含 `chunk_id`、`entry_id`、`title`、`aliases`、`tags`、`content` 六个字段。FTS5 虚拟表模式下这些字段参与全文检索（`UNINDEXED` 字段不参与）；普通表模式下仅作为数据存储，检索时使用 token 匹配。

### 链接发现与图谱构建

`discover_links` 在索引时自动发现笔记间的隐式和显式链接关系：

| 匹配类型 | 说明 | 置信度 |
|----------|------|--------|
| `ExactName` | 文本中出现其他笔记的标题或文件名 | 高（无量化） |
| `Alias` | 文本中出现其他笔记的别名 | 高（无量化） |
| `FuzzyName` | 文本与目标标题的 normalized Levenshtein 相似度 ≥ 0.92 | 920 bp |
| `FirstName` | 文本中出现人名首名（仅当姓氏唯一时匹配） | 650 bp |

链接发现流程：

1. `build_name_index` 从所有笔记的路径 basename、title、aliases 构建名称索引
2. `discover_links` 扫描笔记正文（排除已有 `[[wikilink]]` 和保护区域）
3. 精确匹配优先，模糊匹配需超过阈值，人名首名需姓氏无歧义
4. 结果写入 `knowledge_links` 表，`target_entry_id` 在目标笔记已索引时填充

```rust
pub struct DiscoveredLink {
    pub matched_text: String,     // 源文中被匹配的文本
    pub target_path: String,      // 目标笔记路径
    pub target_title: String,     // 目标笔记标题
    pub display: Option<String>,  // 显示文本
    pub match_type: LinkMatchType,
}
```

## 搜索管线

搜索由 `KnowledgeProvider::search` 驱动，采用五通道并行检索 → WRRF 融合 → rerank 的三阶段流程：

```
输入 KnowledgeSearchQuery
  │
  ├─ Phase 1: 查询编排
  │   ├─ orchestrate(query) → intent + expansions
  │   ├─ 若 orchestrator 未配置 → HeuristicOrchestrator 回退
  │   └─ expansions 去重，确保原始查询在首位
  │
  ├─ Phase 2: 五通道并行检索
  │   ├─ Lane 1: Semantic — 对每个 expansion 做 embedding 检索
  │   ├─ Lane 2: FTS — 对每个 expansion 做 BM25/token 检索
  │   ├─ Lane 3: Graph — 以 Lane 1+2 的 seed hits 为起点做链接遍历
  │   ├─ Lane 4: Temporal — 日期模式匹配
  │   └─ Lane 5: Rerank — 对 Phase 2 融合结果做 Yes/No softmax 重排
  │
  ├─ Phase 2.5: Pass 1 融合（4 通道，不含 rerank）
  │   ├─ WRRF(semantic, fts, graph, temporal) → pass1
  │   └─ pass1 作为 rerank 输入
  │
  ├─ Phase 3: Pass 2 融合（5 通道，含 rerank）
  │   ├─ WRRF(semantic, fts, graph, temporal, rerank) → fused_hits
  │   └─ 裁剪到 limit 条
  │
  └─ Phase 4: 元数据回填
      ├─ load_entry_metadata — 批量加载 uri, tags_json, metadata_json
      └─ 合并 lanes 信息到 metadata
      └─ 返回 Vec<KnowledgeHit>
```

### Lane 1: Semantic（语义检索）

语义检索采用三级降级策略：

**Level 1: vector_top_k（Turso/libSQL native vector index）**

```sql
SELECT e.id, e.title, c.snippet, v.distance
FROM vector_top_k('idx_knowledge_chunks_embedding', ?1, ?2) v
JOIN knowledge_chunks c ON c.rowid = v.id
JOIN knowledge_entries e ON e.id = c.entry_id
ORDER BY v.distance ASC
```

- 使用 libSQL 的 `vector_top_k` 虚拟表函数，直接利用向量索引
- 性能最优，O(log N) 复杂度
- 仅在 Turso 后端 + vector 索引已创建时可用

**Level 2: vector_distance_cos SQL 函数**

```sql
SELECT e.id, e.title, c.snippet, vector_distance_cos(c.embedding, ?1) AS distance
FROM knowledge_chunks c
JOIN knowledge_entries e ON e.id = c.entry_id
WHERE c.embedding IS NOT NULL
ORDER BY distance ASC
LIMIT ?2
```

- 使用 libSQL 的 `vector_distance_cos` 内置函数
- O(N) 复杂度，但无需额外索引结构
- 在 Turso 后端但 vector 索引不可用时自动启用

**Level 3: Rust cosine fallback**

```rust
// 从数据库读取所有有 embedding 的分块
// 在 Rust 中计算 cosine_similarity(query_vector, candidate)
// 排序并截断到 limit
```

- 纯 Rust 计算，无需 SQL vector 函数支持
- O(N) 复杂度，内存中计算
- 在 sqlx 后端或 Turso vector 函数不可用时自动启用
- 保证语义检索在所有后端始终可用（只要有 embedding 数据）

降级逻辑：

```
尝试 vector_top_k
  │ 失败且为 vector capability error → set_vector_index_enabled(false)
  │ 其他错误 → 直接返回错误
  ↓ 成功 → 返回结果

尝试 vector_distance_cos
  │ 失败且为 vector capability error → 继续降级
  │ 其他错误 → 直接返回错误
  ↓ 成功 → 返回结果

Rust cosine fallback
  │ 必然成功（只要有 embedding 数据）
  ↓ 返回结果
```

若 embedder 未配置，语义通道直接返回空列表，不影响其他通道。

### Lane 2: FTS（全文检索）

**FTS5 模式（fts_virtual = true）**

```sql
SELECT e.id, e.title, c.snippet, bm25(knowledge_fts)
FROM knowledge_fts
JOIN knowledge_entries e ON e.id = knowledge_fts.entry_id
JOIN knowledge_chunks c ON c.id = knowledge_fts.chunk_id
WHERE knowledge_fts MATCH ?1
ORDER BY bm25(knowledge_fts) ASC
LIMIT ?2
```

- 使用 FTS5 `MATCH` 查询 + `bm25()` 排名函数
- 分数转换：`1.0 / (|bm25_score| + 1.0)`，将负值 BM25 分数转为正分数

**普通表模式（fts_virtual = false）**

```sql
SELECT e.id, e.title, c.snippet
FROM knowledge_fts f
JOIN knowledge_entries e ON e.id = f.entry_id
JOIN knowledge_chunks c ON c.id = f.chunk_id
LIMIT ?1
```

- 加载较宽候选池（`limit * 20`）
- 在 Rust 中对 title + excerpt 做 token 匹配评分
- `tokenize_query` 将查询拆分为小写 token
- 分数 = 匹配 token 数量

### Lane 3: Graph（图谱遍历）

以 semantic + FTS 通道的融合 seed hits 为起点，遍历 `knowledge_links` 表：

```sql
SELECT DISTINCT target.id, target.title, substr(target.content, 1, 400)
FROM knowledge_links link
JOIN knowledge_entries target ON (
    target.id = link.target_entry_id
    OR (link.target_entry_id IS NULL AND lower(target.title) = lower(link.target_title))
)
WHERE link.source_entry_id = ?1 AND target.id != link.source_entry_id
LIMIT ?2
```

- 对每个 seed hit 查询其出边链接指向的目标笔记
- 支持 `target_entry_id` 直接匹配和 `target_title` 模糊匹配（当目标尚未索引时）
- 排除自引用（`target.id != link.source_entry_id`）
- 默认分数 0.5

### Lane 4: Temporal（时间检索）

从查询文本中提取日期模式，匹配 `knowledge_entries.note_date`：

```sql
SELECT id, title, substr(content, 1, 400)
FROM knowledge_entries
WHERE note_date LIKE ?1
ORDER BY updated_at_ms DESC
LIMIT ?2
```

日期模式提取规则（`temporal_pattern`）：

- 支持 `YYYY`、`YYYY-MM`、`YYYY-MM-DD` 三种模式
- 生成 `LIKE` 模式：`2024%`、`2024-06%`、`2024-06-15`
- 仅匹配 ASCII 数字和连字符组成的子串
- 非日期查询返回空列表（该通道静默跳过）

### Lane 5: Rerank（重排）

对 Pass 1 融合结果做 Yes/No softmax rerank：

```rust
async fn rerank_lane(
    &self,
    query: &str,
    fused_hits: &[FusedHit],
    limit: usize,
) -> Result<Vec<RankedHit>, KnowledgeError>
```

流程：

1. 提取 fused_hits 的 excerpt 作为候选文本
2. 调用 `RerankModel::rerank(query, candidates)` 获取 softmax 分数
3. 按 score DESC 排序返回

若 reranker 未配置，该通道返回空列表，不影响其他通道分数。

## 融合算法

### Weighted Reciprocal Rank Fusion（WRRF）

```rust
pub fn weighted_reciprocal_rank_fuse(
    lanes: &[(&str, &[RankedHit], f64)],  // (通道名, 命中列表, 权重)
    k: usize,                              // RRF 常数（默认 60）
) -> Vec<FusedHit>
```

核心公式：

```
WRRF_score(id) = Σ_{lane} weight_lane / (k + rank_lane(id) + 1)
```

- `k = 60`（经验常数，与 memory 模块一致）
- 同一命中在多个通道出现时，各通道贡献累加
- 跨通道命中的分数天然高于单通道命中
- 最终按 `score DESC, title ASC` 排序

```rust
pub struct FusedHit {
    pub id: String,
    pub title: String,
    pub score: f64,
    pub excerpt: String,
    pub lanes: Vec<String>,   // 出命中的通道名列表
}
```

### 通道权重表

权重由查询意图决定（`lane_weights_for_intent`）：

| Intent | semantic | fts | graph | temporal | rerank |
|--------|----------|-----|-------|----------|--------|
| `Temporal` | 1.1 | 1.0 | 0.8 | **1.6** | 4.0 |
| `Relationship` | 0.9 | 0.9 | **1.6** | 0.5 | 4.0 |
| `Exact` | 0.8 | **1.6** | 0.8 | 0.5 | 4.0 |
| `Conceptual` | **1.5** | 0.9 | 1.1 | 0.5 | 4.0 |
| `Exploratory` | 1.4 | 1.1 | 0.9 | 0.5 | 4.0 |

设计考量：

- rerank 权重始终为 4.0（最高），因为 rerank 是对融合结果的精排，应具有决定性影响
- 各意图突出其最相关通道：时间意图突出 temporal、关系意图突出 graph、精确意图突出 fts、概念意图突出 semantic
- 权重为乘法系数，直接影响 WRRF 分数中各通道的贡献比例

两阶段融合：

1. **Pass 1**：4 通道融合（semantic + fts + graph + temporal），产出 seed hits 供 rerank
2. **Pass 2**：5 通道融合（加入 rerank），最终排序

## Orchestrator（查询编排）

### 编排流程

```rust
async fn orchestration_plan(&self, query: &str) -> Result<KnowledgeOrchestration, KnowledgeError>
```

1. 若 orchestrator 模型已配置，调用 `orchestrate(query)` 获取 intent + expansions
2. 若 orchestrator 未配置，回退到 `HeuristicOrchestrator`（intent = Exploratory，expansions = [query]）
3. 确保 expansions 非空（空时补入原始查询）
4. 确保原始查询在 expansions 首位（若不在则插入）
5. expansions 去重

### 模型驱动 vs 启发式回退

| 策略 | intent 来源 | expansions 来源 | 适用条件 |
|------|------------|----------------|---------|
| 模型驱动 | `LocalOrchestratorRuntime` LLM 推理 | LLM 生成同义词/相关词 | orchestrator model 已配置且可用 |
| 启发式回退 | 固定 `Exploratory` | 仅包含原始查询 | orchestrator 未配置或加载失败 |

模型驱动的 orchestrator 通过 `ModelOrchestrateRequest` 请求 LLM 分析查询意图并生成扩展词，提供更精准的意图分类和更丰富的检索扩展。

## 降级与容错

各通道的降级策略确保知识检索在不同后端和配置下始终可用：

| 通道 | 降级层级 | 条件 | 结果 |
|------|---------|------|------|
| Semantic | Level 1 → 2 → 3 | vector_top_k 失败 → vector_distance_cos 失败 → Rust fallback | 语义检索始终可用（有 embedding 时） |
| Semantic | 完全跳过 | embedder 未配置 | 返回空列表，不影响其他通道 |
| FTS | FTS5 → token | FTS5 模块不可用 | 退回普通表 + token 匹配评分 |
| Graph | 无降级 | 链接数据缺失 | 返回空列表 |
| Temporal | 无降级 | 查询不含日期模式 | 返回空列表 |
| Rerank | 完全跳过 | reranker 未配置 | 返回空列表，4 通道融合仍然有效 |
| Orchestrator | 启发式回退 | orchestrator 未配置 | intent = Exploratory，expansions = [query] |

向量索引状态管理：

- `vector_index_enabled` 存储在 `Arc<Mutex<bool>>` 中，运行时动态更新
- 首次语义检索时若索引未创建，尝试 `ensure_vector_index` 创建
- 若 `vector_top_k` 查询返回 capability error，自动标记 `vector_index_enabled = false`
- 后续语义检索直接跳过 Level 1，从 Level 2 开始
- `reindex` 和 `embed_missing_chunks` 完成后刷新 `vector_index_enabled` 状态

## Context Bundle 组装

`assemble_context_bundle` 将检索结果组装为受字符预算控制的上下文文本，用于 prompt 注入：

```rust
pub fn assemble_context_bundle(
    topic: &str,            // 查询主题
    hits: &[KnowledgeHit],  // 检索命中列表
    budget_chars: usize,    // 字符预算上限
) -> ContextBundle
```

```rust
pub struct ContextBundle {
    pub topic: String,
    pub sections: Vec<ContextSection>,
    pub total_chars: usize,
    pub budget_chars: usize,
    pub truncated: bool,       // 是否因预算不足而截断
}

pub struct ContextSection {
    pub label: String,         // 如 "Direct match"
    pub title: String,         // 笔记标题
    pub uri: String,           // vault 相对路径
    pub content: String,       // 摘要内容（可能截断）
    pub relevance: String,     // 如 "score 0.85"
}
```

组装流程：

1. 对每个 hit，计算剩余可用字符 `available = budget - total_chars - SECTION_OVERHEAD(80)`
2. 若可用空间为 0，标记 `truncated = true` 并停止
3. 若 excerpt 超过可用空间，截断到 `available - 14` 字符并追加 `... [truncated]`
4. 每个 section 包含 80 字符固定开销（标签、标题、relevance 等格式文本）
5. 按 hit 顺序依次填充，直到预算耗尽

设计考量：

- `SECTION_OVERHEAD = 80` 为每个 section 的格式化开销预留空间
- 截断标记 `[truncated]` 明确告知模型内容被裁剪
- `budget_chars` 最小值为 1，防止零预算导致异常

## Runtime 生命周期

Knowledge 功能在 runtime 中通过以下入口管理：

### open_configured_obsidian_provider

```rust
pub async fn open_configured_obsidian_provider(
    config: &AppConfig,
) -> Result<ObsidianKnowledgeProvider, KnowledgeError>
```

流程：

1. 校验 `knowledge.provider == "obsidian"`，否则返回 `InvalidConfig`
2. 校验 `knowledge.obsidian.vault_path` 非空且路径存在
3. 打开 `knowledge.db`（`open_default_knowledge_db`）
4. 创建 `ObsidianKnowledgeProvider::open`（含 schema 初始化、FTS/vector 检测）
5. 可选附加 embedding model（`with_embedding_model`）
6. 可选附加 reranker（`with_reranker`）
7. 可选附加 orchestrator（`with_orchestrator`）

### start_configured / load_provider

runtime 启动时根据 `knowledge.enabled` 决定是否加载知识 provider：

- `enabled = false` → `KnowledgeRuntimeState::Disabled`
- `enabled = true` 但 `vault_path` 未配置 → `KnowledgeRuntimeState::Unconfigured`
- `enabled = true` 且路径有效 → `KnowledgeRuntimeState::Loading` → `open_configured_obsidian_provider` → `Ready` 或 `Error`

### auto_index_watcher

`start_auto_index_watcher` 创建文件系统监控 watcher：

```rust
pub fn start_auto_index_watcher(
    provider: ObsidianKnowledgeProvider,
) -> Result<AutoIndexWatcher, KnowledgeError>
```

- 使用 `notify_debouncer_full` 监控 vault 目录变更（2 秒去抖）
- 过滤 `.md` 文件，排除 `exclude_folders`
- 映射文件系统事件到 `WatchEvent`（Changed / Deleted / Moved / FullRescan）
- 启动时先执行 `reconcile_existing_index`（清理已删除文件）
- 变更事件触发增量索引（单文件 `index_path` 或全量 `reconcile_existing_index`）
- 通过 `oneshot::Sender` 发送停止信号，producer 线程和 consumer task 并行关闭

```rust
pub enum WatchEvent {
    Changed(PathBuf),           // 文件创建或修改
    Deleted(PathBuf),           // 文件删除
    Moved { from: PathBuf, to: PathBuf }, // 文件重命名
    FullRescan,                 // 需要全量重新索引
}
```

### reload

runtime 支持知识 provider 的热重载：当配置变更时，停止当前 watcher，重新 `open_configured_obsidian_provider`，重新启动 watcher。

### configured_knowledge_status

```rust
pub async fn configured_knowledge_status(
    config: &AppConfig,
) -> Result<KnowledgeStatus, KnowledgeError>
```

- 未配置 vault 路径时返回零计数状态（不尝试打开数据库）
- vault 路径不存在时返回零计数状态
- 正常情况下打开 provider 并查询 `status`

### sync_configured_knowledge_with_progress

```rust
pub async fn sync_configured_knowledge_with_progress<F>(
    config: &AppConfig,
    progress: F,
) -> Result<KnowledgeSyncResult, KnowledgeError>
```

- 打开 provider → `reindex_with_progress` → `embed_missing_chunks_with_progress` → `status`
- 进度回调接收 `KnowledgeSyncProgress`，可用于 GUI 进度条

## 配置体系

### knowledge 配置节

```toml
[knowledge]
enabled = true
provider = "obsidian"

[knowledge.obsidian]
vault_path = "/path/to/obsidian-vault"
exclude_folders = [".obsidian", "templates"]
max_excerpt_length = 400

[knowledge.models]
embedding_model_id = "text-embedding-3-small"   # 可选，回退到 models.default_embedding_model_id
orchestrator_model_id = "local-orchestrator"     # 可选，无全局默认
reranker_model_id = "local-reranker"             # 可选，回退到 models.default_reranker_model_id
```

### 校验约束

| 约束 | 条件 | 错误类型 |
|------|------|---------|
| provider 唯一支持值 | `provider == "obsidian"` | `InvalidConfig` |
| vault_path 必需 | `enabled = true` 时非空 | `InvalidConfig` |
| vault_path 存在 | 路径在文件系统上存在 | `KnowledgeStatus` 零计数 |
| exclude_folders 格式 | 字符串列表，不含前后 `/` | 内部 trim 处理 |
| max_excerpt_length | 正整数 | 默认 400 |
| embedding_model_id | 优先 knowledge 专用，回退全局 | `resolve_model_bindings` |

### 模型绑定优先级详解

```rust
pub fn resolve_model_bindings(config: &AppConfig) -> KnowledgeModelBindings {
    KnowledgeModelBindings {
        embedding_model_id: config.knowledge.models.embedding_model_id
            .clone()
            .or_else(|| config.models.default_embedding_model_id.clone()),
        orchestrator_model_id: config.knowledge.models.orchestrator_model_id.clone(),
        reranker_model_id: config.knowledge.models.reranker_model_id
            .clone()
            .or_else(|| config.models.default_reranker_model_id.clone()),
    }
}
```

- `embedding_model_id`：knowledge 专用优先 → 全局 `default_embedding_model_id` 回退
- `orchestrator_model_id`：knowledge 专用优先 → 无回退（未配置时使用 HeuristicOrchestrator）
- `reranker_model_id`：knowledge 专用优先 → 全局 `default_reranker_model_id` 回退

## KnowledgeProviderRouter

`KnowledgeProviderRouter` 支持多 provider 路由，将操作分发到指定名称的 provider：

```rust
pub struct KnowledgeProviderRouter {
    providers: BTreeMap<String, Arc<dyn KnowledgeProvider>>,
}
```

核心方法：

| 方法 | 说明 |
|------|------|
| `register(provider)` | 按 `provider_name()` 注册 provider |
| `get(provider_name)` | 获取指定 provider 的 `Arc` 引用 |
| `search(provider_name, query)` | 路由搜索请求到指定 provider |
| `get_entry(provider_name, id)` | 路由获取请求到指定 provider |
| `list_sources(provider_name)` | 路由来源列表请求到指定 provider |
| `create_note(provider_name, input)` | 路由笔记创建请求到指定 provider |

未知 provider 名返回 `KnowledgeError::SourceUnavailable`。

## 与 runtime 的集成

### 工具层

`knowledge_search` tool（`klaw-tool`）是模型侧的知识检索入口：

- 调用 `KnowledgeProvider::search` 执行五通道融合检索
- 返回 `KnowledgeHit` 列表供模型参考
- `knowledge_create_note` tool 调用 `KnowledgeProvider::create_note` 创建新笔记

### Prompt 注入

runtime 在构建上下文时：

1. 根据 `knowledge.enabled` 判断是否注入知识上下文
2. 对模型生成的查询调用 `KnowledgeProvider::search`
3. 通过 `assemble_context_bundle` 将检索结果组装为受预算控制的文本
4. 拼入 system prompt 的 `Knowledge` 章节

知识上下文不通过工具主动检索给模型，而是由 runtime 在需要时自动注入，确保上下文受预算控制且不膨胀。

### 统计面板

GUI Knowledge 面板通过 `configured_knowledge_status` 获取展示数据：

- 总览统计：笔记数、分块数、已嵌入数、缺失嵌入数
- 来源信息：provider 名称、vault 路径
- 同步进度：通过 `KnowledgeSyncProgress` 回调实时展示

## 后端策略

| 后端 | feature | 检索能力 | 适用场景 |
|------|---------|---------|---------|
| Turso（libSQL） | `turso` | FTS5(BM25) + vector_top_k + vector_distance_cos + Rust fallback | 需要 embedding 或远程 Turso |
| sqlx（标准 SQLite） | `sqlx` | FTS5(BM25) 或 token 回退 + Rust cosine fallback | 本地单进程，无 native vector |

后端选择逻辑：

- `open_default_knowledge_db` 根据编译 feature 选择后端驱动
- `init_schema` 自动检测 FTS5 可用性：创建虚拟表失败时回退为普通表
- `ensure_vector_index` 尝试创建 libSQL vector 索引：失败时标记不可用，语义检索降级
- sqlx 后端下语义检索仅使用 Rust cosine fallback（Level 3）
- 所有后端保证基础检索能力（FTS 或 token + graph + temporal）始终可用

FTS5 可用性检测：

```rust
async fn detect_virtual_fts(db: &Arc<dyn DatabaseExecutor>) -> Result<bool, KnowledgeError>
```

通过查询 `sqlite_master` 检测 `knowledge_fts` 是否为 FTS5 虚拟表（`type = 'virtual'` 且 `name = 'knowledge_fts'`）。

Vector 索引可用性检测：

```rust
async fn has_vector_index(db: &Arc<dyn DatabaseExecutor>) -> Result<bool, KnowledgeError>
```

通过 `knowledge_metadata` 表读取 `knowledge_vector_index_enabled` 值，结合 `sqlite_master` 检测索引是否存在。

## 错误处理

```rust
pub enum KnowledgeError {
    InvalidConfig(String),      // 配置校验失败（如 provider 不支持、vault_path 缺失）
    InvalidQuery(String),       // 查询参数校验失败（如空查询文本）
    InvalidNotePath(String),    // 笔记路径校验失败（如路径遍历、非法字符）
    NoteAlreadyExists(String),  // 创建笔记时路径已存在
    Provider(String),           // provider 内部错误（存储、模型、IO 等）
    SourceUnavailable(String),  // 请求的 provider 未注册
}
```

设计考量：

- 使用 `thiserror` 拒绝 `unwrap()`，与 workspace lints 保持一致
- `Provider` 是最通用的错误类型，涵盖存储、模型、IO 等底层错误
- `SourceUnavailable` 区分"provider 不存在"和"provider 内部错误"
- `InvalidNotePath` 和 `NoteAlreadyExists` 专门服务于 `create_note` 流程
- 语义检索降级时不报错，仅在 vector capability error 时自动切换策略

## 设计考量总结

1. **独立数据库**：`knowledge.db` 与 `klaw.db`、`memory.db` 分离，避免知识索引操作影响其他存储性能
2. **三级语义降级**：vector_top_k → vector_distance_cos → Rust cosine，保证语义检索在所有后端可用
3. **意图感知权重**：不同查询意图动态调整通道权重，避免一刀切的检索策略
4. **两阶段融合**：Pass 1 产出 seed hits 供 rerank，Pass 2 加入 rerank 结果，确保 rerank 对最终排序有决定性影响
5. **Smart Chunking 代码保护**：断点评分将代码围栏内行标记为低分，避免在代码块内部切割
6. **链接图谱**：自动发现四种匹配类型的链接关系，支持图遍历检索和关系探索
7. **FTS5 自动回退**：创建虚拟表失败时自动回退为普通表 + token 评分，保证全文检索始终可用
8. **增量索引**：文件 watcher 监控 vault 变更，mtime 比较避免重复索引，增量删除清理失效条目
9. **Budget 控制**：`assemble_context_bundle` 使用字符预算 + section 开销预留，防止 prompt 膨胀
10. **模型绑定回退**：knowledge 专用配置优先，全局默认兜底，灵活且不冗余

## 测试覆盖

### indexer 集成测试

- embedding 索引初始化 native Turso vector schema
- Markdown 文件索引与 exclude 目录排除
- 已发现链接的解析与目标填充
- 索引进度回调报告
- 删除笔记时清理 entries、chunks、fts、links 四表

### provider 集成测试

- 搜索返回排序命中（从已索引 vault）
- graph 通道使用已发现的纯文本链接
- temporal pattern 忽略非 ASCII 查询
- `get` 按 path ID 返回完整 entry
- 搜索使用 orchestrator 扩展词
- 搜索在 embedding 存在时使用语义通道
- 语义通道在 vector 索引启用时使用 vector_top_k
- 语义通道优先使用 SQL vector_distance（降级前）
- 语义通道仅在 vector SQL 不可用时使用 Rust cosine
- entry metadata 加载过滤到融合命中 ID
- rerank 通道标记最终命中
- 笔记路径规范化拒绝非法路径
- create_note 写入、索引并返回 entry
- create_note 拒绝已存在路径（不覆盖）

### chunker 单元测试

- 小 Markdown 保持为单分块
- 大 Markdown 按标题拆分
- 避免在代码围栏内切割
- 语义断点评分覆盖所有行类型
- thematic break 优先在目标长度附近切割
- 过大分块按句子边界拆分，后续分块标题加 `(cont.)`

### parser 单元测试

- frontmatter 列表和标量字段解析
- wikilink 和 inline tag 提取

### links 单元测试

- 发现 exact、alias、fuzzy、first-name 四种链接
- 跳过已有 wikilink 和保护区域
- 模糊匹配尊重阈值
- 人名首名匹配要求姓氏唯一

### fusion 单元测试

- 跨通道重复 ID 融合
- 单通道唯一命中保留
- 加权融合尊重通道权重

### context 单元测试

- 预算内完整组装
- 小预算时截断并标记 `[truncated]`

### provider_router 单元测试

- 路由调用到已注册 provider
- 拒绝未知 provider（`SourceUnavailable`）

### models 绑定测试

- knowledge 模型绑定优先级与全局回退

## 相关文档

- [知识检索设计](../design/knowledge-search.md) — 搜索管线算法、融合策略、意图分类的详细设计文档
- [本地模型集成](../design/local-models.md) — EmbeddingModel、RerankModel、OrchestratorModel 的本地 LLM 集成架构
- [存储概述](./overview.md) — knowledge.db 表结构和索引概览
- [Memory 存储](./memory.md) — 记忆系统存储模块，与知识系统互补