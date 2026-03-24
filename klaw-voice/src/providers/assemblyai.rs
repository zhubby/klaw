use crate::{
    VoiceCapabilities, VoiceError, VoiceProvider,
    model::{
        SttInput, SttOutput, SttSegment, SttStreamInput, SttStreamOutput, TtsInput, TtsOutput,
        TtsStreamInput, TtsStreamOutput,
    },
    stream::transcript_stream_from_receiver,
};
use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use klaw_config::AssemblyAiVoiceConfig;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::{
    connect_async, tungstenite::http::Request, tungstenite::protocol::Message,
};

const ASSEMBLYAI_POLL_INTERVAL: Duration = Duration::from_secs(1);
const ASSEMBLYAI_POLL_ATTEMPTS: usize = 60;

#[derive(Debug, Clone)]
pub struct AssemblyAiProvider {
    http: reqwest::Client,
    config: AssemblyAiVoiceConfig,
    api_key: String,
}

impl AssemblyAiProvider {
    pub fn new(config: &AssemblyAiVoiceConfig) -> Result<Self, VoiceError> {
        let api_key = config
            .resolve_api_key()
            .ok_or(VoiceError::MissingApiKey("assemblyai"))?;
        Ok(Self {
            http: reqwest::Client::new(),
            config: config.clone(),
            api_key,
        })
    }

    fn upload_url(&self) -> String {
        format!("{}/v2/upload", self.config.base_url.trim_end_matches('/'))
    }

    fn transcript_url(&self) -> String {
        format!(
            "{}/v2/transcript",
            self.config.base_url.trim_end_matches('/')
        )
    }

    fn realtime_url(&self, sample_rate_hz: u32) -> String {
        format!(
            "{}/v2/realtime/ws?sample_rate={}",
            self.config.streaming_base_url.trim_end_matches('/'),
            sample_rate_hz
        )
    }

    async fn upload_audio(&self, audio_bytes: Vec<u8>) -> Result<String, VoiceError> {
        let response = self
            .http
            .post(self.upload_url())
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/octet-stream")
            .body(audio_bytes)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(VoiceError::InvalidResponse(format!(
                "assemblyai upload failed: HTTP {} body={}",
                status, body
            )));
        }
        let value: Value = serde_json::from_str(&body)?;
        value
            .get("upload_url")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| {
                VoiceError::InvalidResponse(
                    "assemblyai upload response missing upload_url".to_string(),
                )
            })
    }
}

#[async_trait]
impl VoiceProvider for AssemblyAiProvider {
    fn name(&self) -> &'static str {
        "assemblyai"
    }

    async fn transcribe(&self, input: SttInput) -> Result<SttOutput, VoiceError> {
        let upload_url = self.upload_audio(input.audio_bytes).await?;
        let create_response = self
            .http
            .post(self.transcript_url())
            .header("Authorization", &self.api_key)
            .json(&json!({
                "audio_url": upload_url,
                "speech_model": self.config.stt_model,
                "language_code": input.language,
            }))
            .send()
            .await?;
        let status = create_response.status();
        let body = create_response.text().await?;
        if !status.is_success() {
            return Err(VoiceError::InvalidResponse(format!(
                "assemblyai transcript create failed: HTTP {} body={}",
                status, body
            )));
        }
        let created: Value = serde_json::from_str(&body)?;
        let transcript_id = created.get("id").and_then(Value::as_str).ok_or_else(|| {
            VoiceError::InvalidResponse(
                "assemblyai transcript create response missing id".to_string(),
            )
        })?;
        for _ in 0..ASSEMBLYAI_POLL_ATTEMPTS {
            let response = self
                .http
                .get(format!("{}/{}", self.transcript_url(), transcript_id))
                .header("Authorization", &self.api_key)
                .send()
                .await?;
            let status = response.status();
            let body = response.text().await?;
            if !status.is_success() {
                return Err(VoiceError::InvalidResponse(format!(
                    "assemblyai transcript poll failed: HTTP {} body={}",
                    status, body
                )));
            }
            let value: Value = serde_json::from_str(&body)?;
            match value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "completed" => {
                    return Ok(SttOutput {
                        text: value
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        language: value
                            .get("language_code")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        confidence: value
                            .get("confidence")
                            .and_then(Value::as_f64)
                            .map(|value| value as f32),
                        duration_ms: value
                            .get("audio_duration")
                            .and_then(Value::as_f64)
                            .map(|seconds| (seconds * 1000.0) as u64),
                    });
                }
                "error" => {
                    return Err(VoiceError::InvalidResponse(
                        value
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("assemblyai transcript failed")
                            .to_string(),
                    ));
                }
                _ => sleep(ASSEMBLYAI_POLL_INTERVAL).await,
            }
        }
        Err(VoiceError::InvalidResponse(
            "assemblyai transcript polling timed out".to_string(),
        ))
    }

    async fn transcribe_stream(
        &self,
        input: SttStreamInput,
    ) -> Result<SttStreamOutput, VoiceError> {
        let sample_rate_hz = input.sample_rate_hz.unwrap_or(16_000);
        let request = Request::builder()
            .uri(self.realtime_url(sample_rate_hz))
            .header("Authorization", self.api_key.clone())
            .body(())
            .map_err(|err| VoiceError::WebSocket(err.to_string()))?;
        let (socket, _) = connect_async(request).await?;
        let (mut sink, mut stream) = socket.split();
        let (tx, rx) = mpsc::unbounded_channel();
        let mut audio_stream = input.audio_stream;

        let sender_tx = tx.clone();
        tokio::spawn(async move {
            while let Some(chunk) = audio_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        let frame = json!({
                            "audio_data": base64::engine::general_purpose::STANDARD.encode(bytes),
                        })
                        .to_string();
                        if let Err(err) = sink.send(Message::Text(frame.into())).await {
                            let _ = sender_tx.send(Err(VoiceError::from(err)));
                            return;
                        }
                    }
                    Err(err) => {
                        let _ = sender_tx.send(Err(err));
                        return;
                    }
                }
            }
            let _ = sink
                .send(Message::Text(
                    json!({
                        "terminate_session": true,
                    })
                    .to_string()
                    .into(),
                ))
                .await;
            let _ = sink.close().await;
        });

        tokio::spawn(async move {
            while let Some(message) = stream.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        let Ok(value): Result<Value, _> = serde_json::from_str(&text) else {
                            let _ = tx.send(Err(VoiceError::InvalidResponse(
                                "assemblyai stream emitted non-json text frame".to_string(),
                            )));
                            return;
                        };
                        let message_type = value
                            .get("message_type")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if !matches!(message_type, "PartialTranscript" | "FinalTranscript") {
                            continue;
                        }
                        let transcript = value
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        if transcript.is_empty() {
                            continue;
                        }
                        let _ = tx.send(Ok(SttSegment {
                            text: transcript,
                            is_final: message_type == "FinalTranscript",
                            confidence: value
                                .get("confidence")
                                .and_then(Value::as_f64)
                                .map(|value| value as f32),
                        }));
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(err) => {
                        let _ = tx.send(Err(VoiceError::from(err)));
                        break;
                    }
                }
            }
        });

        Ok(SttStreamOutput {
            transcript_stream: transcript_stream_from_receiver(rx),
        })
    }

    async fn synthesize(&self, _input: TtsInput) -> Result<TtsOutput, VoiceError> {
        Err(VoiceError::UnsupportedOperation {
            provider: self.name(),
            operation: "synthesize",
        })
    }

    async fn synthesize_stream(
        &self,
        _input: TtsStreamInput,
    ) -> Result<TtsStreamOutput, VoiceError> {
        Err(VoiceError::UnsupportedOperation {
            provider: self.name(),
            operation: "synthesize_stream",
        })
    }

    fn capabilities(&self) -> VoiceCapabilities {
        VoiceCapabilities {
            supports_streaming_stt: true,
            supports_streaming_tts: false,
            supported_languages: vec!["multi".to_string()],
            voice_ids: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AssemblyAiProvider;
    use crate::VoiceProvider;
    use klaw_config::AssemblyAiVoiceConfig;

    #[test]
    fn assemblyai_provider_requires_api_key() {
        let err = AssemblyAiProvider::new(&AssemblyAiVoiceConfig::default())
            .expect_err("provider should require api key");
        assert!(format!("{err}").contains("missing api key"));
    }

    #[test]
    fn assemblyai_provider_reports_streaming_stt_capability() {
        let provider = AssemblyAiProvider::new(&AssemblyAiVoiceConfig {
            api_key: Some("aa-test".to_string()),
            ..AssemblyAiVoiceConfig::default()
        })
        .expect("provider should build");
        let caps = provider.capabilities();
        assert!(caps.supports_streaming_stt);
        assert!(!caps.supports_streaming_tts);
    }
}
