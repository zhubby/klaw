# Voice 工具

## 功能描述

`Voice` 工具提供语音能力：
- **ASR** (Automatic Speech Recognition) - 语音转文字
- **TTS** (Text-to-Speech) - 文字转语音

支持多种第三方提供商：
- ASR: Deepgram, AssemblyAI
- TTS: ElevenLabs

生成的语音会自动归档存储，可以通过 `channel_attachment` 发送给用户。

## 配置

```toml
# 全局语音配置在根配置段
[voice.asr]
enabled = true
provider = "deepgram"
env_key = "DEEPGRAM_API_KEY"

[voice.tts]
enabled = true
provider = "elevenlabs"
env_key = "ELEVENLABS_API_KEY"
default_voice_id = "pNInz6obpgDQGcFmaJgB"

# 工具配置
[tools.voice]
enabled = true
```

## 参数说明

### 语音转文字（ASR）

```json
{
  "action": "transcribe",
  "archive_id": "audio-file-archive-id",
  "language": "zh"
}
```

参数：
- `action`: `"transcribe"` - 语音转文字
- `archive_id`: `string` - 音频文件归档 ID
- `language`: `string` (可选) - 语言代码（`zh`, `en` 等）

### 文字转语音（TTS）

```json
{
  "action": "synthesize",
  "text": "你好，欢迎使用 Klaw",
  "voice_id": "pNInz6obpgDQGcFmaJgB",
  "speed": 1.0,
  "filename": "greeting.mp3"
}
```

参数：
- `action`: `"synthesize"` - 文字转语音
- `text`: `string` - 要合成的文字
- `voice_id`: `string` (可选) - 语音 ID，覆盖默认
- `speed`: `number` (可选) - 语速，默认 1.0
- `filename`: `string` (可选) - 输出文件名

## 输出说明

ASR 返回识别出的文字。TTS 返回归档 ID 和音频文件信息，可直接作为附件发送。

## 配置示例

### Deepgram ASR

```toml
[voice.asr]
enabled = true
provider = "deepgram"
env_key = "DEEPGRAM_API_KEY"
model = "nova-2"
```

### ElevenLabs TTS

```toml
[voice.tts]
enabled = true
provider = "elevenlabs"
env_key = "ELEVENLABS_API_KEY"
default_voice_id = "pNInz6obpgDQGcFmaJgB"
model_id = "eleven_multilingual_v2"
```

## 使用场景

- 语音聊天机器人
- 会议录音转录
- 语音通知
- 无障碍访问
