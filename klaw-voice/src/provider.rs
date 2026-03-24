use crate::{
    VoiceError,
    model::{
        SttInput, SttOutput, SttStreamInput, SttStreamOutput, TtsInput, TtsOutput, TtsStreamInput,
        TtsStreamOutput,
    },
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceCapabilities {
    pub supports_streaming_stt: bool,
    pub supports_streaming_tts: bool,
    pub supported_languages: Vec<String>,
    pub voice_ids: Vec<String>,
}

#[async_trait]
pub trait VoiceProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn transcribe(&self, input: SttInput) -> Result<SttOutput, VoiceError>;

    async fn transcribe_stream(&self, input: SttStreamInput)
    -> Result<SttStreamOutput, VoiceError>;

    async fn synthesize(&self, input: TtsInput) -> Result<TtsOutput, VoiceError>;

    async fn synthesize_stream(&self, input: TtsStreamInput)
    -> Result<TtsStreamOutput, VoiceError>;

    fn capabilities(&self) -> VoiceCapabilities;
}
