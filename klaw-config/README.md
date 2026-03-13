# klaw-config

`klaw-config` 负责 `klaw` 配置文件的模型定义、序列化/反序列化、默认模板、迁移与校验。

## 能力

- 提供 `AppConfig` 及子配置结构。
- 支持读取/初始化 `~/.klaw/config.toml`。
- 支持按默认配置迁移已有配置文件。
- 校验 provider、工具、MCP、gateway、heartbeat 等配置合法性。

## 模型配置

- 根级 `model_provider`：选择当前活跃 provider。
- 根级可选 `model`：覆盖活跃 provider 的 `default_model`。
- `model_providers.<id>.default_model`：provider 默认模型。
