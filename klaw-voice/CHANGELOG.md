# CHANGELOG

## 2026-03-26

### Changed

- `VoiceService::from_config()` 不再依赖 legacy `voice.enabled` 开关；只要 provider 配置有效即可构建服务，方便 GUI 测试与上层 runtime 以统一语义复用 voice 配置

## 2026-03-24

### Added

- 新增 `klaw-voice` crate，提供完整 voice provider 抽象、服务层、STT/TTS 数据模型，以及 Deepgram、AssemblyAI、ElevenLabs 的 provider 实现与流式接口
