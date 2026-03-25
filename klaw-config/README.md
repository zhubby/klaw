# klaw-config

`klaw-config` 负责 `klaw` 配置文件的模型定义、序列化/反序列化、默认模板、迁移与校验。

## 能力

- 提供 `AppConfig` 及子配置结构。
- 支持读取/初始化 `~/.klaw/config.toml`。
- 支持按默认配置迁移已有配置文件。
- 支持通过 `ConfigStore` 在进程内共享配置快照，并在保存/重载后同步内存状态。
- 校验 provider、工具、MCP、gateway、heartbeat 等配置合法性。
- 支持按 channel 配置会话命令开关（`channels.disable_session_commands_for`）。
- 保持 channel 外部配置为分类型数组（当前 `channels.dingtalk` / `channels.telegram`），供运行时映射为统一的 channel 实例快照。
- 支持 `storage.root_dir` 配置项，用于覆盖默认 `~/.klaw` 数据目录根路径。
- 支持独立的 `tools.skills_registry` 与 `tools.skills_manager` 开关配置。
- 支持 `observability.local_store` 配置项,用于控制本地分析存储是否启用、保留时长与刷新间隔。
- 支持 `gateway.enabled` 开关配置、`gateway.listen_port = 0` 随机端口模式，以及 `gateway.webhook.events` / `gateway.webhook.agents` 双 webhook 入口配置。
- 支持完整 `voice` 配置块，用于声明 STT/TTS 默认 provider、默认语言/音色，以及 Deepgram、AssemblyAI、ElevenLabs 的 provider 参数。
- `heartbeat.defaults.timezone` 在未显式配置时会默认采用系统探测到的 timezone，而不是硬编码 `UTC`。

## 模型配置

- 根级 `model_provider`：选择当前活跃 provider。
- 根级可选 `model`：覆盖活跃 provider 的 `default_model`。
- `model_providers.<id>.default_model`：provider 默认模型。
- `model_providers.<id>.stream`：是否启用 provider 原生 stream API。
- `channels.telegram[].stream_output` / `channels.dingtalk[].stream_output`：是否允许 channel 侧尝试增量输出。
- `voice.enabled`：是否启用语音能力。
- `voice.stt_provider`：当前 STT provider，支持 `deepgram` / `assemblyai`。
- `voice.tts_provider`：当前 TTS provider，当前支持 `elevenlabs`。
- `voice.default_language` / `voice.default_voice_id`：语音默认语言与默认音色。
- `voice.providers.*`：各语音 provider 的 `api_key` / `api_key_env` / base URL / streaming URL / 模型参数。
