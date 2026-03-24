use crate::{
    VoiceCapabilities, VoiceError, VoiceProvider,
    model::{
        SttInput, SttOutput, SttStreamInput, SttStreamOutput, TtsInput, TtsOutput, TtsStreamInput,
        TtsStreamOutput,
    },
    stream::byte_stream_from_receiver,
};
use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use klaw_config::ElevenLabsVoiceConfig;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async, tungstenite::http::Request, tungstenite::protocol::Message,
};

#[derive(Debug, Clone)]
pub struct ElevenLabsProvider {
    http: reqwest::Client,
    config: ElevenLabsVoiceConfig,
    api_key: String,
}

impl ElevenLabsProvider {
    pub fn new(config: &ElevenLabsVoiceConfig) -> Result<Self, VoiceError> {
        let api_key = config
            .resolve_api_key()
            .ok_or(VoiceError::MissingApiKey("elevenlabs"))?;
        Ok(Self {
            http: reqwest::Client::new(),
            config: config.clone(),
            api_key,
        })
    }

    fn selected_voice_id(&self, input_voice_id: Option<&str>) -> Result<String, VoiceError> {
        input_voice_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| self.config.default_voice_id.clone())
            .ok_or_else(|| {
                VoiceError::Config(
                    "elevenlabs voice_id is required when no default_voice_id is configured"
                        .to_string(),
                )
            })
    }

    fn synthesize_url(&self, voice_id: &str) -> String {
        format!(
            "{}/v1/text-to-speech/{}",
            self.config.base_url.trim_end_matches('/'),
            voice_id
        )
    }

    fn stream_url(&self, voice_id: &str) -> String {
        format!(
            "{}/v1/text-to-speech/{}/stream-input?model_id={}",
            self.config.streaming_base_url.trim_end_matches('/'),
            voice_id,
            self.config.default_model
        )
    }
}

#[async_trait]
impl VoiceProvider for ElevenLabsProvider {
    fn name(&self) -> &'static str {
        "elevenlabs"
    }

    async fn transcribe(&self, _input: SttInput) -> Result<SttOutput, VoiceError> {
        Err(VoiceError::UnsupportedOperation {
            provider: self.name(),
            operation: "transcribe",
        })
    }

    async fn transcribe_stream(
        &self,
        _input: SttStreamInput,
    ) -> Result<SttStreamOutput, VoiceError> {
        Err(VoiceError::UnsupportedOperation {
            provider: self.name(),
            operation: "transcribe_stream",
        })
    }

    async fn synthesize(&self, input: TtsInput) -> Result<TtsOutput, VoiceError> {
        let voice_id = self.selected_voice_id(input.voice_id.as_deref())?;
        let response = self
            .http
            .post(self.synthesize_url(&voice_id))
            .header("xi-api-key", &self.api_key)
            .header("Accept", "audio/mpeg")
            .json(&json!({
                "text": input.text,
                "model_id": self.config.default_model,
                "voice_settings": {
                    "speed": input.speed,
                }
            }))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await?;
            return Err(VoiceError::InvalidResponse(format!(
                "elevenlabs synthesize failed: HTTP {} body={}",
                status, body
            )));
        }
        let mime_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("audio/mpeg")
            .to_string();
        let audio_bytes = response.bytes().await?.to_vec();
        Ok(TtsOutput {
            audio_bytes,
            mime_type,
            duration_ms: None,
        })
    }

    async fn synthesize_stream(
        &self,
        input: TtsStreamInput,
    ) -> Result<TtsStreamOutput, VoiceError> {
        let voice_id = self.selected_voice_id(input.voice_id.as_deref())?;
        let request = Request::builder()
            .uri(self.stream_url(&voice_id))
            .body(())
            .map_err(|err| VoiceError::WebSocket(err.to_string()))?;
        let (mut socket, _) = connect_async(request).await?;
        let (tx, rx) = mpsc::unbounded_channel();
        let api_key = self.api_key.clone();
        let text = input.text.clone();
        let speed = input.speed;

        tokio::spawn(async move {
            let init_frame = json!({
                "text": " ",
                "xi_api_key": api_key,
                "voice_settings": {
                    "speed": speed,
                }
            });
            if let Err(err) = socket
                .send(Message::Text(init_frame.to_string().into()))
                .await
            {
                let _ = tx.send(Err(VoiceError::from(err)));
                return;
            }
            if let Err(err) = socket
                .send(Message::Text(
                    json!({
                        "text": text,
                        "try_trigger_generation": true,
                    })
                    .to_string()
                    .into(),
                ))
                .await
            {
                let _ = tx.send(Err(VoiceError::from(err)));
                return;
            }
            if let Err(err) = socket
                .send(Message::Text(
                    json!({
                        "text": "",
                    })
                    .to_string()
                    .into(),
                ))
                .await
            {
                let _ = tx.send(Err(VoiceError::from(err)));
                return;
            }

            while let Some(message) = socket.next().await {
                match message {
                    Ok(Message::Text(text)) => {
                        let Ok(value): Result<Value, _> = serde_json::from_str(&text) else {
                            let _ = tx.send(Err(VoiceError::InvalidResponse(
                                "elevenlabs stream emitted non-json text frame".to_string(),
                            )));
                            return;
                        };
                        if let Some(audio) = value.get("audio").and_then(Value::as_str) {
                            match base64::engine::general_purpose::STANDARD.decode(audio) {
                                Ok(bytes) => {
                                    let _ = tx.send(Ok(bytes.into()));
                                }
                                Err(err) => {
                                    let _ =
                                        tx.send(Err(VoiceError::InvalidResponse(err.to_string())));
                                    return;
                                }
                            }
                        }
                        if value
                            .get("isFinal")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            break;
                        }
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

        Ok(TtsStreamOutput {
            audio_stream: byte_stream_from_receiver(rx),
            mime_type: "audio/mpeg".to_string(),
        })
    }

    fn capabilities(&self) -> VoiceCapabilities {
        VoiceCapabilities {
            supports_streaming_stt: false,
            supports_streaming_tts: true,
            supported_languages: vec!["multi".to_string()],
            voice_ids: self
                .config
                .default_voice_id
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ElevenLabsProvider;
    use crate::VoiceProvider;
    use klaw_config::ElevenLabsVoiceConfig;

    #[test]
    fn elevenlabs_provider_requires_api_key() {
        let err = ElevenLabsProvider::new(&ElevenLabsVoiceConfig::default())
            .expect_err("provider should require api key");
        assert!(format!("{err}").contains("missing api key"));
    }

    #[test]
    fn elevenlabs_provider_reports_streaming_tts_capability() {
        let provider = ElevenLabsProvider::new(&ElevenLabsVoiceConfig {
            api_key: Some("el-test".to_string()),
            default_voice_id: Some("voice-1".to_string()),
            ..ElevenLabsVoiceConfig::default()
        })
        .expect("provider should build");
        let caps = provider.capabilities();
        assert!(!caps.supports_streaming_stt);
        assert!(caps.supports_streaming_tts);
        assert_eq!(caps.voice_ids, vec!["voice-1".to_string()]);
    }
}
