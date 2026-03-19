# Voice 模块设计

## 背景

`klaw-voice` 模块负责语音转文字（STT）和文字转语音（TTS）能力，架构在 `klaw-archive` 模块的本地存储能力之上。主要职责：

1. 从 channel 收到的语音消息通过 STT 转为文本
2. 提供 TTS Tool，使模型的回复变成语音，保存在 archive，然后通过 channel 发出

支持的供应商：
- ElevenLabs（TTS 优先）
- Deepgram（STT 优先）
- AssemblyAI

## 设计结论

首版 voice 采用以下方案：

- 新增独立 crate：`klaw-voice`
- STT 在 Channel 层直接调用，转文字后再提交给 runtime
- TTS 通过 ToolSignal 机制触发 channel 发送
- 配置文件指定默认 STT/TTS provider
- 同时支持流式 TTS 和 STT

## 模块放置

### `klaw-voice`

负责 voice 的领域语义，包括：

- `VoiceProvider` trait 抽象
- `VoiceService` 服务封装
- STT/TTS 数据模型
- 流式处理支持
- 具体供应商实现（ElevenLabs/Deepgram/AssemblyAI）

### `klaw-tool`

新增 `TtsTool`：

- 继承现有 `Tool` trait
- 调用 `VoiceService` 执行 TTS
- 生成语音后保存到 archive
- 发出 `voice_message_ready` 信号

### `klaw-config`

扩展配置模型：

- 新增 `VoiceConfig` 配置
- 新增各供应商配置（`ElevenLabsConfig`/`DeepgramConfig`/`AssemblyAiConfig`）

### `klaw-channel`

在 channel 消息处理中集成 STT：

- 检测语音类型媒体
- 调用 `VoiceService.transcribe()`
- 使用转录文本作为用户输入

## 实现方案

### 模块结构

```
klaw-voice/
├── Cargo.toml
├── src/
│   ├── lib.rs              # 模块入口，导出公共API
│   ├── error.rs            # VoiceError 定义
│   ├── model.rs            # 数据模型（TTS/STT 请求响应）
│   ├── provider.rs         # VoiceProvider trait 定义
│   ├── service.rs          # VoiceService 封装
│   ├── stream.rs           # 流式处理支持
│   └── providers/
│       ├── mod.rs
│       ├── elevenlabs.rs   # ElevenLabs 实现
│       ├── deepgram.rs     # Deepgram 实现
│       └── assemblyai.rs   # AssemblyAI 实现
```

### 核心抽象

#### VoiceProvider Trait

```rust
#[async_trait]
pub trait VoiceProvider: Send + Sync {
    fn name(&self) -> &str;

    async fn transcribe(&self, input: SttInput) -> Result<SttOutput, VoiceError>;

    async fn transcribe_stream(
        &self,
        input: SttStreamInput,
    ) -> Result<SttStreamOutput, VoiceError>;

    async fn synthesize(&self, input: TtsInput) -> Result<TtsOutput, VoiceError>;

    async fn synthesize_stream(
        &self,
        input: TtsStreamInput,
    ) -> Result<TtsStreamOutput, VoiceError>;

    fn capabilities(&self) -> VoiceCapabilities;
}

#[derive(Debug, Clone)]
pub struct VoiceCapabilities {
    pub supports_streaming_stt: bool,
    pub supports_streaming_tts: bool,
    pub supported_languages: Vec<String>,
    pub voice_ids: Vec<String>,
}
```

#### 数据模型

```rust
#[derive(Debug, Clone)]
pub struct SttInput {
    pub audio_bytes: Vec<u8>,
    pub mime_type: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SttOutput {
    pub text: String,
    pub language: Option<String>,
    pub confidence: Option<f32>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct TtsInput {
    pub text: String,
    pub voice_id: Option<String>,
    pub language: Option<String>,
    pub speed: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct TtsOutput {
    pub audio_bytes: Vec<u8>,
    pub mime_type: String,
    pub duration_ms: Option<u64>,
}
```

### VoiceService 封装

```rust
pub struct VoiceService {
    stt_provider: Arc<dyn VoiceProvider>,
    tts_provider: Arc<dyn VoiceProvider>,
    archive: Arc<dyn ArchiveService>,
    config: VoiceRuntimeConfig,
}

impl VoiceService {
    pub async fn transcribe(&self, input: SttInput) -> Result<SttOutput, VoiceError>;
    pub async fn synthesize(&self, input: TtsInput) -> Result<TtsOutput, VoiceError>;
    pub async fn synthesize_and_archive(
        &self,
        input: TtsInput,
        archive_input: ArchiveIngestInput,
    ) -> Result<(TtsOutput, ArchiveRecord), VoiceError>;
}
```

### TTS Tool 实现

```rust
pub struct TtsTool {
    voice_service: Arc<VoiceService>,
}

impl Tool for TtsTool {
    fn name(&self) -> &str { "tts" }

    fn description(&self) -> &str {
        "Convert text to speech audio. Use when you want to respond with voice output."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to convert to speech (max 5000 characters)"
                },
                "voice_id": {
                    "type": "string",
                    "description": "Optional voice ID override"
                }
            },
            "required": ["text"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let text = args.get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing `text`".into()))?;

        let tts_output = self.voice_service.synthesize(TtsInput {
            text: text.to_string(),
            voice_id: args.get("voice_id").and_then(Value::as_str).map(Into::into),
            language: None,
            speed: None,
        }).await?;

        let archive_input = ArchiveIngestInput {
            source_kind: ArchiveSourceKind::ModelGenerated,
            filename: Some(format!("tts-{}.mp3", Uuid::new_v4())),
            declared_mime_type: Some(tts_output.mime_type.clone()),
            session_key: Some(ctx.session_key.clone()),
            channel: ctx.metadata.get("channel").and_then(Value::as_str).map(Into::into),
            chat_id: ctx.metadata.get("chat_id").and_then(Value::as_str).map(Into::into),
            message_id: None,
            metadata: json!({}),
        };

        let record = self.voice_service
            .archive_audio(&tts_output, archive_input)
            .await?;

        Ok(ToolOutput {
            content_for_model: json!({
                "status": "success",
                "archive_id": record.id,
                "duration_ms": tts_output.duration_ms,
            }).to_string(),
            content_for_user: Some(format!("语音已生成，时长 {}ms", tts_output.duration_ms.unwrap_or(0))),
        })
    }
}
```

### ToolSignal 定义

```rust
pub fn voice_message_ready(
    archive_id: &str,
    session_key: &str,
    chat_id: &str,
    channel: &str,
) -> ToolSignal {
    ToolSignal {
        kind: "voice_message_ready".to_string(),
        payload: json!({
            "archive_id": archive_id,
            "session_key": session_key,
            "chat_id": chat_id,
            "channel": channel,
        }),
    }
}
```

## 配置设计

在 `klaw-config` 中新增 voice 配置：

```toml
[voice]
enabled = true
stt_provider = "deepgram"
tts_provider = "elevenlabs"
default_language = "zh-CN"
default_voice_id = "21m00Tcm4TlvDq8ikWAM"

[voice.providers.elevenlabs]
api_key_env = "ELEVENLABS_API_KEY"
base_url = "https://api.elevenlabs.io"
default_model = "eleven_multilingual_v2"

[voice.providers.deepgram]
api_key_env = "DEEPGRAM_API_KEY"
base_url = "https://api.deepgram.com"
stt_model = "nova-2"

[voice.providers.assemblyai]
api_key_env = "ASSEMBLYAI_API_KEY"
base_url = "https://api.assemblyai.com"
```

配置模型：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    #[serde(default = "default_voice_enabled")]
    pub enabled: bool,
    pub stt_provider: String,
    pub tts_provider: String,
    #[serde(default = "default_voice_language")]
    pub default_language: String,
    pub default_voice_id: Option<String>,
    pub providers: BTreeMap<String, VoiceProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceProviderConfig {
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: String,
    pub default_model: Option<String>,
    pub stt_model: Option<String>,
    pub default_voice_id: Option<String>,
}
```

## Channel 集成

### STT 调用（在 Channel 消息处理中）

```rust
impl DingtalkChannel {
    async fn process_voice_message(
        &self,
        media: &MediaReference,
        runtime: &dyn ChannelRuntime,
    ) -> ChannelResult<String> {
        let audio_bytes = if let Some(bytes) = &media.bytes {
            bytes.clone()
        } else if let Some(url) = &media.remote_url {
            self.client.download_media(url).await?
        } else {
            return Err("voice message has no audio data".into());
        };

        let voice_service = self.voice_service.as_ref()
            .ok_or("voice service not configured")?;

        let stt_output = voice_service.transcribe(SttInput {
            audio_bytes,
            mime_type: media.mime_type.clone().unwrap_or("audio/wav".into()),
            language: Some("zh-CN".into()),
        }).await?;

        Ok(stt_output.text)
    }
}
```

### Signal 处理（在 Runtime 中）

```rust
// 在 agent 执行完成后，检查 tool_signals
for signal in &output.tool_signals {
    if signal.kind == "voice_message_ready" {
        let archive_id = signal.payload["archive_id"].as_str().unwrap();
        let channel = signal.payload["channel"].as_str().unwrap();
        let chat_id = signal.payload["chat_id"].as_str().unwrap();

        let blob = archive.open_download(archive_id).await?;

        let outbound = OutboundMessage {
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
            content: String::new(),
            reply_to: None,
            metadata: json!({
                "voice_archive_id": archive_id,
                "voice_mime_type": blob.record.mime_type,
            }),
        };

        channel.send_voice(&outbound, &blob.bytes).await?;
    }
}
```

## 流式处理

### 流式 STT

```rust
pub struct SttStreamInput {
    pub audio_stream: Pin<Box<dyn Stream<Item = Result<Bytes, VoiceError>> + Send>>,
    pub mime_type: String,
    pub language: Option<String>,
}

pub struct SttStreamOutput {
    pub transcript_stream: Pin<Box<dyn Stream<Item = Result<SttSegment, VoiceError>> + Send>>,
}

pub struct SttSegment {
    pub text: String,
    pub is_final: bool,
    pub confidence: Option<f32>,
}
```

### 流式 TTS

```rust
pub struct TtsStreamInput {
    pub text: String,
    pub voice_id: Option<String>,
    pub language: Option<String>,
}

pub struct TtsStreamOutput {
    pub audio_stream: Pin<Box<dyn Stream<Item = Result<Bytes, VoiceError>> + Send>>,
    pub mime_type: String,
}
```

流式处理允许边生成边发送，降低首字节延迟，适合实时交互场景。

## 供应商实现要点

### ElevenLabs

- TTS 核心供应商
- WebSocket 流式 TTS 支持
- 多语言/多音色支持
- API: `/v1/text-to-speech/{voice_id}`

### Deepgram

- STT 核心供应商
- WebSocket 流式 STT 支持
- 实时转录，低延迟
- API: `/v1/listen`

### AssemblyAI

- STT 备选供应商
- 支持 speaker diarization
- 支持情感分析等高级特性
- API: `/v2/transcript`

## 依赖关系

```
klaw-voice
├── klaw-archive (依赖: archive service)
├── klaw-config (依赖: voice config)
├── reqwest (HTTP client, 已在 workspace)
├── tokio (async runtime, 已在 workspace)
├── tokio-stream (流式处理)
├── async-trait (已在 workspace)
├── serde, serde_json (已在 workspace)
├── thiserror (已在 workspace)
├── tracing (已在 workspace)
└── base64 (已在 workspace)
```

## 实施计划

### 阶段 1：基础框架（2天）

- [ ] 创建 `klaw-voice` crate 骨架
- [ ] 定义 `VoiceProvider` trait 和数据模型
- [ ] 实现 `VoiceService` 封装
- [ ] 扩展 `klaw-config` 配置模型

### 阶段 2：供应商实现（3天）

- [ ] 实现 Deepgram STT（非流式）
- [ ] 实现 ElevenLabs TTS（非流式）
- [ ] 实现 AssemblyAI STT（非流式）
- [ ] 编写单元测试

### 阶段 3：Tool 和集成（2天）

- [ ] 实现 `TtsTool`
- [ ] 定义 `voice_message_ready` 信号
- [ ] 在 `klaw-cli runtime` 中处理信号
- [ ] 集成测试

### 阶段 4：Channel 集成（2天）

- [ ] 在 DingtalkChannel 中集成 STT
- [ ] 实现语音消息发送
- [ ] 端到端测试

### 阶段 5：流式支持（可选，2天）

- [ ] 实现流式 STT
- [ ] 实现流式 TTS
- [ ] 流式性能测试

## 测试方案

需要覆盖以下场景：

1. `VoiceProvider` trait 实现正确性
2. STT 能正确转录音频为文本
3. TTS 能正确生成音频
4. 生成的音频能正确保存到 archive
5. `TtsTool` 参数验证
6. `voice_message_ready` 信号正确生成
7. Channel 能正确处理语音消息并调用 STT
8. 流式 STT 的实时性和准确性
9. 流式 TTS 的低延迟特性
10. 供应商配置正确加载

## 后续演进

- 支持更多供应商（Azure Speech、Google Cloud Speech）
- 语音克隆（voice cloning）
- 多说话人识别（speaker diarization）
- 实时语音对话（双向流式）
- 语音情感分析
- 自定义唤醒词