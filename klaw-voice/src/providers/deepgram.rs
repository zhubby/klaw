use crate::{
    VoiceCapabilities, VoiceError, VoiceProvider,
    model::{
        SttInput, SttOutput, SttSegment, SttStreamInput, SttStreamOutput, TtsInput, TtsOutput,
        TtsStreamInput, TtsStreamOutput,
    },
    stream::transcript_stream_from_receiver,
};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use klaw_config::DeepgramVoiceConfig;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async, tungstenite::http::Request, tungstenite::protocol::Message,
};

#[derive(Debug, Clone)]
pub struct DeepgramProvider {
    http: reqwest::Client,
    config: DeepgramVoiceConfig,
    api_key: String,
}

impl DeepgramProvider {
    pub fn new(config: &DeepgramVoiceConfig) -> Result<Self, VoiceError> {
        let api_key = config
            .resolve_api_key()
            .ok_or(VoiceError::MissingApiKey("deepgram"))?;
        Ok(Self {
            http: reqwest::Client::new(),
            config: config.clone(),
            api_key,
        })
    }

    fn listen_url(&self, language: Option<&str>) -> String {
        let mut url = format!(
            "{}/v1/listen?model={}&smart_format=true&detect_language=true",
            self.config.base_url.trim_end_matches('/'),
            self.config.stt_model
        );
        if let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) {
            url.push_str("&language=");
            url.push_str(language);
        }
        url
    }

    fn stream_url(&self, language: Option<&str>) -> String {
        let mut url = format!(
            "{}/v1/listen?model={}&smart_format=true&interim_results=true&endpointing=300",
            self.config.streaming_base_url.trim_end_matches('/'),
            self.config.stt_model
        );
        if let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) {
            url.push_str("&language=");
            url.push_str(language);
        }
        url
    }
}

#[async_trait]
impl VoiceProvider for DeepgramProvider {
    fn name(&self) -> &'static str {
        "deepgram"
    }

    async fn transcribe(&self, input: SttInput) -> Result<SttOutput, VoiceError> {
        let response = self
            .http
            .post(self.listen_url(input.language.as_deref()))
            .header("Authorization", format!("Token {}", self.api_key))
            .header("Content-Type", input.mime_type)
            .body(input.audio_bytes)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(VoiceError::InvalidResponse(format!(
                "deepgram transcribe failed: HTTP {} body={}",
                status, body
            )));
        }
        let value: Value = serde_json::from_str(&body)?;
        let alternative = value
            .get("results")
            .and_then(|value| value.get("channels"))
            .and_then(Value::as_array)
            .and_then(|channels| channels.first())
            .and_then(|value| value.get("alternatives"))
            .and_then(Value::as_array)
            .and_then(|alternatives| alternatives.first())
            .ok_or_else(|| {
                VoiceError::InvalidResponse(
                    "deepgram response missing results.channels[0].alternatives[0]".to_string(),
                )
            })?;
        Ok(SttOutput {
            text: alternative
                .get("transcript")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            language: value
                .get("results")
                .and_then(|value| value.get("channels"))
                .and_then(Value::as_array)
                .and_then(|channels| channels.first())
                .and_then(|value| value.get("detected_language"))
                .and_then(Value::as_str)
                .map(ToString::to_string),
            confidence: alternative
                .get("confidence")
                .and_then(Value::as_f64)
                .map(|value| value as f32),
            duration_ms: value
                .get("metadata")
                .and_then(|value| value.get("duration"))
                .and_then(Value::as_f64)
                .map(|seconds| (seconds * 1000.0) as u64),
        })
    }

    async fn transcribe_stream(
        &self,
        input: SttStreamInput,
    ) -> Result<SttStreamOutput, VoiceError> {
        let request = Request::builder()
            .uri(self.stream_url(input.language.as_deref()))
            .header("Authorization", format!("Token {}", self.api_key))
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
                        if let Err(err) = sink.send(Message::Binary(bytes)).await {
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
                        "type": "CloseStream",
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
                                "deepgram stream emitted non-json text frame".to_string(),
                            )));
                            return;
                        };
                        let Some(alternative) = value
                            .get("channel")
                            .and_then(|value| value.get("alternatives"))
                            .and_then(Value::as_array)
                            .and_then(|alternatives| alternatives.first())
                        else {
                            continue;
                        };
                        let transcript = alternative
                            .get("transcript")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        if transcript.is_empty() {
                            continue;
                        }
                        let _ = tx.send(Ok(SttSegment {
                            text: transcript,
                            is_final: value
                                .get("is_final")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            confidence: alternative
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
    use super::DeepgramProvider;
    use crate::VoiceProvider;
    use klaw_config::DeepgramVoiceConfig;

    #[test]
    fn deepgram_provider_requires_api_key() {
        let err = DeepgramProvider::new(&DeepgramVoiceConfig::default())
            .expect_err("provider should require api key");
        assert!(format!("{err}").contains("missing api key"));
    }

    #[test]
    fn deepgram_provider_reports_streaming_stt_capability() {
        let provider = DeepgramProvider::new(&DeepgramVoiceConfig {
            api_key: Some("dg-test".to_string()),
            ..DeepgramVoiceConfig::default()
        })
        .expect("provider should build");
        let caps = provider.capabilities();
        assert!(caps.supports_streaming_stt);
        assert!(!caps.supports_streaming_tts);
    }
}
