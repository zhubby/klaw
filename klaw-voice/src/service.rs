use crate::{
    VoiceError, VoiceProvider,
    model::{
        SttInput, SttOutput, SttStreamInput, SttStreamOutput, TtsInput, TtsOutput, TtsStreamInput,
        TtsStreamOutput,
    },
    providers::{AssemblyAiProvider, DeepgramProvider, ElevenLabsProvider},
};
use klaw_archive::{ArchiveIngestInput, ArchiveRecord, ArchiveService, ArchiveSourceKind};
use klaw_config::{SttProviderKind, TtsProviderKind, VoiceConfig};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct VoiceRuntimeConfig {
    pub stt_provider: SttProviderKind,
    pub tts_provider: TtsProviderKind,
    pub default_language: String,
    pub default_voice_id: Option<String>,
}

pub struct VoiceService {
    stt_provider: Arc<dyn VoiceProvider>,
    tts_provider: Arc<dyn VoiceProvider>,
    config: VoiceRuntimeConfig,
}

impl std::fmt::Debug for VoiceService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceService")
            .field("stt_provider", &self.stt_provider.name())
            .field("tts_provider", &self.tts_provider.name())
            .field("config", &self.config)
            .finish()
    }
}

impl VoiceService {
    pub fn from_config(config: &VoiceConfig) -> Result<Self, VoiceError> {
        if !config.enabled {
            return Err(VoiceError::Config(
                "voice service cannot be built when voice.enabled=false".to_string(),
            ));
        }

        let stt_provider: Arc<dyn VoiceProvider> = match config.stt_provider {
            SttProviderKind::Deepgram => {
                Arc::new(DeepgramProvider::new(&config.providers.deepgram)?)
            }
            SttProviderKind::Assemblyai => {
                Arc::new(AssemblyAiProvider::new(&config.providers.assemblyai)?)
            }
        };
        let tts_provider: Arc<dyn VoiceProvider> = match config.tts_provider {
            TtsProviderKind::Elevenlabs => {
                Arc::new(ElevenLabsProvider::new(&config.providers.elevenlabs)?)
            }
        };

        Ok(Self {
            stt_provider,
            tts_provider,
            config: VoiceRuntimeConfig {
                stt_provider: config.stt_provider,
                tts_provider: config.tts_provider,
                default_language: config.default_language.clone(),
                default_voice_id: config
                    .default_voice_id
                    .clone()
                    .or_else(|| config.providers.elevenlabs.default_voice_id.clone()),
            },
        })
    }

    #[must_use]
    pub fn runtime_config(&self) -> &VoiceRuntimeConfig {
        &self.config
    }

    #[must_use]
    pub fn stt_provider_name(&self) -> &'static str {
        self.stt_provider.name()
    }

    #[must_use]
    pub fn tts_provider_name(&self) -> &'static str {
        self.tts_provider.name()
    }

    pub async fn transcribe(&self, input: SttInput) -> Result<SttOutput, VoiceError> {
        self.stt_provider
            .transcribe(self.with_default_language(input))
            .await
    }

    pub async fn transcribe_stream(
        &self,
        input: SttStreamInput,
    ) -> Result<SttStreamOutput, VoiceError> {
        self.stt_provider
            .transcribe_stream(self.with_default_stream_language(input))
            .await
    }

    pub async fn synthesize(&self, input: TtsInput) -> Result<TtsOutput, VoiceError> {
        self.tts_provider
            .synthesize(self.with_tts_defaults(input))
            .await
    }

    pub async fn synthesize_stream(
        &self,
        input: TtsStreamInput,
    ) -> Result<TtsStreamOutput, VoiceError> {
        self.tts_provider
            .synthesize_stream(self.with_tts_stream_defaults(input))
            .await
    }

    pub async fn synthesize_and_archive(
        &self,
        input: TtsInput,
        archive: &dyn ArchiveService,
        mut archive_input: ArchiveIngestInput,
    ) -> Result<(TtsOutput, ArchiveRecord), VoiceError> {
        let output = self.synthesize(input).await?;
        archive_input.source_kind = ArchiveSourceKind::ModelGenerated;
        let record = archive
            .ingest_bytes(archive_input, &output.audio_bytes)
            .await?;
        Ok((output, record))
    }

    fn with_default_language(&self, mut input: SttInput) -> SttInput {
        if input
            .language
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            input.language = Some(self.config.default_language.clone());
        }
        input
    }

    fn with_default_stream_language(&self, mut input: SttStreamInput) -> SttStreamInput {
        if input
            .language
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            input.language = Some(self.config.default_language.clone());
        }
        input
    }

    fn with_tts_defaults(&self, mut input: TtsInput) -> TtsInput {
        if input
            .language
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            input.language = Some(self.config.default_language.clone());
        }
        if input
            .voice_id
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            input.voice_id = self.config.default_voice_id.clone();
        }
        input
    }

    fn with_tts_stream_defaults(&self, mut input: TtsStreamInput) -> TtsStreamInput {
        if input
            .language
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            input.language = Some(self.config.default_language.clone());
        }
        if input
            .voice_id
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
        {
            input.voice_id = self.config.default_voice_id.clone();
        }
        input
    }
}

#[cfg(test)]
mod tests {
    use super::VoiceService;
    use klaw_config::{SttProviderKind, TtsProviderKind, VoiceConfig};

    fn sample_voice_config() -> VoiceConfig {
        let mut config = VoiceConfig {
            enabled: true,
            stt_provider: SttProviderKind::Deepgram,
            tts_provider: TtsProviderKind::Elevenlabs,
            default_language: "zh-CN".to_string(),
            default_voice_id: Some("voice-1".to_string()),
            ..VoiceConfig::default()
        };
        config.providers.deepgram.api_key = Some("dg-test".to_string());
        config.providers.assemblyai.api_key = Some("aa-test".to_string());
        config.providers.elevenlabs.api_key = Some("el-test".to_string());
        config
    }

    #[test]
    fn voice_service_builds_from_config() {
        let service =
            VoiceService::from_config(&sample_voice_config()).expect("voice service should build");
        assert_eq!(service.stt_provider_name(), "deepgram");
        assert_eq!(service.tts_provider_name(), "elevenlabs");
        assert_eq!(service.runtime_config().default_language, "zh-CN");
        assert_eq!(
            service.runtime_config().default_voice_id.as_deref(),
            Some("voice-1")
        );
    }

    #[test]
    fn voice_service_uses_selected_assemblyai_stt_provider() {
        let mut config = sample_voice_config();
        config.stt_provider = SttProviderKind::Assemblyai;
        let service = VoiceService::from_config(&config).expect("voice service should build");
        assert_eq!(service.stt_provider_name(), "assemblyai");
    }
}
