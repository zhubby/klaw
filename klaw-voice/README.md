# klaw-voice

`klaw-voice` 提供语音领域能力，包括：

- 统一的 `VoiceProvider` 抽象
- STT / TTS 请求与响应模型
- 非流式与流式语音接口
- `VoiceService` 服务层
- Deepgram / AssemblyAI / ElevenLabs provider 实现

当前首个业务接入点是 Telegram 入站语音识别；TTS provider 能力和服务层也在本 crate 中实现，供后续 `TtsTool` 与 channel 出站语音复用。
