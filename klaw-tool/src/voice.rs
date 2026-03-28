use async_trait::async_trait;
use klaw_archive::{
    ArchiveBlob, ArchiveIngestInput, ArchiveRecord, ArchiveService, ArchiveSourceKind,
    open_default_archive_service,
};
use klaw_config::AppConfig;
use klaw_voice::{SttInput, TtsInput, VoiceError, VoiceService};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

#[async_trait]
trait VoiceRuntime: Send + Sync {
    async fn transcribe(&self, input: SttInput) -> Result<klaw_voice::SttOutput, VoiceError>;

    async fn synthesize_and_archive(
        &self,
        input: TtsInput,
        archive: &dyn ArchiveService,
        archive_input: ArchiveIngestInput,
    ) -> Result<(klaw_voice::TtsOutput, ArchiveRecord), VoiceError>;

    fn stt_provider_name(&self) -> &'static str;

    fn tts_provider_name(&self) -> &'static str;
}

#[async_trait]
impl VoiceRuntime for VoiceService {
    async fn transcribe(&self, input: SttInput) -> Result<klaw_voice::SttOutput, VoiceError> {
        VoiceService::transcribe(self, input).await
    }

    async fn synthesize_and_archive(
        &self,
        input: TtsInput,
        archive: &dyn ArchiveService,
        archive_input: ArchiveIngestInput,
    ) -> Result<(klaw_voice::TtsOutput, ArchiveRecord), VoiceError> {
        VoiceService::synthesize_and_archive(self, input, archive, archive_input).await
    }

    fn stt_provider_name(&self) -> &'static str {
        VoiceService::stt_provider_name(self)
    }

    fn tts_provider_name(&self) -> &'static str {
        VoiceService::tts_provider_name(self)
    }
}

pub struct VoiceTool {
    archive: Arc<dyn ArchiveService>,
    voice: Arc<dyn VoiceRuntime>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VoiceRequest {
    action: String,
    #[serde(default)]
    archive_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    voice_id: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    speed: Option<f32>,
    #[serde(default)]
    filename: Option<String>,
}

impl VoiceTool {
    pub async fn open_default(config: &AppConfig) -> Result<Self, ToolError> {
        let archive = open_default_archive_service().await.map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to open archive service: {err}"))
        })?;
        let voice = VoiceService::from_config(&config.voice).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to build voice service: {err}"))
        })?;
        Ok(Self {
            archive: Arc::new(archive),
            voice: Arc::new(voice),
        })
    }

    #[cfg(test)]
    fn new(archive: Arc<dyn ArchiveService>, voice: Arc<dyn VoiceRuntime>) -> Self {
        Self { archive, voice }
    }

    fn parse_request(args: Value) -> Result<VoiceRequest, ToolError> {
        let mut request: VoiceRequest = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))?;
        request.action = request.action.trim().to_string();
        if request.action.is_empty() {
            return Err(ToolError::InvalidArgs(
                "`action` cannot be empty".to_string(),
            ));
        }
        trim_optional_string(&mut request.archive_id, "archive_id")?;
        trim_optional_string(&mut request.text, "text")?;
        trim_optional_string(&mut request.voice_id, "voice_id")?;
        trim_optional_string(&mut request.language, "language")?;
        trim_optional_string(&mut request.filename, "filename")?;
        Ok(request)
    }

    fn require_archive_id(request: &VoiceRequest) -> Result<&str, ToolError> {
        request
            .archive_id
            .as_deref()
            .ok_or_else(|| ToolError::InvalidArgs("missing `archive_id`".to_string()))
    }

    fn require_text(request: &VoiceRequest) -> Result<&str, ToolError> {
        request
            .text
            .as_deref()
            .ok_or_else(|| ToolError::InvalidArgs("missing `text`".to_string()))
    }

    fn record_to_json(record: &ArchiveRecord) -> Value {
        json!({
            "id": record.id,
            "source_kind": record.source_kind,
            "media_kind": record.media_kind,
            "mime_type": record.mime_type,
            "extension": record.extension,
            "original_filename": record.original_filename,
            "content_sha256": record.content_sha256,
            "size_bytes": record.size_bytes,
            "storage_rel_path": record.storage_rel_path,
            "session_key": record.session_key,
            "channel": record.channel,
            "chat_id": record.chat_id,
            "message_id": record.message_id,
            "created_at_ms": record.created_at_ms,
        })
    }

    fn build_archive_input(ctx: &ToolContext, request: &VoiceRequest) -> ArchiveIngestInput {
        ArchiveIngestInput {
            source_kind: ArchiveSourceKind::ModelGenerated,
            filename: request.filename.clone(),
            declared_mime_type: None,
            session_key: Some(ctx.session_key.clone()),
            channel: ctx
                .metadata
                .get("channel")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            chat_id: ctx
                .metadata
                .get("chat_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            message_id: None,
            metadata: json!({
                "voice.tool_action": "tts",
                "voice.session_key": ctx.session_key,
            }),
        }
    }

    fn ensure_audio_record(blob: &ArchiveBlob) -> Result<(), ToolError> {
        if blob.record.media_kind == klaw_archive::ArchiveMediaKind::Audio {
            return Ok(());
        }
        if blob.record.media_kind == klaw_archive::ArchiveMediaKind::Other {
            if blob
                .record
                .mime_type
                .as_deref()
                .is_some_and(|mime| mime.trim().starts_with("audio/"))
            {
                return Ok(());
            }
            if blob.record.extension.as_deref().is_some_and(|ext| {
                matches!(
                    ext.trim().to_ascii_lowercase().as_str(),
                    "mp3" | "wav" | "ogg" | "m4a" | "aac"
                )
            }) {
                return Ok(());
            }
        }
        Err(ToolError::InvalidArgs(format!(
            "archive `{}` is not an audio file",
            blob.record.id
        )))
    }
}

#[async_trait]
impl Tool for VoiceTool {
    fn name(&self) -> &str {
        "voice"
    }

    fn description(&self) -> &str {
        "Convert between text and archived audio. Use `stt` to read a voice/audio attachment by `archive_id` and return transcript text. Use `tts` to synthesize text into speech, archive the generated audio, and return the archived file info."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "description": "Voice conversion tool. Use `stt` for archived audio attachments and `tts` for generating a spoken response that should be saved into archive storage.",
            "oneOf": [
                {
                    "description": "Speech-to-text for one archived audio file. Use when the current or prior message includes a voice/audio attachment with an `archive_id`.",
                    "properties": {
                        "action": { "const": "stt" },
                        "archive_id": {
                            "type": "string",
                            "description": "Exact archive id of the audio file to transcribe."
                        },
                        "language": {
                            "type": "string",
                            "description": "Optional language hint such as `zh-CN` or `en-US`. Leave unset to use the voice service default."
                        }
                    },
                    "required": ["action", "archive_id"],
                    "additionalProperties": false
                },
                {
                    "description": "Text-to-speech for one text response. Use when you want to generate a voice file and keep it in archive storage.",
                    "properties": {
                        "action": { "const": "tts" },
                        "text": {
                            "type": "string",
                            "description": "Text to synthesize into speech."
                        },
                        "voice_id": {
                            "type": "string",
                            "description": "Optional voice id override for the configured TTS provider."
                        },
                        "language": {
                            "type": "string",
                            "description": "Optional language hint. Leave unset to use the voice service default."
                        },
                        "speed": {
                            "type": "number",
                            "description": "Optional speaking speed multiplier."
                        },
                        "filename": {
                            "type": "string",
                            "description": "Optional archived filename for the generated audio, such as `reply.mp3`."
                        }
                    },
                    "required": ["action", "text"],
                    "additionalProperties": false
                }
            ]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::NetworkWrite
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let request = Self::parse_request(args)?;
        let payload = match request.action.as_str() {
            "stt" => {
                let archive_id = Self::require_archive_id(&request)?;
                let blob = self
                    .archive
                    .open_download(archive_id)
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("failed to open archive file: {err}"))
                    })?;
                Self::ensure_audio_record(&blob)?;
                let mime_type = blob
                    .record
                    .mime_type
                    .clone()
                    .or_else(|| {
                        blob.record
                            .extension
                            .as_deref()
                            .map(|ext| format!("audio/{ext}"))
                    })
                    .unwrap_or_else(|| "audio/octet-stream".to_string());
                let output = self
                    .voice
                    .transcribe(SttInput {
                        audio_bytes: blob.bytes,
                        mime_type,
                        language: request.language.clone(),
                    })
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("voice stt request failed: {err}"))
                    })?;
                json!({
                    "action": "stt",
                    "archive_id": archive_id,
                    "provider": self.voice.stt_provider_name(),
                    "record": Self::record_to_json(&blob.record),
                    "text": output.text,
                    "language": output.language,
                    "confidence": output.confidence,
                    "duration_ms": output.duration_ms,
                })
            }
            "tts" => {
                let text = Self::require_text(&request)?;
                let archive_input = Self::build_archive_input(ctx, &request);
                let (output, record) = self
                    .voice
                    .synthesize_and_archive(
                        TtsInput {
                            text: text.to_string(),
                            voice_id: request.voice_id.clone(),
                            language: request.language.clone(),
                            speed: request.speed,
                        },
                        self.archive.as_ref(),
                        archive_input,
                    )
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("voice tts request failed: {err}"))
                    })?;
                json!({
                    "action": "tts",
                    "provider": self.voice.tts_provider_name(),
                    "record": Self::record_to_json(&record),
                    "mime_type": output.mime_type,
                    "duration_ms": output.duration_ms,
                    "archive_id": record.id,
                    "storage_rel_path": record.storage_rel_path,
                })
            }
            _ => {
                return Err(ToolError::InvalidArgs(
                    "`action` must be one of stt/tts".to_string(),
                ));
            }
        };

        Ok(ToolOutput {
            content_for_model: serde_json::to_string_pretty(&payload).map_err(|err| {
                ToolError::ExecutionFailed(format!("failed to serialize voice response: {err}"))
            })?,
            content_for_user: None,
            signals: Vec::new(),
        })
    }
}

fn trim_optional_string(value: &mut Option<String>, field: &str) -> Result<(), ToolError> {
    if let Some(raw) = value.as_mut() {
        *raw = raw.trim().to_string();
        if raw.is_empty() {
            return Err(ToolError::InvalidArgs(format!("`{field}` cannot be empty")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use klaw_archive::{ArchiveError, ArchiveMediaKind, ArchiveQuery};
    use std::path::PathBuf;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeArchiveService {
        blob: Mutex<Option<ArchiveBlob>>,
        ingested: Mutex<Option<(ArchiveIngestInput, Vec<u8>)>>,
    }

    #[async_trait]
    impl ArchiveService for FakeArchiveService {
        async fn ingest_path(
            &self,
            _input: ArchiveIngestInput,
            _source_path: &std::path::Path,
        ) -> Result<ArchiveRecord, ArchiveError> {
            unreachable!("ingest_path not used in tests")
        }

        async fn ingest_bytes(
            &self,
            input: ArchiveIngestInput,
            bytes: &[u8],
        ) -> Result<ArchiveRecord, ArchiveError> {
            self.ingested
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .replace((input, bytes.to_vec()));
            Ok(sample_record("arch-generated", ArchiveMediaKind::Audio))
        }

        async fn find(&self, _query: ArchiveQuery) -> Result<Vec<ArchiveRecord>, ArchiveError> {
            Ok(Vec::new())
        }

        async fn get(&self, archive_id: &str) -> Result<ArchiveRecord, ArchiveError> {
            Ok(sample_record(archive_id, ArchiveMediaKind::Audio))
        }

        async fn open_download(&self, _archive_id: &str) -> Result<ArchiveBlob, ArchiveError> {
            self.blob
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone()
                .ok_or_else(|| ArchiveError::NotFound("missing blob".to_string()))
        }

        async fn list_session_keys(&self) -> Result<Vec<String>, ArchiveError> {
            Ok(Vec::new())
        }
    }

    struct FakeVoiceRuntime;

    #[async_trait]
    impl VoiceRuntime for FakeVoiceRuntime {
        async fn transcribe(&self, input: SttInput) -> Result<klaw_voice::SttOutput, VoiceError> {
            Ok(klaw_voice::SttOutput {
                text: format!("transcribed {} bytes", input.audio_bytes.len()),
                language: input.language.or(Some("zh-CN".to_string())),
                confidence: Some(0.95),
                duration_ms: Some(1200),
            })
        }

        async fn synthesize_and_archive(
            &self,
            _input: TtsInput,
            archive: &dyn ArchiveService,
            archive_input: ArchiveIngestInput,
        ) -> Result<(klaw_voice::TtsOutput, ArchiveRecord), VoiceError> {
            let bytes = vec![1, 2, 3, 4];
            let record = archive
                .ingest_bytes(archive_input, &bytes)
                .await
                .map_err(|err| VoiceError::Config(err.to_string()))?;
            Ok((
                klaw_voice::TtsOutput {
                    audio_bytes: bytes,
                    mime_type: "audio/mpeg".to_string(),
                    duration_ms: Some(800),
                },
                record,
            ))
        }

        fn stt_provider_name(&self) -> &'static str {
            "deepgram"
        }

        fn tts_provider_name(&self) -> &'static str {
            "elevenlabs"
        }
    }

    fn sample_record(id: &str, media_kind: ArchiveMediaKind) -> ArchiveRecord {
        ArchiveRecord {
            id: id.to_string(),
            source_kind: ArchiveSourceKind::ChannelInbound,
            media_kind,
            mime_type: Some("audio/ogg".to_string()),
            extension: Some("ogg".to_string()),
            original_filename: Some("sample.ogg".to_string()),
            content_sha256: "abc".to_string(),
            size_bytes: 4,
            storage_rel_path: "archives/sample.ogg".to_string(),
            session_key: Some("telegram:bot:chat".to_string()),
            channel: Some("telegram".to_string()),
            chat_id: Some("chat".to_string()),
            message_id: Some("m1".to_string()),
            metadata_json: "{}".to_string(),
            created_at_ms: 1,
        }
    }

    fn base_ctx() -> ToolContext {
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("channel".to_string(), json!("telegram"));
        metadata.insert("chat_id".to_string(), json!("chat-1"));
        ToolContext {
            session_key: "telegram:bot-1:chat-1".to_string(),
            metadata,
        }
    }

    #[tokio::test]
    async fn stt_returns_transcript_for_archived_audio() {
        let archive = Arc::new(FakeArchiveService {
            blob: Mutex::new(Some(ArchiveBlob {
                record: sample_record("arch-1", ArchiveMediaKind::Audio),
                absolute_path: PathBuf::from("/tmp/sample.ogg"),
                bytes: vec![1, 2, 3],
            })),
            ..Default::default()
        });
        let tool = VoiceTool::new(archive, Arc::new(FakeVoiceRuntime));

        let output = tool
            .execute(
                json!({"action": "stt", "archive_id": "arch-1"}),
                &base_ctx(),
            )
            .await
            .expect("stt should succeed");

        assert!(output.content_for_model.contains("\"action\": \"stt\""));
        assert!(output.content_for_model.contains("transcribed 3 bytes"));
        assert!(
            output
                .content_for_model
                .contains("\"provider\": \"deepgram\"")
        );
    }

    #[tokio::test]
    async fn stt_rejects_non_audio_archive() {
        let archive = Arc::new(FakeArchiveService {
            blob: Mutex::new(Some(ArchiveBlob {
                record: sample_record("arch-1", ArchiveMediaKind::Image),
                absolute_path: PathBuf::from("/tmp/sample.png"),
                bytes: vec![1, 2, 3],
            })),
            ..Default::default()
        });
        let tool = VoiceTool::new(archive, Arc::new(FakeVoiceRuntime));

        let err = tool
            .execute(
                json!({"action": "stt", "archive_id": "arch-1"}),
                &base_ctx(),
            )
            .await
            .expect_err("non-audio should fail");

        assert!(err.to_string().contains("is not an audio file"));
    }

    #[tokio::test]
    async fn tts_archives_generated_audio() {
        let archive = Arc::new(FakeArchiveService::default());
        let tool = VoiceTool::new(archive.clone(), Arc::new(FakeVoiceRuntime));

        let output = tool
            .execute(
                json!({"action": "tts", "text": "hello", "filename": "reply.mp3"}),
                &base_ctx(),
            )
            .await
            .expect("tts should succeed");

        assert!(output.content_for_model.contains("\"action\": \"tts\""));
        assert!(
            output
                .content_for_model
                .contains("\"archive_id\": \"arch-generated\"")
        );

        let ingested = archive
            .ingested
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .expect("ingest_bytes should be called");
        assert_eq!(ingested.0.filename.as_deref(), Some("reply.mp3"));
        assert_eq!(ingested.0.channel.as_deref(), Some("telegram"));
        assert_eq!(ingested.0.chat_id.as_deref(), Some("chat-1"));
    }
}
