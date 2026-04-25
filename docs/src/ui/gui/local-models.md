# Local Models

`Local Models` 面板用于管理桌面端的本地模型资产。

## 当前能力

- 查看已安装模型
- 从 Hugging Face 下载指定 repo/revision 的完整仓库快照
- 在下载弹窗中查看每个文件的进度并取消未完成下载
- 在可选中表格中查看模型名称、大小和创建日期
- 通过右键菜单升级模型到当前 revision 的最新快照；若远端 revision SHA 与本地 manifest 一致，会提示已是最新并跳过下载
- 通过右键菜单删除未被当前配置引用的模型，删除前会弹窗确认
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
  snapshots/
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
token = "hf_..."

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
