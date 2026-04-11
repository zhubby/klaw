# 语音面板

## 功能说明

语音交互配置，支持语音转文字（STT）和文字转语音（TTS）。

## 核心功能

- 启用/禁用语音功能
- 配置 STT 提供者
- 配置 TTS 提供者
- 选择默认语言
- 选择默认语音 ID
- 测试语音识别和合成

## 支持的提供者

**STT（语音转文字）：**
- OpenAI Whisper API
- 本地 Whisper
- 其他...

**TTS（文字转语音）：**
- OpenAI TTS
- ElevenLabs
- 本地 TTS

## 配置示例

```toml
[voice]
enabled = true
stt_provider = "openai"
tts_provider = "openai"
default_language = "zh-CN"
```

## 相关设计文档

- [Voice 模块设计](../../superpowers/plans/voice-module-design.md)
