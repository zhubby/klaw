# klaw-config

`klaw-config` 负责 `klaw` 配置文件的模型定义、序列化/反序列化、默认模板、迁移与校验。

## 能力

- 提供 `AppConfig` 及子配置结构。
- 支持读取/初始化 `~/.klaw/config.toml`。
- 支持按默认配置迁移已有配置文件。
- 支持通过 `ConfigStore` 在进程内共享配置快照，并在保存/重载后同步内存状态。
- 校验 provider、工具、MCP、gateway、heartbeat 等配置合法性。
- 支持按 channel 配置会话命令开关（`channels.disable_session_commands_for`）。
- 支持 `storage.root_dir` 配置项，用于覆盖默认 `~/.klaw` 数据目录根路径。

## 模型配置

- 根级 `model_provider`：选择当前活跃 provider。
- 根级可选 `model`：覆盖活跃 provider 的 `default_model`。
- `model_providers.<id>.default_model`：provider 默认模型。
