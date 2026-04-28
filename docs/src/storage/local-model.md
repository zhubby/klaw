# Local Model 存储

`klaw-model` 提供本地 GGUF 模型的下载、存储、索引、推理运行时和绑定保护，是 Klaw 本地模型系统的核心模块。支持从 HuggingFace 下载模型文件、manifest-based 索引管理、llama.cpp 推理（Rust 绑定与命令行后端）、以及 Knowledge 搜索所需的 embedding / reranker / orchestrator 推理能力。

## 设计目标

- **Manifest 索引驱动**：所有已安装模型通过全局 `manifest.json` 索引管理，每个模型记录完整的元数据（来源、文件列表、能力、量化信息、时间戳等）
- **绑定保护**：模型被 Knowledge 或全局默认配置绑定期间禁止删除，避免运行时突然失去关键推理能力
- **流式下载与校验**：从 HuggingFace 下载模型时采用流式写入 `.part` 临时文件 + SHA256 实时哈希，支持取消和断点重试
- **多后端推理**：llama.cpp Rust 绑定（默认）和命令行后端双轨可选，同一套 trait 接口覆盖 Embedding / Rerank / Chat / Orchestrator 四种推理能力
- **模型缓存共享**：`LlamaBackend` 全进程单例初始化，GGUF model handle 通过 `Arc<Mutex<HashMap>>` 缓存复用，避免重复加载
- **配置分层**：全局默认模型 ID + Knowledge 专属模型 ID 双层绑定，Knowledge 优先覆盖全局回退
- **自动格式检测**：PromptFormat 根据 GGUF 文件名自动检测（Qwen Embedding / EmbeddingGemma / Raw），简化用户配置

## 模块结构

```text
klaw-model/src/
├── lib.rs          # 对外导出、re-export（所有公开类型、trait、service）
├── types.rs        # ModelCapability、ModelUsageBinding、InstalledModelManifest 等数据类型
├── catalog.rs      # HuggingFaceModelRef 校验、normalize_model_id 规范化
├── manifest.rs     # InstalledModelsManifest 索引读写、单模型 manifest 读写
├── storage.rs      # ModelStorage、ModelStoragePaths — 文件系统管理与 manifest CRUD
├── download.rs     # HuggingFaceDownloader — 下载管线、SHA256、.part 临时文件、进度回调
├── llama_cpp.rs    # ModelLlamaRuntime<B>、LlamaCppRsBackend、LlamaCppCommandBackend、推理 trait
├── service.rs      # ModelService — 下载 + 存储的编排层
└── error.rs        # ModelError 错误枚举
```

## 数据目录

模型文件存储在独立的数据目录下，默认根路径为 `~/.klaw/models/`。

```text
models/
  manifest.json              # 全局模型索引（JSON，包含所有已安装模型的 InstalledModelsManifest）
  snapshots/                 # 模型文件实体，按 model_id 分目录
    Qwen__Qwen3-Reranker-0.6B-GGUF--main/
      qwen3-reranker-0.6b-q4_k_m.gguf
      tokenizer.json
    Qwen__Qwen3-Embedding-0.6B-GGUF--main/
      qwen3-embedding-0.6b-q4_k_m.gguf
      tokenizer.json
  cache/
    downloads/               # 下载过程中的 .part 临时文件，完成后 rename 到 snapshots
      Qwen__Qwen3-Embedding-0.6B-GGUF--main.qwen3-embedding-0.6b-q4_k_m.gguf__part.part
  manifests/                 # （遗留）旧版逐模型 manifest 目录，已自动迁移到根 manifest.json
```

路径解析优先级：

1. `config.models.root_dir` 显式配置
2. `config.storage.root_dir` 下的 `models/` 子目录
3. `~/.klaw/models/`（默认数据目录）

## Manifest 结构

### InstalledModelsManifest（根索引）

`manifest.json` 是全局模型索引文件，包含所有已安装模型的完整记录：

```json
{
  "models": [
    {
      "model_id": "Qwen__Qwen3-Embedding-0.6B-GGUF--main",
      "source": "huggingface",
      "repo_id": "Qwen/Qwen3-Embedding-0.6B-GGUF",
      "revision": "main",
      "resolved_revision": "abcdef1234567890",
      "default_gguf_model_file": "snapshots/Qwen__Qwen3-Embedding-0.6B-GGUF--main/qwen3-embedding-0.6b-q4_k_m.gguf",
      "files": [
        {
          "relative_path": "snapshots/Qwen__Qwen3-Embedding-0.6B-GGUF--main/qwen3-embedding-0.6b-q4_k_m.gguf",
          "size_bytes": 423456789,
          "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
          "format": "gguf"
        },
        {
          "relative_path": "snapshots/Qwen__Qwen3-Embedding-0.6B-GGUF--main/tokenizer.json",
          "size_bytes": 1234567,
          "sha256": null,
          "format": "tokenizer_json"
        }
      ],
      "capabilities": ["embedding"],
      "quantization": "Q4_K_M",
      "size_bytes": 424691256,
      "installed_at": "2026-04-25T08:30:00Z",
      "last_used_at": "2026-04-26T14:20:00Z"
    }
  ]
}
```

### InstalledModelManifest 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `model_id` | String | 规范化模型 ID，由 `normalize_model_id` 生成 |
| `source` | String | 来源标识（当前固定 `"huggingface"`） |
| `repo_id` | String | HuggingFace 仓库 ID（如 `Qwen/Qwen3-Embedding-0.6B-GGUF`） |
| `revision` | String | 请求的分支/标签（如 `main`） |
| `resolved_revision` | Option\<String\> | revision 解析后的 SHA commit hash |
| `default_gguf_model_file` | Option\<String\> | 默认 GGUF 文件的相对路径（必须属于 `files` 且 `format == gguf`） |
| `files` | Vec\<InstalledModelFile\> | 所有已下载文件列表 |
| `capabilities` | Vec\<ModelCapability\> | 模型能力标签（下载时默认为空，由用户或系统后续标注） |
| `quantization` | Option\<String\> | 量化方案标识（如 `Q4_K_M`） |
| `size_bytes` | u64 | 所有文件总大小（字节） |
| `installed_at` | String | 安装时间（RFC 3339 格式） |
| `last_used_at` | Option\<String\> | 最近使用时间（RFC 3339 格式），由 `mark_used` 更新 |

### Model ID 规范

`normalize_model_id(repo_id, revision)` 将 HuggingFace 的 `owner/name` + `revision` 组合为扁平的目录安全 ID：

```text
normalize_model_id("Qwen/Qwen3-Embedding-0.6B-GGUF", "main")
→ "Qwen__Qwen3-Embedding-0.6B-GGUF--main"

normalize_model_id("TheBloke/Llama-2-7B-GGUF", "refs/pr/42")
→ "TheBloke__Llama-2-7B-GGUF--refs__pr__42"
```

规则：`/` 替换为 `__`，`owner` 和 `name` 之间用 `__` 连接，`name` 和 `revision` 之间用 `--` 连接。

### InstalledModelFile 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `relative_path` | String | 相对于 `root_dir` 的文件路径（统一使用 `/` 分隔符） |
| `size_bytes` | u64 | 文件大小（字节） |
| `sha256` | Option\<String\> | SHA256 哈希值（下载时实时计算） |
| `format` | ModelFileFormat | 文件格式分类（`gguf` / `tokenizer_json` / `other`） |

## Rust 类型

### ModelCapability

```text
pub enum ModelCapability {
    Embedding,   // 向量嵌入
    Rerank,      // 相关性重排
    Chat,        // 对话生成
    Orchestrator, // 查询意图分析 + 扩展
}
```

`serde(rename_all = "snake_case")` 序列化为 `"embedding"` / `"rerank"` / `"chat"` / `"orchestrator"`。

### ModelFileFormat

```text
pub enum ModelFileFormat {
    Gguf,           // .gguf 模型权重文件
    TokenizerJson,  // tokenizer.json 分词器文件
    Other,          // 其他文件（README、config 等）
}
```

文件名检测规则：`.gguf` 后缀 → `Gguf`；`tokenizer.json` 后缀 → `TokenizerJson`；其余 → `Other`。

### ModelUsageBinding

```text
pub enum ModelUsageBinding {
    Embedding,              // 全局默认 embedding 绑定
    Reranker,               // 全局默认 reranker 绑定
    Chat,                   // 全局默认 chat 绑定
    KnowledgeEmbedding,     // Knowledge 专属 embedding 绑定
    KnowledgeOrchestrator,  // Knowledge 专属 orchestrator 绑定
    KnowledgeReranker,      // Knowledge 专属 reranker 绑定
}
```

绑定类型分两层：全局层（`Embedding` / `Reranker` / `Chat`）和 Knowledge 专属层（`KnowledgeEmbedding` / `KnowledgeOrchestrator` / `KnowledgeReranker`）。删除模型时，调用方传入该模型当前的所有 `active_bindings`，任何非空绑定列表都会触发 `ModelError::InUse` 拒绝。

### InstalledModelManifest / InstalledModelFile / ModelSummary / ModelInstallRequest / ModelInstallResult

详见上方 Manifest 结构章节的 Rust 定义。`ModelSummary` 是 `list_installed` 返回的精简视图（不含 `files` 列表和 `resolved_revision`）。`ModelInstallRequest` 和 `ModelInstallResult` 是下载管线中的请求/响应类型。

## 核心 API

### ModelStoragePaths

```text
pub struct ModelStoragePaths {
    pub root_dir: PathBuf,        // 模型数据根目录
    pub manifest_path: PathBuf,   // manifest.json 路径
    pub snapshots_dir: PathBuf,   // snapshots/ 目录
    pub cache_dir: PathBuf,       // cache/ 目录
    pub downloads_dir: PathBuf,   // cache/downloads/ 目录
}
```

构造方式：

| 方法 | 说明 |
|------|------|
| `from_root(root_dir)` | 从给定根目录推导所有子路径 |
| `from_config(config)` | 根据 AppConfig 中的 `models.root_dir` / `storage.root_dir` / 默认路径推导 |
| `ensure_dirs()` | 创建 `root_dir`、`snapshots_dir`、`downloads_dir` 目录 |

路径推导优先级：

```text
from_config(config):
  config.models.root_dir         → 直接使用
  config.storage.root_dir + "models" → 组合使用
  ~/.klaw/models/                → 默认回退
```

### ModelStorage

```text
pub struct ModelStorage {
    paths: ModelStoragePaths,
}
```

| 方法 | 签名 | 说明 |
|------|------|------|
| `new` | `(paths: ModelStoragePaths) → Self` | 构造存储实例 |
| `open_default` | `(config: &AppConfig) → Result<Self>` | 从配置构造 + `ensure_dirs` |
| `paths` | `() → &ModelStoragePaths` | 获取路径引用 |
| `save_manifest` | `(manifest: &InstalledModelManifest) → Result<()>` | 写入/更新单模型 manifest（upsert 语义：同 model_id 替换） |
| `list_installed` | `() → Result<Vec<ModelSummary>>` | 列出所有已安装模型的精简摘要（按 model_id 排序） |
| `load_manifest` | `(model_id: &str) → Result<InstalledModelManifest>` | 按 model_id 查找单模型 manifest |
| `mark_used` | `(model_id: &str) → Result<()>` | 更新 `last_used_at` 为当前时间 |
| `set_default_gguf_model_file` | `(model_id, relative_path: Option<String>) → Result<InstalledModelManifest>` | 设置或清除默认 GGUF 文件；必须属于 `files` 且 `format == Gguf` |
| `remove_model` | `(model_id, active_bindings: &[ModelUsageBinding]) → Result<()>` | 删除模型文件 + manifest 记录；绑定非空时拒绝 |

**内部方法**：

| 方法 | 说明 |
|------|------|
| `load_all_manifests` | 合并根 `manifest.json` + 遗留 `manifests/` 目录中的 per-model JSON，自动迁移 |
| `save_all_manifests` | 按 model_id 排序后写入根 `manifest.json` |

### Manifest 操作语义

`save_manifest` 采用 **upsert** 语义：

```text
load_all_manifests → 保留非同 ID 的现有记录 → append 新/更新记录 → save_all_manifests
```

`load_all_manifests` 内置 **遗留迁移**：

```text
若 root_dir/manifests/ 目录存在：
  1. 读取每个 *.json 为 InstalledModelManifest
  2. 仅当 model_id 不在根 manifest.json 中才合并
  3. 合合完成后统一回写到根 manifest.json
```

### 文件删除与目录修剪

`remove_model` 删除每个文件后调用 `prune_empty_parents`，从文件父目录向上逐层删除空目录直到 `root_dir`：

```text
删除 snapshots/Qwen__model--main/model.gguf
  → 检查 snapshots/Qwen__model--main/ 是否为空 → 删除
  → 检查 snapshots/ 是否为空 → 保留（可能有其他模型）
```

## 下载流程

### HuggingFaceDownloader

```text
pub struct HuggingFaceDownloader {
    client: Client,            // reqwest HTTP 客户端
    endpoint: String,          // HuggingFace API endpoint（默认 https://huggingface.co）
    auth_token: Option<String>, // Bearer auth token
}
```

### 管线步骤

```text
ModelService::install_model(request, cancellation, progress)
  │
  ├─ 1. HuggingFaceModelRef::new(repo_id, revision)  — 校验 repo_id 格式（必须含 /）
  ├─ 2. normalize_model_id(repo_id, revision)         — 生成扁平 model_id
  ├─ 3. resolve_revision_sha(model_ref, cancellation) — GET /api/models/{repo}/revision/{rev}
  │     → 返回 Option<String>（SHA commit hash）
  ├─ 4. current_manifest_if_matching(storage, model_id, resolved_revision)
  │     → 若已安装的 manifest.resolved_revision 匹配 → 返回 up_to_date=true 跳过下载
  ├─ 5. list_repo_files(model_ref, cancellation)       — GET /api/models/{repo}/tree/{rev}?recursive=true
  │     → 过滤 type=="file" 的条目 → 返回文件名列表
  ├─ 6. 循环 download_file(model_id, model_ref, file_name, ...)
  │     ├─ GET {endpoint}/{repo_id}/resolve/{revision}/{file_name}
  │     ├─ 流式写入 .part 临时文件
  │     ├─ SHA256 实时哈希计算
  │     ├─ 每个字节块触发 progress(DownloadProgress)
  │     ├─ 取消检测：CancellationToken → 删除 .part → ModelError::Cancelled
  │     └─ 完成后 rename .part → 最终路径
  ├─ 7. 构建 InstalledModelManifest（capabilities 默认空、installed_at 为当前 UTC）
  └─ 8. storage.save_manifest(&manifest) — 写入根 manifest.json
```

### DownloadProgress 回调

```text
pub struct DownloadProgress {
    pub model_id: String,
    pub file_name: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,   // content_length，可能未知
    pub file_index: usize,          // 当前文件序号（1-based）
    pub total_files: usize,         // 总文件数
}
```

进度回调类型：`FnMut(DownloadProgress) + Send`，由 `ModelService` 通过 `Arc<Mutex<F>>` 包装传递。

### 取消与重试机制

- 每个步骤前调用 `check_cancelled(cancellation)`，若已取消则返回 `ModelError::Cancelled`
- 流式下载中逐块检测 `cancellation.is_cancelled()`，取消后删除 `.part` 临时文件
- `ModelService::install_model` 在取消时不调用 `save_manifest`，保证 manifest 不被部分写入
- 重试策略：重新调用 `install_model` 即可；`.part` 文件不会阻碍重试（每次下载新建 `.part`）

### 文件格式自动检测

`detect_format(file_name)` 根据文件名后缀分类：

```text
".gguf"           → ModelFileFormat::Gguf
"tokenizer.json"  → ModelFileFormat::TokenizerJson
其他              → ModelFileFormat::Other
```

## 推理运行时

### ModelLlamaRuntime\<B\>

```text
pub struct ModelLlamaRuntime<B> {
    storage: ModelStorage,
    backend: B,              // B: LlamaCppBackend
}
```

`ModelLlamaRuntime<B>` 通过泛型后端 `B` 统一承载四种推理能力，`B` 约束为 `LlamaCppBackend` trait 实现。

### 后端选择

| 后端 | 类型 | 说明 |
|------|------|------|
| `LlamaCppRsBackend` | Rust 绑定（默认） | 直接调用 llama.cpp C API，单进程内推理，支持 embedding / rerank / chat 全能力 |
| `LlamaCppCommandBackend` | CLI 回退 | 子进程调用 `llama-cli`，仅 chat 可用，embedding 未实现，rerank 降级为字符串匹配 |

`default_backend_kind()` 返回 `LlamaBackendKind::RustBinding`。

### LlamaCppRsBackend 内部结构

```text
pub struct LlamaCppRsBackend {
    default_ctx_size: u32,
    model_cache: Arc<Mutex<HashMap<PathBuf, Arc<LlamaModel>>>>,
    llama_backend: Arc<OnceLock<Result<Arc<LlamaBackend>, String>>>,
}
```

**LlamaBackend 单例**：

```text
static LLAMA_BACKEND: OnceLock<Mutex<Weak<LlamaBackend>>> = OnceLock::new();

shared_llama_backend():
  1. 从 OnceLock 获取全局 Mutex<Weak<LlamaBackend>>
  2. Weak::upgrade() 成功 → 返回现有 Arc（跨 LlamaCppRsBackend clone 共享）
  3. Weak::upgrade() 失败 → LlamaBackend::init() 创建新实例
     → 禁用 llama.cpp native 日志（send_logs_to_tracing）
     → 存入 Weak → 返回 Arc
```

所有 `LlamaCppRsBackend` clone 共享同一个 `OnceLock` 和 `Weak<LlamaBackend>`，保证全进程只初始化一次 llama.cpp backend。`model_cache` 通过 `Arc::clone` 在 clone 间共享，避免同一 GGUF 文件重复加载。

### GGUF 文件定位策略

`gguf_path_for_model(model_id)` 的查找顺序：

```text
1. manifest.default_gguf_model_file
   → 必须同时满足：
     a) 文件的 relative_path == default_gguf_model_file
     b) 文件的 format == Gguf
     c) relative_path 以 ".gguf" 结尾
   → 任何不匹配 → ModelError::Manifest

2. default_gguf_model_file 不在 files 列表中
   → ModelError::Manifest（"not a GGUF file in model"）

3. default_gguf_model_file 为 None
   → 遍历 files 找第一个 relative_path 以 ".gguf" 结尾的文件
   → 未找到 → ModelError::NotFound

4. 最终路径 = root_dir.join(file.relative_path)
```

### 四种推理能力

#### Embedding 推理

```text
EmbeddingRuntime::embed(request: ModelEmbeddingRequest) → ModelEmbeddingResponse
  1. gguf_path_for_model(model_id) → 定位 GGUF 文件
  2. PromptFormat::detect(file_name) → 选择 prompt 格式
  3. format_query(text) → 格式化输入文本
  4. str_to_token(formatted, AddBos::Never) → 分词
  5. embedding_context_params(n_tokens, prompt_format) → 配置 context
     - QwenEmbedding: pooling=Last, flash_attention=false
     - 其他: pooling=Mean, flash_attention=true
     - n_ctx = max(n_tokens, default_ctx_size)
  6. LlamaBatch::add_sequence → 编码
  7. embeddings_seq_ith(0) → L2 归一化 → 输出向量
```

#### Rerank 推理

```text
RerankRuntime::rerank(request: ModelRerankRequest) → ModelRerankResponse
  1. gguf_path_for_model(model_id) → 定位 GGUF 文件
  2. first_token_id(model, "Yes") / first_token_id(model, "No") → 获取判定 token ID
  3. 对每个 candidate:
     a. format_reranker_input(query, candidate) → 构造 ChatML prompt
     b. str_to_token(input, AddBos::Always) → 分词
     c. 逐 token add 到 batch → decode
     d. 取最后 token 的 logits
     e. softmax_binary_probability(yes_logit, no_logit) → 相关性分数
```

Reranker prompt 格式：

```text
<|im_start|>system
Judge whether the document is relevant to the search query. Respond only with "Yes" or "No".<|im_end|>
<|im_start|>user
Search query: {query}
Document: {candidate}<|im_end|>
<|im_start|>assistant
```

#### Chat 推理

```text
ChatRuntime::chat(request: ModelChatRequest) → ModelChatResponse
  1. gguf_path_for_model(model_id) → 定位 GGUF 文件
  2. str_to_token(prompt, AddBos::Always) → 分词
  3. 逐 token add 到 batch → decode（prefill）
  4. 自回归生成（最多 256 tokens）:
     a. LlamaSampler::greedy() 采样
     b. model.is_eog_token(new_token) → 停止
     c. token_to_piece → UTF-8 解码 → 追加输出
  5. trim 输出 → 返回
```

#### Orchestrator 推理

```text
OrchestratorRuntime::orchrate(request: ModelOrchestrateRequest) → ModelOrchestrateResponse
  1. gguf_path_for_model(model_id) → 定位 GGUF 文件
  2. format_orchestrator_prompt(query) → 构造 ChatML prompt
  3. backend.run_chat(prompt) → 获取原始文本
  4. parse_orchestrator_response(text, original_query):
     a. extract_json_object → 找到 JSON 块
     b. 解析 intent + expansions
     c. 确保 expansions[0] == original_query
     d. dedup expansions
  5. 解析失败 → heuristic_orchestrator_response(query) 降级回退
```

Orchestrator system prompt：

```text
You are a search query analyzer. Given a user's search query, classify it and expand it.
Return JSON with:
- "intent": one of "exact", "conceptual", "relationship", "exploratory", "temporal"
- "expansions": 2-4 alternative phrasings (always include the original query first)
Be concise. Only return the JSON object.
```

### QueryIntent 分类

```text
pub enum QueryIntent {
    Exact,          // 精确匹配（引号、id:）
    Conceptual,     // 概念探索（how、why、architecture）
    Relationship,   // 关系查询（[[链接、depends on）
    Exploratory,    // 通用探索（默认）
    Temporal,       // 时间相关（today、recent、202x）
}
```

启发式降级 `heuristic_query_intent(query)` 关键词匹配规则：

| 模式 | 映射 |
|------|------|
| `today` / `recent` / `latest` / `yesterday` / `last week` / `this month` / `202` | Temporal |
| `[[` / `relationship` / `related` / `link` / `backlink` / `depends on` | Relationship |
| `"..."` / `exact` / `quote` / `id:` | Exact |
| `how` / `why` / `architecture` / `design` / `concept` | Conceptual |
| 其他 | Exploratory |

### PromptFormat 自动检测

```text
pub enum PromptFormat {
    EmbeddingGemma,    // 文件名含 "embeddinggemma"
    QwenEmbedding,     // 文件名含 "qwen" + "embed"
    Raw,               // 默认
}
```

| 格式 | query 模板 | document 模板 |
|------|-----------|---------------|
| EmbeddingGemma | `<bos>search_query: {text}` | `<bos>search_document: {text}` |
| QwenEmbedding | `Instruct: Retrieve relevant passages\nQuery: {text}` | 原文 |
| Raw | 原文 | 原文 |

## 绑定保护

### 绑定类型表

| 绑定类型 | 层级 | 说明 |
|---------|------|------|
| `Embedding` | 全局 | `config.models.default_embedding_model_id` 绑定 |
| `Reranker` | 全局 | `config.models.default_reranker_model_id` 绑定 |
| `Chat` | 全局 | `config.models.default_chat_model_id` 绑定 |
| `KnowledgeEmbedding` | Knowledge | `config.knowledge.models.embedding_model_id` 绑定 |
| `KnowledgeOrchestrator` | Knowledge | `config.knowledge.models.orchestrator_model_id` 绑定 |
| `KnowledgeReranker` | Knowledge | `config.knowledge.models.reranker_model_id` 绑定 |

### 删除拒绝逻辑

```text
ModelStorage::remove_model(model_id, active_bindings):
  if active_bindings.is_empty():
    → 删除文件 + 移除 manifest 记录
  else:
    → 返回 ModelError::InUse(model_id)
```

调用方（GUI / CLI）在删除前需查询该模型的所有绑定来源，将非空绑定列表传入。若模型被任何全局默认或 Knowledge 配置引用，删除操作被拒绝，保证运行时不会突然失去关键推理能力。

## 配置体系

### config.toml 模型配置

```toml
[models]
enabled = true                        # 是否启用本地模型系统（默认 false）
root_dir = "/path/to/models"          # 模型数据根目录（可选，默认 ~/.klaw/models/）
default_embedding_model_id = "Qwen__Qwen3-Embedding-0.6B-GGUF--main"  # 全局默认 embedding 模型
default_reranker_model_id = "Qwen__Qwen3-Reranker-0.6B-GGUF--main"    # 全局默认 reranker 模型
default_chat_model_id = "Qwen__Qwen3-0.6B-GGUF--main"                 # 全局默认 chat 模型

[models.huggingface]
endpoint = "https://huggingface.co"   # HuggingFace API endpoint
token = ""                            # Bearer auth token（可选，私有仓库需要）

[models.llama_cpp]
command = "llama-cli"                 # llama.cpp CLI 命令路径
library_path = ""                     # llama.cpp 共享库路径（可选，用于 Rust 绑定加载）
default_ctx_size = 4096               # 默认 context window 大小
```

### Knowledge 模型绑定配置

```toml
[knowledge.models]
embedding_provider = "local_model"    # embedding 提供者标识
embedding_model_id = "Qwen__Qwen3-Embedding-0.6B-GGUF--main"  # Knowledge 专属 embedding
orchestrator_model_id = "Qwen__Qwen3-0.6B-GGUF--main"         # Knowledge 专属 orchestrator
reranker_model_id = "Qwen__Qwen3-Reranker-0.6B-GGUF--main"    # Knowledge 专属 reranker
```

### 绑定优先级

```text
Knowledge 查询推理时的模型选择优先级：

  knowledge.models.embedding_model_id     → 优先使用（Knowledge 专属）
  models.default_embedding_model_id       → 回退（全局默认）

  knowledge.models.reranker_model_id      → 优先使用（Knowledge 专属）
  models.default_reranker_model_id        → 回退（全局默认）

  knowledge.models.orchestrator_model_id  → 优先使用（Knowledge 专属）
  （orchestrator 无全局回退，必须显式配置）
```

### 验证约束

当 `models.enabled = true` 时，配置校验器强制以下规则：

| 规则 | 错误信息 |
|------|---------|
| `root_dir` 若配置则不能为空/纯空格 | `models.root_dir cannot be empty when configured` |
| `huggingface.endpoint` 不能为空 | `models.huggingface.endpoint cannot be empty when models.enabled=true` |
| `llama_cpp.command` 不能为空 | `models.llama_cpp.command cannot be empty when models.enabled=true` |
| `default_embedding_model_id` 若配置则不能为空 | `models.default_embedding_model_id cannot be empty when configured` |
| `default_reranker_model_id` 若配置则不能为空 | `models.default_reranker_model_id cannot be empty when configured` |
| `default_chat_model_id` 若配置则不能为空 | `models.default_chat_model_id cannot be empty when configured` |

## 与 Knowledge 的集成

本地模型为 Knowledge 搜索系统提供三种推理能力：

```text
┌─────────────────────────────────────────────────────────┐
│                    Knowledge Search                       │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Embedding   │  │   Reranker   │  │ Orchestrator │  │
│  │  向量嵌入    │  │  结果重排     │  │  查询意图    │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         │                 │                  │           │
└─────────┼─────────────────┼──────────────────┼───────────┘
          │                 │                  │
          ▼                 ▼                  ▼
┌─────────────────────────────────────────────────────────┐
│              ModelLlamaRuntime<B>                        │
│                                                          │
│  embed()            rerank()         orchestrate()       │
│  ↓                  ↓                ↓                   │
│  gguf_path →        gguf_path →      gguf_path →        │
│  PromptFormat →     Yes/No logits →  Chat → JSON parse  │
│  token → ctx →      softmax →        heuristic fallback  │
│  embedding → L2     score list      intent + expansions  │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │  LlamaCppBackend (Rs / Command)                  │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

### Embedding → chunk 向量化 + 语义搜索

- Knowledge 文档 chunk 写入时调用 `embed()` 生成向量
- 搜索时 query 也调用 `embed()` 生成查询向量
- 向量相似度检索在 `klaw-memory` 的向量索引中完成

### Reranker → pass1 结果精排

- Knowledge 两阶段检索的 pass1（粗筛）结果传入 `rerank()`
- `RerankRuntime` 对每个候选 chunk 计算相关性分数
- 高分候选进入 pass2（精排）

### Orchestrator → 查询意图 + 扩展

- 用户原始搜索 query 传入 `orchestrate()`
- 返回 `QueryIntent`（意图分类）+ `expansions`（扩展短语列表）
- 扩展短语用于多路并行检索，提高召回率
- 模型推理失败时自动降级到启发式规则

## 后端策略

### Rust 绑定（LlamaCppRsBackend）

| 优势 | 劣势 |
|------|------|
| 单进程内推理，无子进程开销 | 编译依赖 llama.cpp C 库，需要 `library_path` 或预编译 |
| 支持全部四种推理能力 | 需要更严格的内存管理（model cache、backend 单例） |
| model handle 缓存复用，重复请求零加载延迟 | 阻塞推理需 `spawn_blocking` 避免占 async runtime |
| 共享 LlamaBackend（Weak 引用），进程级单初始化 | 不同 GGUF 量化需不同 context 参数，缓存 key 必须含路径 |

### 命令行后端（LlamaCppCommandBackend）

| 优势 | 劣势 |
|------|------|
| 无编译依赖，仅需 `llama-cli` 可执行文件 | 子进程启动延迟，不适合高频调用 |
| 配置简单 | embedding 未实现（返回 Unsupported） |
| 部署灵活 | rerank 降级为字符串子串匹配（`contains(query)` → 1.0 / 0.0） |
| | chat 输出受 CLI 格式影响，可能含噪声 |

### 选择建议

```text
生产环境 / Knowledge 搜索 → LlamaCppRsBackend（完整推理能力 + 缓存复用）
快速验证 / 无编译环境    → LlamaCppCommandBackend（仅 chat 可用，rerank 降级）
```

## 测试覆盖

`klaw-model/src/storage.rs` 测试覆盖以下核心路径：

| 测试 | 说明 |
|------|------|
| `lists_saved_manifests_as_model_summaries` | 保存 manifest 后 `list_installed()` 返回正确摘要，根 manifest.json 存在且无遗留目录 |
| `sets_default_gguf_model_file_in_manifest` | 设置 default GGUF 文件后 manifest 持久化正确，reload 一致 |
| `rejects_default_gguf_model_file_not_in_manifest` | 设置不在 files 列表中的路径返回 `ModelError::Manifest` |
| `clears_default_gguf_model_file_in_manifest` | `set_default_gguf_model_file(id, None)` 清除默认，持久化后 reload 为 None |
| `rejects_default_gguf_model_file_with_non_gguf_format` | 设置 `TokenizerJson` 格式文件为默认 GGUF 返回 `ModelError::Manifest` |
| `migrates_legacy_per_model_manifests_into_root_manifest` | 遗留 `manifests/*.json` 自动合并到根 `manifest.json`，迁移后遗留文件仍存在 |
| `rejects_removal_when_model_is_bound` | `active_bindings = [Embedding]` 时 `remove_model` 返回 `ModelError::InUse` |
| `removes_model_from_root_manifest` | 无绑定时成功删除：文件不存在、manifest 列表为空、根 manifest 持久化 |

`klaw-model/src/llama_cpp.rs` 测试覆盖：

| 测试 | 说明 |
|------|------|
| `resolves_first_gguf_path_from_manifest` | 无 default 时取第一个 `.gguf` 文件 |
| `resolves_manifest_default_gguf_path_before_file_order` | 有 default 时优先取 default，即使文件列表顺序不同 |
| `rejects_manifest_default_gguf_path_outside_file_list` | default 指向不在 files 列表中的路径返回 Manifest 错误 |
| `default_backend_kind_prefers_rust_binding` | `default_backend_kind()` 返回 RustBinding |
| `cloned_rust_backend_shares_lazy_backend_owner` | clone 后 `OnceLock` 共享同一底层 |
| `llama_log_options_disable_native_logs` | 日志选项正确禁用 llama.cpp native 输出 |
| `prompt_format_detects_qwen_embedding_models` | 含 "qwen" + "embed" 的文件名检测为 QwenEmbedding |
| `qwen_embedding_context_uses_last_pooling` | QwenEmbedding 使用 pooling=Last |
| `qwen_embedding_context_disables_flash_attention` | QwenEmbedding 禁用 flash attention |
| `embedding_context_sizes_to_current_batch` | context size 适配当前 batch 大小 |
| `prompt_format_detects_embeddinggemma_models` | 含 "embeddinggemma" 的文件名检测为 EmbeddingGemma |
| `softmax_probability_prefers_higher_yes_logit` | Yes logit > No logit 时概率 > 0.5 |
| `parses_orchestrator_json_expansions_from_embedded_text` | 从嵌入文本中提取 JSON 块并解析 expansions |
| `heuristic_orchestrator_fallback_keeps_original_query_first` | 启发式降级 expansions[0] == 原始 query |
| `parser_inserts_original_query_when_missing` | JSON expansions 不含原始 query 时自动插入到首位 |
| `heuristic_intent_detects_temporal_queries` | 含时间关键词的 query 检测为 Temporal |

`klaw-model/src/download.rs` 测试覆盖：

| 测试 | 说明 |
|------|------|
| `detects_file_format_from_name` | `.gguf` → Gguf，`tokenizer.json` → TokenizerJson，其他 → Other |
| `renders_relative_path_with_forward_slashes` | Windows 风格 `\` 统一替换为 `/` |
| `collect_tree_file_paths_keeps_only_files` | HuggingFace tree API 返回中仅保留 `type=="file"` 条目 |
| `collect_tree_file_paths_rejects_empty_file_list` | 文件列表为空返回 `ModelError::Download` |
| `parse_revision_sha_reads_model_info_sha` | 从 model info JSON 正确提取 SHA |

`klaw-model/src/service.rs` 测试覆盖：

| 测试 | 说明 |
|------|------|
| `cancelled_install_stops_before_manifest_is_saved` | CancellationToken 取消后 install_model 返回 Cancelled，manifest 不被写入 |
| `current_manifest_matches_resolved_revision` | resolved_revision 匹配现有 manifest 返回 up_to_date，不匹配或 None 返回 None |

## 相关文档

- [本地模型系统详细设计](../design/local-models.md) — 算法细节、推理管线优化、GGUF 加载策略
- [存储概述](./overview.md) — 所有存储子系统一览
- [Memory 存储](./memory.md) — 向量索引与混合检索（与本地模型 embedding 集成）
- [Knowledge 搜索设计](../design/knowledge-search.md) — 两阶段检索架构与模型绑定关系
- [Model GUI 面板](../ui/gui/local-models.md) — 模型管理面板界面与操作流程