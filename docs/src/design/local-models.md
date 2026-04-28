# 本地模型系统设计文档

本文详细描述 `klaw-model` 模块的架构、核心算法与配置体系，涵盖本地模型的下载、存储、推理运行时全链路。

## 系统架构总览

本地模型系统采用 **存储-服务-运行时** 三层架构：

```
┌─────────────────────────────────────────────────────────────────┐
│                        调用层                                   │
│  klaw-knowledge (Embedding/Rerank/Orchestrator)                │
│  klaw-gui (Model 面板 / 下载 / 管理)                            │
└──────────────────────┬──────────────────────────────────────────┤
│                      ▼                                         │
│  ┌──────────────────────────────────────────┐                  │
│  │          ModelService                     │                  │
│  │  install / list / remove / bind 管理      │                  │
│  └──────────┬───────────────┬────────────────┘                  │
│             ▼               ▼                                    │
│  ┌──────────────┐  ┌──────────────────┐                         │
│  │ ModelStorage  │  │ HuggingFace      │                         │
│  │ manifest.json │  │ Downloader       │                         │
│  │ snapshots/    │  │ (API + 文件下载)  │                         │
│  └──────────────┘  └──────────────────┘                         │
│             │                                                   │
│             ▼                                                   │
│  ┌──────────────────────────────────────────┐                  │
│  │          ModelLlamaRuntime<B>             │                  │
│  │  resolve GGUF → load → inference          │                  │
│  │  ┌───────────────────────────────────┐   │                  │
│  │  │   LlamaCppBackend (trait)          │   │                  │
│  │  │   ├─ LlamaCppRsBackend (Rust bind) │   │                  │
│  │  │   └─ LlamaCppCommandBackend (CLI)   │   │                  │
│  │  └───────────────────────────────────┘   │                  │
│  └──────────────────────────────────────────┘                  │
└─────────────────────────────────────────────────────────────────┘
```

## 存储层：ModelStorage

### 目录结构

默认存储根目录为 `~/.klaw/models/`，由 `ModelStoragePaths` 管理：

```text
models/
  manifest.json          # 全局模型索引（JSON）
  snapshots/             # 模型文件实体
    Qwen__Qwen3-Reranker-0.6B-GGUF--main/
      *.gguf
      tokenizer.json
      ...
  cache/
    downloads/           # 下载中的 .part 临时文件
```

### Manifest 结构

每个已安装模型记录在 `manifest.json` 中，核心字段：

```json
{
  "model_id": "ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main",
  "source": "huggingface",
  "repo_id": "ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF",
  "revision": "main",
  "resolved_revision": "abc123def456",
  "default_gguf_model_file": "snapshots/.../qwen3-reranker-0.6b-q8_0.gguf",
  "files": [
    {
      "relative_path": "snapshots/.../qwen3-reranker-0.6b-q8_0.gguf",
      "size_bytes": 629145600,
      "sha256": "a1b2c3...",
      "format": "gguf"
    }
  ],
  "capabilities": ["rerank"],
  "quantization": "Q8_0",
  "size_bytes": 629145600,
  "installed_at": "2026-04-25T00:00:00Z",
  "last_used_at": null
}
```

- `model_id` 由 `normalize_model_id(repo_id, revision)` 生成，格式为 `owner__name--revision`（如 `ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main`）
- `resolved_revision` 是下载时从 HuggingFace API 获取的 commit SHA，用于增量更新判断
- `default_gguf_model_file` 指定推理时优先加载的 GGUF 文件，可通过 GUI 或 API 手动设置
- `capabilities` 标注模型用途（`embedding` / `rerank` / `chat` / `orchestrator`）

### Model ID 规范

`model_id` 的生成规则：将 HuggingFace `repo_id` 中的 `/` 替换为 `__`，再用 `--` 连接 `revision`：

```text
repo_id: ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF
revision: main
→ model_id: ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main
```

### 绑定保护

删除模型时会检查当前配置绑定（`ModelUsageBinding`），若模型正被以下绑定引用则拒绝删除：

| 绑定类型 | 说明 |
|---------|------|
| `Embedding` | 全局默认 embedding |
| `Reranker` | 全局默认 reranker |
| `Chat` | 全局默认 chat |
| `KnowledgeEmbedding` | knowledge 专用 embedding |
| `KnowledgeOrchestrator` | knowledge 专用 orchestrator |
| `KnowledgeReranker` | knowledge 专用 reranker |

## 下载层：HuggingFaceDownloader

### 下载流程

```text
用户请求 install_model(repo_id, revision)
    │
    ├─ 1. resolve_revision_sha → 获取远端 commit SHA
    │     若本地 manifest 的 resolved_revision 匹配，直接返回 up_to_date=true
    │
    ├─ 2. list_repo_files → 获取仓库文件列表（递归 tree API）
    │
    ├─ 3. 逐文件 download_file
    │     ├─ 流式下载到 .part 临时文件
    │     ├─ 实时 SHA256 校验（流式哈希）
    │     ├─ 支持 CancellationToken 取消（删除临时文件）
    │     └─ 完成后 rename 到 snapshots/ 最终路径
    │
    └─ 4. 保存 InstalledModelManifest 到 manifest.json
```

### 取消与重试

- 下载全程支持 `CancellationToken`，取消时自动清理 `.part` 临时文件
- 进度回调 `FnMut(DownloadProgress)` 通知每个文件的已下载字节数、总字节数和文件序号
- 若本地 `resolved_revision` 与远端 SHA 一致，跳过下载直接返回 `up_to_date=true`

### 测试模型示例

以下模型为本项目推荐的测试模型：

| 模型 | repo_id | 能力 | quantization | 用途 |
|------|---------|------|-------------|------|
| **Qwen3-Reranker-0.6B-Q8_0** | `ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF` | rerank | Q8_0 | Knowledge 搜索二次精排 |
| **EmbeddingGemma-300M** | `unsloth/embeddinggemma-300m-GGUF` | embedding | 多种量化 | Knowledge chunk embedding |

配置示例：

```toml
[models]
enabled = true
default_embedding_model_id = "unsloth__embeddinggemma-300m-GGUF--main"
default_reranker_model_id = "ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main"

[models.huggingface]
endpoint = "https://huggingface.co"
# token = "hf_..."  # 可选，私有仓库需要

[models.llama_cpp]
command = "llama-cli"
default_ctx_size = 4096
```

## 运行时层：推理引擎

### Backend 选择

`LlamaBackendKind` 决定推理执行方式：

| Backend | 说明 | 优先级 |
|---------|------|--------|
| `RustBinding` | `llama-cpp-2` Rust 绑定，直接内存加载 | **默认** |
| `Command` | `llama-cli` 子进程调用，兼容/调试 fallback | 次选 |

`default_backend_kind()` 默认返回 `RustBinding`。Rust binding 需本机安装 `cmake` + `clang`。

### LlamaCppRsBackend 核心机制

1. **共享后端**：全局 `Lazy<Mutex<Weak<LlamaBackend>>>` 确保 llama.cpp native backend 只初始化一次，多个 `LlamaCppRsBackend` 实例共享同一 backend 句柄

2. **模型缓存**：`LlamaCppRsBackend` 内部持有 `model_cache: Arc<Mutex<HashMap<PathBuf, Arc<LlamaModel>>>>`，同一 GGUF 文件只加载一次，后续推理共享模型句柄

3. **推理线程安全**：每次推理创建新的 `LlamaContext`，模型共享但上下文独立，避免多推理互相干扰

### GGUF 文件定位

`ModelLlamaRuntime::gguf_path_for_model` 的定位策略：

```text
1. 从 ModelStorage 加载 manifest
2. 若 manifest.default_gguf_model_file 存在 → 直接使用
3. 否则遍历 manifest.files：
   a. 筛选 format == Gguf 的文件
   b. 按文件名长度降序排列（优先选大文件/主模型）
   c. 取第一个
4. 将 relative_path 解析为绝对路径并校验文件存在
```

### 四种推理能力

#### 1. Embedding 推理

```
输入文本 → PromptFormat.format_query → tokenize → LlamaContext(embeddings=true) → encode → embeddings_seq_ith → L2 归一化 → Vec<f32>
```

关键参数：
- `pooling_type`：Qwen embedding 使用 `Last` pooling；EmbeddingGemma / Raw 使用默认
- `flash_attention`：Qwen embedding 显式禁用 `LLAMA_FLASH_ATTN_TYPE_DISABLED`
- `n_ctx`：取 `max(token_count + 16, 64)`，不使用 default_ctx_size（embedding 无需大窗口）
- `n_batch / n_ubatch`：取 `max(token_count, 512)`
- 输出向量经 **L2 归一化**（`l2_normalize`），确保余弦相似度等价于内积

#### 2. Rerank 推理（核心算法）

Reranker 采用 **Yes/No 二元判断 + softmax 归一化** 的经典架构：

```
对每个候选文档：
  构造 prompt = format_reranker_input(query, document)
  tokenize → LlamaContext → decode 全序列
  取最后一个 token 的 logits
  提取 "Yes" 和 "No" 两个 token 的 logit
  softmax_binary_probability → P(Yes) = score
```

Prompt 格式（Qwen3 风格 ChatML）：

```text
<|im_start|>system
Judge whether the document is relevant to the search query. Respond only with "Yes" or "No".<|im_end|>
<|im_start|>user
Search query: {query}
Document: {document}<|im_end|>
<|im_start|>assistant
```

`softmax_binary_probability` 计算：

```text
score = exp(yes_logit - max_logit) / (exp(yes_logit - max_logit) + exp(no_logit - max_logit))
```

其中 `max_logit = max(yes_logit, no_logit)` 用于数值稳定性。

**特性：**
- 对每个候选独立评分，不依赖候选间排序
- 输出概率在 [0, 1] 区间，天然适合做 relevance score
- Q8_0 量化足以保持 logit 精度（reranker 只读 2 个 token 的 logits）

#### 3. Chat 推理

```
prompt → tokenize → LlamaContext → greedy sampler → 自回归生成（最多 256 tokens）→ trim → String
```

用于 orchestrator 的内部调用，不对外暴露通用 chat 能力。

#### 4. Orchestrator 推理（Query Intent + Expansion）

Orchestrator 复用 Chat 推理通道，但使用专用 system prompt：

```text
You are a search query analyzer. Given a user's search query, classify it and expand it.

Return JSON with:
- "intent": one of "exact", "conceptual", "relationship", "exploratory", "temporal"
- "expansions": 2-4 alternative phrasings (always include the original query first)

Be concise. Only return the JSON object.
```

响应解析流程：

```text
模型输出 → extract_json_object（定位第一个完整 JSON 对象）
         → parse_orchestrator_response
            ├─ parse_query_intent → QueryIntent enum
            ├─ expansions 列表提取 + 去重
            └─ 若原始查询不在 expansions[0] → 强制插入
         → 失败时回退到 heuristic_orchestrator_response
```

### 启发式 Orchestrator 回退

当无 orchestrator 模型或推理失败时，自动回退到启发式规则：

**Query Intent 启发式 (`heuristic_query_intent`)：**

| 触发关键词 | Intent |
|-----------|--------|
| `today`, `recent`, `latest`, `yesterday`, `last week`, `this month`, `202` | `Temporal` |
| `[[...]]`, `relationship`, `related`, `link`, `backlink`, `depends on` | `Relationship` |
| `"..."`, `exact`, `quote`, `id:` | `Exact` |
| `how`, `why`, `architecture`, `design`, `concept` | `Conceptual` |
| 其他 | `Exploratory` |

**Query Expansion 启发式 (`heuristic_orchestrator_expansions`)：**

- 原始查询作为第一个 expansion
- 若查询超过 2 个词，去掉停用词（how/does/the/a/an/is/...）后保留长度 > 2 的词作为额外 expansion
- 去重后返回

### PromptFormat 自动检测

`PromptFormat::detect` 根据 GGUF 文件名推断 prompt 格式：

| 文件名特征 | PromptFormat | format_query 行为 |
|-----------|-------------|------------------|
| 包含 `embeddinggemma` | `EmbeddingGemma` | Gemma 风格 prompt template |
| 包含 `qwen` + `embedding` | `QwenEmbedding` | Qwen ChatML + Last pooling |
| 其他 | `Raw` | 直接传入原始文本 |

## 配置体系

### 模型绑定优先级

Knowledge 模型的绑定遵循 **就近优先 + 全局回退** 原则：

```text
knowledge.models.embedding_model_id  ──→ 若存在则使用
                                     ──→ 否则回退到 models.default_embedding_model_id

knowledge.models.reranker_model_id   ──→ 若存在则使用
                                     ──→ 否则回退到 models.default_reranker_model_id

knowledge.models.orchestrator_model_id ──→ 仅 knowledge 级配置，无全局回退
```

`resolve_model_bindings` 函数统一处理上述逻辑，返回 `KnowledgeModelBindings`。

### 配置结构一览

```toml
[models]
enabled = true                         # 是否启用本地模型子系统
root_dir = "~/.klaw/models"            # 存储根目录（可选）
default_embedding_model_id = "..."     # 全局默认 embedding model_id
default_reranker_model_id = "..."      # 全局默认 reranker model_id
default_chat_model_id = "..."          # 全局默认 chat model_id

[models.huggingface]
endpoint = "https://huggingface.co"    # HF API endpoint
token = "hf_..."                       # 认证 token（可选）

[models.llama_cpp]
command = "llama-cli"                  # CLI fallback 命令
library_path = "/path/to/libllama"     # 动态库路径（可选）
default_ctx_size = 4096                # 默认 context 窗口大小
```

```toml
[knowledge.models]
embedding_provider = "local"           # embedding provider（默认 local）
embedding_model_id = "..."             # knowledge 专用 embedding（优先）
orchestrator_model_id = "..."          # knowledge 专用 orchestrator
reranker_model_id = "..."              # knowledge 专用 reranker（优先）
```

### 验证约束

- `models.default_reranker_model_id` 若配置则不可为空字符串
- `knowledge.models.reranker_model_id` 若配置则不可为空字符串
- `knowledge.retrieval.rerank_candidates` 必须 > 0（启用 knowledge 时）

## 测试模型配置示例

以 **Qwen3-Reranker-0.6B-Q8_0-GGUF** 和 **embeddinggemma-300m-GGUF** 作为测试模型：

```toml
[models]
enabled = true
default_embedding_model_id = "unsloth__embeddinggemma-300m-GGUF--main"
default_reranker_model_id = "ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main"

[models.huggingface]
endpoint = "https://huggingface.co"

[models.llama_cpp]
command = "llama-cli"
default_ctx_size = 4096

[knowledge]
enabled = true
provider = "obsidian"

[knowledge.obsidian]
vault_path = "/Users/me/Knowledge"
auto_index = true
max_excerpt_length = 400

[knowledge.retrieval]
top_k = 5
rerank_candidates = 20
graph_hops = 1
temporal_decay = 0.85

[knowledge.models]
embedding_model_id = "unsloth__embeddinggemma-300m-GGUF--main"
reranker_model_id = "ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main"

[tools.knowledge]
enabled = true
search_limit = 5
context_limit = 3
```

### 模型能力对照

| model_id | 能力 | PromptFormat | 推理方式 |
|----------|------|-------------|---------|
| `unsloth__embeddinggemma-300m-GGUF--main` | embedding | EmbeddingGemma | tokenize → encode → L2 normalize |
| `ggml-org__Qwen3-Reranker-0.6B-Q8_0-GGUF--main` | rerank | Raw (ChatML) | Yes/No logits → softmax → P(Yes) |