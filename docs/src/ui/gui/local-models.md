# Local Models

`Local Models` 面板用于管理桌面端的本地模型资产。

## 当前能力

- 查看已安装模型
- 从 Hugging Face 显式下载指定 repo/revision/file
- 将模型绑定为默认 `embedding` / `reranker` / `chat`
- 删除未被当前配置引用的模型
- 打开本地模型目录

## 存储位置

默认存储在：

```text
~/.klaw/models
```

目录分层：

```text
models/
  manifests/
  blobs/
  cache/downloads/
```

## 配置

```toml
[models]
enabled = true
default_embedding_model_id = "Qwen__Qwen3-Embedding-0.6B-GGUF--main"
default_reranker_model_id = "Qwen__Qwen3-Reranker-0.6B-GGUF--main"

[models.huggingface]
endpoint = "https://huggingface.co"
auth_token_env = "HF_TOKEN"

[models.llama_cpp]
command = "llama-cli"
```

默认情况下，`klaw-model` 会优先使用 Rust binding backend
(`llama-cpp-2` / `utilityai/llama-cpp-rs`) 运行本地模型，而不是通过 `llama-cli`
子进程调用。`command` 仍保留为兼容/调试 fallback 配置。

构建默认 backend 需要本机安装：

- `cmake`
- `clang`

`knowledge` 不再需要直接持有裸模型文件路径，而是优先使用：

- `knowledge.models.embedding_model_id`
- `knowledge.models.reranker_model_id`

若未显式设置，则回退到：

- `models.default_embedding_model_id`
- `models.default_reranker_model_id`
